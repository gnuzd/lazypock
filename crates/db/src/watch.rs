//! Keeping the in-RAM schema cache fresh at runtime.
//!
//! Every schema mutation emits `NOTIFY lazypock_schema_changed` at commit
//! ([`crate::collections::SCHEMA_CHANGED_CHANNEL`]). This module watches that
//! channel and reloads the cache, so a collection created on one node becomes
//! visible on every node without a restart.
//!
//! The watch uses its **own dedicated connection**: `LISTEN` is session-scoped,
//! so it cannot ride on a pooled connection the rest of the server keeps
//! recycling. That is also why the direct endpoint is required (PLAN.md 18.1).

use std::sync::Arc;
use std::time::Duration;

use lazypock_core::error::{Error, Result};
use tokio_postgres::NoTls;

use crate::collections::SCHEMA_CHANGED_CHANNEL;
use crate::pool::Db;
use crate::schema::SchemaCache;

/// How long to wait before reconnecting after the watch connection drops.
const RECONNECT_DELAY: Duration = Duration::from_secs(1);

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

/// Watch for schema changes and refresh `cache` whenever one lands.
pub fn watch_schema(cache: Arc<SchemaCache>, db: Db, url: String) -> SchemaWatcher {
    let handle = tokio::spawn(async move {
        loop {
            if let Err(err) = watch_once(&cache, &db, &url).await {
                // A dropped LISTEN connection must never take the node down:
                // the cache keeps serving the last good snapshot while we retry.
                tracing::warn!(error = %err, "schema watcher disconnected; reconnecting");
            }
            tokio::time::sleep(RECONNECT_DELAY).await;
        }
    });
    SchemaWatcher { handle }
}

async fn watch_once(cache: &SchemaCache, db: &Db, url: &str) -> Result<()> {
    let (client, mut connection) = tokio_postgres::connect(url, NoTls)
        .await
        .map_err(|err| Error::internal(format!("schema watcher cannot connect: {err}")))?;

    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

    // The connection has to be driven for `LISTEN` to complete, so the driver
    // task starts first and forwards notifications.
    let driver = tokio::spawn(async move {
        let _ = std::future::poll_fn(|cx| loop {
            match connection.poll_message(cx) {
                std::task::Poll::Ready(Some(Ok(tokio_postgres::AsyncMessage::Notification(
                    notification,
                )))) => {
                    let _ = sender.send(notification);
                }
                std::task::Poll::Ready(Some(Ok(_))) => continue,
                std::task::Poll::Ready(Some(Err(_)) | None) => return std::task::Poll::Ready(()),
                std::task::Poll::Pending => return std::task::Poll::Pending,
            }
        })
        .await;
    });

    client
        .batch_execute(&format!("listen {SCHEMA_CHANGED_CHANNEL}"))
        .await
        .map_err(|err| Error::internal(format!("schema watcher cannot listen: {err}")))?;

    // `LISTEN` is session-scoped: dropping the `Client` closes that session and
    // with it the notification stream. The handle is therefore held for the
    // whole watch rather than dropped once `LISTEN` has been issued.
    let session = client;
    tracing::debug!(
        channel = SCHEMA_CHANGED_CHANNEL,
        "watching for schema changes"
    );

    // Reload once now that `LISTEN` is established. Without this there is a
    // window between the boot-time load and `LISTEN` in which a schema change
    // would be missed, leaving the node on a stale snapshot until the next one.
    reload(cache, db).await;

    while receiver.recv().await.is_some() {
        reload(cache, db).await;
    }

    drop(session);
    driver.abort();
    Err(Error::internal("the schema watch connection closed"))
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
