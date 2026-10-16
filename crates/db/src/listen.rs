//! A dedicated `LISTEN` connection.
//!
//! `LISTEN` is session-scoped, so it cannot ride on a pooled connection the rest
//! of the server keeps recycling: the schema watcher ([`crate::watch`]) and the
//! realtime bus (`lazypock_realtime::PgBus`) each need a connection of their own.
//! That is why a direct endpoint is required rather than a transaction pooler
//! (`PLAN.md` 18.1).
//!
//! Being a dedicated connection does not exempt it from the TLS rules — and both
//! callers used to open one with `tokio_postgres::connect(url, NoTls)`, which
//! cannot connect to a server that requires TLS at all. On Neon, or on any
//! deployment using `sslmode=require`, realtime therefore never connected and the
//! schema watcher retried forever against a TLS handshake failure, leaving the
//! node on its boot-time schema snapshot. The connection is built here instead,
//! with the same [`crate::pool::tls_mode`] decision the pool makes, so there is
//! one place that can get it wrong.

use std::str::FromStr;

use lazypock_core::error::{Error, Result};
use tokio_postgres::config::SslMode;
use tokio_postgres::{AsyncMessage, Client, NoTls, Notification};

use crate::pool::{normalize_url, rustls_connector, tls_mode, TlsMode};

/// Shown in `pg_stat_activity`, so a dedicated connection is recognisable.
const APPLICATION_NAME: &str = "lazypock";

/// Drive a connection on its own task, forwarding its notifications.
///
/// A macro rather than a function, and the comment is the reason: the two TLS
/// modes produce two *different* `Connection` types, and naming them in a generic
/// signature means naming `tokio_postgres`'s `Sink`/`Stream` items, which are not
/// part of its public API. A macro expands per arm with the concrete type, so the
/// loop is written once and the compiler instantiates it twice. Writing it out
/// twice by hand is exactly how the TLS decision drifted apart in the first place.
macro_rules! drive {
    ($connection:expr, $sender:expr) => {{
        let mut connection = $connection;
        let sender = $sender;
        tokio::spawn(async move {
            let _ = std::future::poll_fn(|cx| loop {
                match connection.poll_message(cx) {
                    std::task::Poll::Ready(Some(Ok(AsyncMessage::Notification(notification)))) => {
                        let _ = sender.send(notification);
                    }
                    std::task::Poll::Ready(Some(Ok(_))) => continue,
                    // The connection ending is not reported here: both callers
                    // see `next()` return `None` and decide what to do.
                    std::task::Poll::Ready(Some(Err(_)) | None) => {
                        return std::task::Poll::Ready(())
                    }
                    std::task::Poll::Pending => return std::task::Poll::Pending,
                }
            })
            .await;
        })
    }};
}

/// One connection, driven on its own task, delivering its notifications.
///
/// Dropping this drops the client and with it the session — and so the `LISTEN`.
pub struct ListenSession {
    /// Held, not read: dropping the client closes the session and ends the
    /// subscription, which is a silent thing to debug.
    _client: Client,
    notifications: tokio::sync::mpsc::UnboundedReceiver<Notification>,
    /// Drives the connection; `LISTEN` does not complete until it is polled.
    _driver: tokio::task::JoinHandle<()>,
}

impl ListenSession {
    /// Connect using the TLS mode `url` asks for, and start driving.
    pub async fn connect(url: &str) -> Result<Self> {
        let mut config = tokio_postgres::Config::from_str(&normalize_url(url))
            .map_err(|err| Error::config(format!("invalid DATABASE_URL: {err}")))?;
        config.application_name(APPLICATION_NAME);

        let (sender, notifications) = tokio::sync::mpsc::unbounded_channel();
        let (client, driver) = match tls_mode(url) {
            TlsMode::Required => {
                let (client, connection) = config
                    .connect(rustls_connector()?)
                    .await
                    .map_err(connect_error)?;
                (client, drive!(connection, sender))
            }
            TlsMode::Disabled => {
                // Force plaintext explicitly, regardless of libpq's `prefer`
                // default — the same reading the pool makes.
                config.ssl_mode(SslMode::Disable);
                let (client, connection) = config.connect(NoTls).await.map_err(connect_error)?;
                (client, drive!(connection, sender))
            }
        };

        Ok(Self {
            _client: client,
            notifications,
            _driver: driver,
        })
    }

    /// Issue `LISTEN` on `channel`.
    ///
    /// Channel names come from constants in this workspace, never from input.
    pub async fn listen(&self, channel: &str) -> Result<()> {
        self._client
            .batch_execute(&format!("listen {channel}"))
            .await
            .map_err(|err| Error::internal(format!("cannot listen on {channel}: {err}")))
    }

    /// The next notification, or `None` once the connection has ended.
    pub async fn next(&mut self) -> Option<Notification> {
        self.notifications.recv().await
    }
}

fn connect_error(err: tokio_postgres::Error) -> Error {
    Error::internal(format!(
        "a dedicated connection could not be established: {err}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dedicated_connection_uses_the_tls_the_url_asks_for() {
        // The bug this module exists for: both dedicated connections passed
        // `NoTls` regardless of the URL, so one asking for TLS could never
        // connect. The decision belongs to `tls_mode`, and it must be consulted
        // rather than defaulted.
        assert_eq!(
            tls_mode("postgres://h/db?sslmode=require"),
            TlsMode::Required
        );
        assert_eq!(
            tls_mode("postgres://h/db?ssl=require&channel_binding=require"),
            TlsMode::Required
        );
        assert_eq!(tls_mode("postgres://h/db"), TlsMode::Disabled);
    }
}
