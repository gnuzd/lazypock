//! Deleting objects only after the transaction that orphaned them commits.
//!
//! Removing an object inside the transaction would be wrong in both directions:
//! the object would be gone if the transaction rolled back, and a crash between
//! commit and delete would leak it. So the deletion is *queued* in the same
//! transaction and carried out afterwards.

use lazypock_core::error::Result;
use lazypock_db::pool::db_error;
use lazypock_db::runner::QueryRunner;

use crate::storage::Storage;

/// Most entries one drain pass handles.
pub const BATCH: i64 = 100;

/// Longest retry delay, in seconds.
const MAX_BACKOFF: i64 = 3600;

/// Outcome of one drain pass.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DrainReport {
    /// Objects removed.
    pub deleted: u64,
    /// Objects that could not be removed and were rescheduled.
    pub failed: u64,
}

/// Queue an object for deletion.
///
/// Call this inside the transaction that removed the reference, so the queue
/// entry and the change commit together.
pub async fn enqueue(runner: &dyn QueryRunner, key: &str) -> Result<()> {
    runner
        .run_execute(
            "insert into _file_outbox (id, storage_key) values ($1::text, $2::text)",
            &[&lazypock_core::id::new_id(), &key.to_string()],
        )
        .await
        .map_err(db_error)?;
    Ok(())
}

/// Queue several objects at once.
pub async fn enqueue_all(runner: &dyn QueryRunner, keys: &[String]) -> Result<()> {
    for key in keys {
        enqueue(runner, key).await?;
    }
    Ok(())
}

/// Delete the queued objects, dropping the entries that succeed.
///
/// A failure is recorded and retried later with a backoff, so a backend outage
/// does not lose the deletion.
pub async fn drain(runner: &dyn QueryRunner, storage: &dyn Storage) -> Result<DrainReport> {
    let rows = runner
        .run_query(
            "select id, storage_key from _file_outbox \
             where run_at <= now() order by run_at limit $1::text::bigint",
            &[&BATCH.to_string()],
        )
        .await
        .map_err(db_error)?;

    let mut report = DrainReport::default();
    for row in &rows {
        let id: String = row.get(0);
        let key: String = row.get(1);

        match storage.delete(&key).await {
            Ok(()) => {
                runner
                    .run_execute("delete from _file_outbox where id = $1::text", &[&id])
                    .await
                    .map_err(db_error)?;
                report.deleted += 1;
            }
            Err(err) => {
                runner
                    .run_execute(
                        "update _file_outbox set attempts = attempts + 1, last_error = $1::text, \
                         run_at = now() + make_interval(secs => least($3::text::bigint, \
                             30 * (attempts + 1))) \
                         where id = $2::text",
                        &[&err.message().to_string(), &id, &MAX_BACKOFF.to_string()],
                    )
                    .await
                    .map_err(db_error)?;
                report.failed += 1;
                tracing::warn!(
                    key = %key,
                    error = %err,
                    "could not delete a stored object; it will be retried"
                );
            }
        }
    }

    Ok(report)
}

/// How many deletions are waiting.
pub async fn pending(runner: &dyn QueryRunner) -> Result<i64> {
    let row = runner
        .run_query_one("select count(*) from _file_outbox", &[])
        .await
        .map_err(db_error)?;
    Ok(row.get(0))
}
