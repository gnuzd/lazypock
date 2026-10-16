//! Keeping the in-RAM schema cache fresh at runtime.
//!
//! Every schema mutation emits `NOTIFY lazypock_schema_changed` at commit
//! ([`crate::collections::SCHEMA_CHANGED_CHANNEL`]). This module watches that
//! channel and reloads the cache, so a collection created on one node becomes
//! visible on every node without a restart.
//!
//! The watch uses its **own dedicated connection**, built by
//! [`crate::listen::ListenSession`] so it uses the TLS the URL asks for: that
//! connection used to be opened with `NoTls`, which cannot connect to a server
//! requiring TLS at all. The symptom was not an obvious one — the API kept
//! serving, but the cache stayed on its boot snapshot, so a collection created
//! afterwards answered `no collection named "…"` until the node was restarted.
//!
//! The watch also *reports itself*: while it cannot connect, it is recorded in
//! [`lazypock_core::health`], which `/api/health` exposes. Without that the node
//! looked healthy while realtime and live schema reload were both dead.

use std::sync::Arc;
use std::time::Duration;

use lazypock_core::error::Error;
use lazypock_core::health;

use crate::collections::SCHEMA_CHANGED_CHANNEL;
use crate::listen::ListenSession;
use crate::pool::Db;
use crate::schema::SchemaCache;

/// How long to wait before the first reconnect attempt, and after a connection
/// that had been working ends.
const RECONNECT_DELAY: Duration = Duration::from_secs(1);

/// The longest wait between attempts.
///
/// The watcher used to retry every second, forever. Against a failure that cannot
/// succeed — a TLS handshake from a plaintext-only client, say — that is a warning
/// a second indefinitely, and it buries the one line that says what is wrong while
/// growing a log nobody can read. Backing off keeps the retry and makes the log
/// useful.
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(60);

/// A running schema watcher.
///
/// Dropping it stops the watch.
#[derive(Debug)]
pub struct SchemaWatcher {
    handle: tokio::task::JoinHandle<()>,
}

impl SchemaWatcher {
    /// Stop watching.
    pub fn stop(self) {
        self.handle.abort();
    }
}

impl Drop for SchemaWatcher {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Why a watch session ended.
///
/// The distinction decides the retry delay: a connection that was live and then
/// dropped is worth reconnecting to at once, while one that never came up is not.
enum Failure {
    /// The connection was never established.
    NotConnected(Error),
    /// It was established, and then ended.
    Disconnected(Error),
}

/// Watch for schema changes and refresh `cache` whenever one lands.
///
/// `on_reload` runs after every successful reload, on the watcher task. The app uses
/// it to tell the realtime registry subscribers that the collection set may have
/// changed — every node reloads on its own connection, so every node announces it
/// locally, whichever node made the change.
pub fn watch_schema(
    cache: Arc<SchemaCache>,
    db: Db,
    url: String,
    on_reload: Option<Arc<dyn Fn() + Send + Sync>>,
) -> SchemaWatcher {
    let handle = tokio::spawn(async move {
        let mut delay = RECONNECT_DELAY;
        loop {
            if let Err(failure) = watch_once(&cache, &db, &url, on_reload.as_deref()).await {
                let (err, was_connected) = match failure {
                    Failure::NotConnected(err) => (err, false),
                    Failure::Disconnected(err) => (err, true),
                };

                // Report it rather than only logging it: this is the difference
                // between "the node is healthy" and "the cache is frozen".
                health::set_unavailable(health::SCHEMA_WATCH, err.message());

                // A dropped connection reconnects promptly; a refused one backs
                // off.
                if was_connected {
                    delay = RECONNECT_DELAY;
                }
                tracing::warn!(
                    error = %err,
                    retry_in_secs = delay.as_secs(),
                    "schema watcher disconnected; reconnecting"
                );
            }

            // A dropped `LISTEN` connection must never take the node down: the
            // cache keeps serving the last good snapshot while we retry.
            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(MAX_RECONNECT_DELAY);
        }
    });
    SchemaWatcher { handle }
}

/// Watch once, returning only when the session ends.
async fn watch_once(
    cache: &SchemaCache,
    db: &Db,
    url: &str,
    on_reload: Option<&(dyn Fn() + Send + Sync)>,
) -> std::result::Result<(), Failure> {
    let mut session = ListenSession::connect(url).await.map_err(|err| {
        Failure::NotConnected(Error::internal(format!(
            "schema watcher cannot connect: {}",
            err.message()
        )))
    })?;

    session
        .listen(SCHEMA_CHANGED_CHANNEL)
        .await
        .map_err(|err| {
            Failure::NotConnected(Error::internal(format!(
                "schema watcher cannot listen: {}",
                err.message()
            )))
        })?;

    // `LISTEN` is established, so whatever was reported is no longer true.
    health::set_available(health::SCHEMA_WATCH);
    tracing::debug!(
        channel = SCHEMA_CHANGED_CHANNEL,
        "watching for schema changes"
    );

    // Reload once now that `LISTEN` is established. Without this there is a
    // window between the boot-time load and `LISTEN` in which a schema change
    // would be missed, leaving the node on a stale snapshot until the next one.
    reload(cache, db).await;
    if let Some(on_reload) = on_reload {
        on_reload();
    }

    while session.next().await.is_some() {
        reload(cache, db).await;
        if let Some(on_reload) = on_reload {
            on_reload();
        }
    }

    Err(Failure::Disconnected(Error::internal(
        "the schema watch connection closed",
    )))
}

/// Refresh the cache, keeping the previous snapshot on any failure.
async fn reload(cache: &SchemaCache, db: &Db) {
    match db.client().await {
        Ok(client) => match cache.reload(&client).await {
            Ok(schema) => tracing::info!(collections = schema.len(), "schema cache reloaded"),
            Err(err) => tracing::warn!(
                error = %err,
                "schema reload failed; keeping the previous snapshot"
            ),
        },
        Err(err) => tracing::warn!(
            error = %err,
            "schema reload could not get a connection; will retry on the next change"
        ),
    }
}
