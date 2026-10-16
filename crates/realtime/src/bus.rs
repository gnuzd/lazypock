//! Where change events come from.

use std::future::Future;
use std::pin::Pin;

use lazypock_core::error::{Error, Result};
use tokio_postgres::NoTls;

use crate::event::ChangeEvent;

/// The channel every node listens on.
pub const CHANNEL: &str = "lazypock_rt";

/// A boxed, `Send` future, matching the shape used elsewhere in the workspace.
pub type BusFuture<'a> = Pin<Box<dyn Future<Output = Option<ChangeEvent>> + Send + 'a>>;

/// A source of change events.
///
/// Postgres `LISTEN/NOTIFY` is the default. A Redis backend can implement this
/// without the dispatcher changing (`PLAN.md` section 8.8).
pub trait EventBus: Send {
    /// The next event, or `None` once the bus has closed.
    fn next(&mut self) -> BusFuture<'_>;
}

/// The Postgres bus: one dedicated `LISTEN` connection per node.
pub struct PgBus {
    receiver: tokio::sync::mpsc::UnboundedReceiver<ChangeEvent>,
    /// Drives the connection and forwards notifications.
    _driver: tokio::task::JoinHandle<()>,
    /// Keeps the session — and so the `LISTEN` — alive.
    ///
    /// Dropping the client closes the connection and ends the subscription, which
    /// is an easy mistake to make and a silent one to debug: the bus simply stops
    /// delivering.
    _session: tokio::task::JoinHandle<()>,
}

impl PgBus {
    /// Connect and start listening.
    ///
    /// The connection is its own, deliberately: `LISTEN` is session-scoped, so it
    /// cannot ride on a pooled connection the rest of the server keeps recycling.
    /// That is also why the direct endpoint is required (`PLAN.md` section 18.1).
    pub async fn connect(url: &str) -> Result<Self> {
        let (client, mut connection) =
            tokio_postgres::connect(url, NoTls).await.map_err(|err| {
                Error::internal(format!("the realtime listener cannot connect: {err}"))
            })?;

        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();

        // The connection has to be driven for `LISTEN` to complete, so the driver
        // starts first and forwards notifications.
        let driver = tokio::spawn(async move {
            let _ = std::future::poll_fn(|cx| loop {
                match connection.poll_message(cx) {
                    std::task::Poll::Ready(Some(Ok(
                        tokio_postgres::AsyncMessage::Notification(notification),
                    ))) => match ChangeEvent::parse(notification.payload()) {
                        Ok(event) => {
                            let _ = sender.send(event);
                        }
                        Err(err) => {
                            tracing::warn!(error = %err, "ignoring an unreadable realtime payload")
                        }
                    },
                    std::task::Poll::Ready(Some(Ok(_))) => continue,
                    std::task::Poll::Ready(Some(Err(_)) | None) => {
                        return std::task::Poll::Ready(())
                    }
                    std::task::Poll::Pending => return std::task::Poll::Pending,
                }
            })
            .await;
        });

        client
            .batch_execute(&format!("listen {CHANNEL}"))
            .await
            .map_err(|err| {
                Error::internal(format!("the realtime listener cannot listen: {err}"))
            })?;

        let session = tokio::spawn(async move {
            let _client = client;
            std::future::pending::<()>().await;
        });

        tracing::debug!(channel = CHANNEL, "listening for realtime changes");

        Ok(Self {
            receiver,
            _driver: driver,
            _session: session,
        })
    }
}

impl EventBus for PgBus {
    fn next(&mut self) -> BusFuture<'_> {
        Box::pin(async move { self.receiver.recv().await })
    }
}
