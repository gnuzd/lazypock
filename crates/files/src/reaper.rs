//! Removing objects whose rows are gone, and uploads nobody ever attached.
//!
//! Both jobs are **database first, storage second**, and one worker does both:
//!
//! * [`store::due_deletions`] — the queue the `_files_deleted` trigger fills inside
//!   the deleting transaction. Each entry's source object and its variants are
//!   removed from the backend and the entry is dropped. A backend failure is
//!   rescheduled with an exponential backoff, so a storage outage delays cleanup
//!   instead of losing it.
//! * [`store::unattached_older_than`] — an upload from the editor or the library that
//!   was never attached to a record, older than the configured TTL. Deleting the row
//!   queues its object through the same trigger, so there is one deletion path, not
//!   two.
//!
//! The pre-existing key-based `_file_outbox` is drained here too: it is what the
//! explicit key-addressed delete and the orphan sweeper write, and leaving it to a
//! separate worker would mean two places to look when storage does not lose an
//! object it should have.

use std::sync::Arc;

use lazypock_core::error::Result;
use lazypock_db::runner::QueryRunner;

use crate::outbox;
use crate::storage::Storage;
use crate::store;

/// How long an unattached upload is kept before the reaper reclaims it.
pub const DEFAULT_UNATTACHED_TTL_SECONDS: i64 = 24 * 60 * 60;

/// The longest a failed deletion waits before being retried.
pub const DEFAULT_MAX_BACKOFF_SECONDS: i64 = 24 * 60 * 60;

/// How many entries one pass handles.
pub const DEFAULT_BATCH: i64 = 100;

/// What one pass did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ReaperReport {
    /// Objects removed (both queues).
    pub deleted: u64,
    /// Deletions that failed and were rescheduled.
    pub failed: u64,
    /// Unattached rows reclaimed.
    pub reclaimed: u64,
    /// Entries left alone because they belong to a different backend.
    pub skipped_other_backend: u64,
}

impl ReaperReport {
    /// Whether the pass did anything at all.
    pub fn is_empty(&self) -> bool {
        self.deleted == 0 && self.failed == 0 && self.reclaimed == 0
    }
}

/// The worker: one storage backend, and the rules for what may go.
pub struct Reaper {
    storage: Arc<dyn Storage>,
    /// The backend this server is configured with, as `_files.storage_backend` names
    /// it. Entries recorded for another backend are left alone.
    backend: String,
    unattached_ttl_seconds: i64,
    max_backoff_seconds: i64,
    batch: i64,
}

impl Reaper {
    /// A reaper for one backend.
    pub fn new(storage: Arc<dyn Storage>, backend: impl Into<String>) -> Self {
        Self {
            storage,
            backend: backend.into(),
            unattached_ttl_seconds: DEFAULT_UNATTACHED_TTL_SECONDS,
            max_backoff_seconds: DEFAULT_MAX_BACKOFF_SECONDS,
            batch: DEFAULT_BATCH,
        }
    }

    /// How long an unattached upload is kept. Comes from `files.rules` in settings.
    pub fn with_unattached_ttl(mut self, seconds: i64) -> Self {
        self.unattached_ttl_seconds = seconds.max(0);
        self
    }

    /// How many entries one pass handles.
    pub fn with_batch(mut self, batch: i64) -> Self {
        self.batch = batch.clamp(1, 10_000);
        self
    }

    /// The backend this reaper deletes from.
    pub fn backend(&self) -> &str {
        &self.backend
    }

    /// How long an unattached upload is kept.
    pub fn unattached_ttl_seconds(&self) -> i64 {
        self.unattached_ttl_seconds
    }

    /// Run one pass: the key-based outbox, then the row-based deletions, then the
    /// abandoned uploads.
    pub async fn run(&self, runner: &dyn QueryRunner) -> Result<ReaperReport> {
        let mut report = ReaperReport::default();

        // The key-based queue first: an entry there is an object nothing references
        // any more, so it is the most likely to be leaked.
        let drained = outbox::drain(runner, self.storage.as_ref()).await?;
        report.deleted += drained.deleted;
        report.failed += drained.failed;

        for entry in store::due_deletions(runner, self.batch).await? {
            if !self.owns(&entry) {
                report.skipped_other_backend += 1;
                continue;
            }

            let keys = entry.keys();
            match self.remove_all(&keys).await {
                Ok(()) => {
                    store::finish_deletion(runner, entry.id).await?;
                    report.deleted += 1;
                }
                Err(err) => {
                    store::defer_deletion(
                        runner,
                        entry.id,
                        err.message(),
                        self.max_backoff_seconds,
                    )
                    .await?;
                    report.failed += 1;
                    tracing::warn!(
                        file_id = %entry.file_id,
                        attempts = entry.attempts + 1,
                        error = %err,
                        "could not remove a stored object; it will be retried"
                    );
                }
            }
        }

        // Abandoned uploads last: deleting the row queues the object for the *next*
        // pass, which keeps every object removal in one code path.
        for file in
            store::unattached_older_than(runner, self.unattached_ttl_seconds, self.batch).await?
        {
            if file.storage_backend != self.backend {
                report.skipped_other_backend += 1;
                continue;
            }
            store::delete(runner, &file.id).await?;
            report.reclaimed += 1;
        }

        // A reference to a file that is gone (a no-op when everything lines up).
        store::prune_refs(runner).await?;

        if !report.is_empty() {
            tracing::info!(
                deleted = report.deleted,
                failed = report.failed,
                reclaimed = report.reclaimed,
                skipped_other_backend = report.skipped_other_backend,
                "file reaper finished a pass"
            );
        }

        Ok(report)
    }

    /// Whether an entry was recorded for the backend this server owns.
    ///
    /// A key is only unique within a backend, so deleting `posts/p1/abc.png` from a
    /// different backend than the one that stored it would delete somebody else's
    /// object. The entry is left for an operator who switches that backend back.
    fn owns(&self, entry: &store::PendingDeletion) -> bool {
        entry.backend == self.backend
    }

    /// Remove every key, reporting the first failure.
    async fn remove_all(&self, keys: &[String]) -> Result<()> {
        for key in keys {
            self.storage.delete(key).await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_a_day_and_a_hundred_entries() {
        let reaper = Reaper::new(
            Arc::new(crate::storage::LocalStorage::new(
                "/tmp/lazypock-reaper-test",
            )),
            "local",
        );
        assert_eq!(
            reaper.unattached_ttl_seconds(),
            DEFAULT_UNATTACHED_TTL_SECONDS
        );
        assert_eq!(reaper.backend(), "local");
    }

    #[test]
    fn a_batch_may_not_be_zero() {
        let reaper = Reaper::new(
            Arc::new(crate::storage::LocalStorage::new(
                "/tmp/lazypock-reaper-test",
            )),
            "local",
        )
        .with_batch(0);
        assert_eq!(reaper.batch, 1);
    }

    #[test]
    fn an_entry_from_another_backend_is_not_ours() {
        let reaper = Reaper::new(
            Arc::new(crate::storage::LocalStorage::new(
                "/tmp/lazypock-reaper-test",
            )),
            "local",
        );
        let entry = store::PendingDeletion {
            id: 1,
            file_id: "f1".into(),
            backend: "s3".into(),
            storage_path: "posts/p1/abc.png".into(),
            variants: serde_json::json!({}),
            attempts: 0,
        };
        assert!(!reaper.owns(&entry));

        let ours = store::PendingDeletion {
            backend: "local".into(),
            ..entry
        };
        assert!(reaper.owns(&ours));
    }
}
