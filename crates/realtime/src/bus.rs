//! Where change events come from.

use std::future::Future;
use std::pin::Pin;

use lazypock_core::error::{Error, Result};
use lazypock_core::health;
use lazypock_db::listen::ListenSession;

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
    /// Held for the life of the bus: dropping the session would drop the
    /// connection and silently end the subscription.
    session: ListenSession,
}

impl PgBus {
    /// Connect and start listening.
    ///
    /// The connection is its own, deliberately: `LISTEN` is session-scoped, so it
    /// cannot ride on a pooled connection the rest of the server keeps recycling.
    /// It is built by [`ListenSession`] so that it uses the TLS the URL asks for.
    /// This used to pass `NoTls` directly, which cannot connect to a server that
    /// requires TLS — so on any TLS deployment realtime never connected at all,
    /// and the only trace was a single error at startup.
    pub async fn connect(url: &str) -> Result<Self> {
        let session = ListenSession::connect(url).await.map_err(|err| {
            Error::internal(format!(
                "the realtime listener cannot connect: {}",
                err.message()
            ))
        })?;

        session.listen(CHANNEL).await.map_err(|err| {
            Error::internal(format!(
                "the realtime listener cannot listen: {}",
                err.message()
            ))
        })?;

        // Reported, so a node whose realtime is down does not also look healthy.
        health::set_available(health::REALTIME);
        tracing::debug!(channel = CHANNEL, "listening for realtime changes");

        Ok(Self { session })
    }
}

impl EventBus for PgBus {
    fn next(&mut self) -> BusFuture<'_> {
        Box::pin(async move {
            loop {
                let notification = self.session.next().await?;
                match ChangeEvent::parse(notification.payload()) {
                    Ok(event) => return Some(event),
                    // One unreadable payload is not a reason to end the
                    // subscription.
                    Err(err) => {
                        tracing::warn!(error = %err, "ignoring an unreadable realtime payload")
                    }
                }
            }
        })
    }
}
