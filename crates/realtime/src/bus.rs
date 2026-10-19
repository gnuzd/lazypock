//! Where change events come from.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use lazypock_core::error::{Error, Result};
use lazypock_core::health;
use lazypock_db::listen::ListenSession;

use crate::event::ChangeEvent;
use crate::hub::Hub;

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

/// How long to wait before the first reconnect attempt, and after a session that
/// had been working ends.
const RECONNECT_DELAY: Duration = Duration::from_secs(1);

/// The longest wait between attempts.
///
/// A server that refuses the connection — a suspended compute, a firewall, a
/// wrong URL — would otherwise be retried every second forever: a warning a
/// second that buries the one line saying what is wrong. Backing off keeps the
/// retry and keeps the log readable.
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(30);

/// Why a listen session ended.
///
/// The distinction decides the retry delay: a session that was live and then
/// dropped is worth reconnecting to at once, while one that never came up is not.
#[derive(Debug)]
enum Failure {
    /// The connection was never established.
    NotConnected(Error),
    /// It was established, and then ended.
    Disconnected(Error),
}

/// Run the realtime listener, reconnecting whenever the session ends.
///
/// A `LISTEN` is a *session*: a server restart, a failover, a provider suspending
/// an idle compute, or a network blip all end it, and nothing in the protocol
/// resumes it. The bus used to be connected exactly once at boot, so the first
/// such event retired realtime for the life of the process — the API kept
/// answering and `/api/health` kept saying OK while no subscriber ever received
/// anything again.
///
/// Never returns: the task lives as long as the process.
pub async fn run_supervised(hub: Arc<Hub>, url: String) {
    let mut delay = RECONNECT_DELAY;
    loop {
        let failure = listen_once(hub.clone(), &url).await;
        let (err, was_connected) = match failure {
            Failure::NotConnected(err) => (err, false),
            Failure::Disconnected(err) => (err, true),
        };

        // Reported, so a node whose realtime is down does not also look healthy.
        health::set_unavailable(health::REALTIME, err.message());

        // A session that was live reconnects promptly; one that never came up
        // backs off.
        if was_connected {
            delay = RECONNECT_DELAY;
        }
        tracing::warn!(
            error = %err,
            retry_in_secs = delay.as_secs(),
            "realtime listener disconnected; reconnecting"
        );

        tokio::time::sleep(delay).await;
        delay = next_delay(delay);
    }
}

/// The wait after `delay`, doubled and capped.
fn next_delay(delay: Duration) -> Duration {
    (delay * 2).min(MAX_RECONNECT_DELAY)
}

/// One listen session, returning only when it ends.
async fn listen_once(hub: Arc<Hub>, url: &str) -> Failure {
    match PgBus::connect(url).await {
        Ok(bus) => {
            tracing::info!("realtime listening");
            hub.run(bus).await;
            Failure::Disconnected(Error::internal("the realtime LISTEN session closed"))
        }
        Err(err) => Failure::NotConnected(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnect_delay_doubles_and_stops_at_the_cap() {
        let mut delay = RECONNECT_DELAY;
        let seen: Vec<u64> = (0..8)
            .map(|_| {
                let current = delay.as_secs();
                delay = next_delay(delay);
                current
            })
            .collect();
        assert_eq!(seen, vec![1, 2, 4, 8, 16, 30, 30, 30], "{seen:?}");
    }
}
