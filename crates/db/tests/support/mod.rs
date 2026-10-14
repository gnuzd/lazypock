//! Shared harness for the `lazypock-db` integration tests.
//!
//! Each test gets its own randomly named schema, so tests are isolated and run
//! in parallel. `TEST_DATABASE_URL` overrides the default connection; when it is
//! not set and Postgres is unreachable the tests skip with a message, but when
//! it *is* set (or `LAZYPOCK_REQUIRE_DB_TESTS=1`) an unreachable database is a
//! hard failure so CI can never silently skip them.
//!
//! Several binaries include this module, so not every helper is used by every
//! one of them.
#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use lazypock_db::deadpool_postgres::Client;
use lazypock_db::pool::{Db, DbOptions};
use lazypock_db::tokio_postgres::{self, NoTls};

/// Default connection, matching `deploy/docker-compose.test.yml`.
pub const DEFAULT_TEST_URL: &str = "postgres://postgres:postgres@localhost:55432/lazypock_test";

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A pooled handle bound to one disposable schema.
pub struct TestDb {
    /// Pooled handle; every connection is scoped to `schema`.
    pub db: Db,
    /// Connection string of the whole test database.
    pub admin_url: String,
    /// The schema this handle is scoped to.
    pub schema: String,
}

impl TestDb {
    /// Check out a pooled connection.
    pub async fn client(&self) -> Client {
        self.db.client().await.expect("pooled client")
    }

    /// Options that scope a new pool to this test's schema.
    pub fn db_options(&self) -> DbOptions {
        DbOptions {
            options: Some(format!("-c search_path={}", self.schema)),
            ..Default::default()
        }
    }

    /// Best-effort cleanup; a leftover schema in a disposable test database is
    /// harmless, so this never fails a test.
    pub async fn cleanup(&self) {
        if let Ok(admin) = admin_connect(&self.admin_url).await {
            let _ = admin
                .batch_execute(&format!(
                    "drop schema if exists \"{}\" cascade",
                    self.schema
                ))
                .await;
        }
    }

    /// Names of the tables and views visible on this test's search path.
    pub async fn tables(&self) -> Vec<String> {
        let client = self.client().await;
        let rows = client
            .query(
                "select table_name from information_schema.tables \
                 where table_schema = current_schema()",
                &[],
            )
            .await
            .expect("list tables");
        rows.iter().map(|row| row.get::<_, String>(0)).collect()
    }

    /// Column names of a table on this test's search path, in ordinal order.
    pub async fn columns(&self, table: &str) -> Vec<String> {
        let client = self.client().await;
        let rows = client
            .query(
                "select column_name from information_schema.columns \
                 where table_schema = current_schema() and table_name = $1 \
                 order by ordinal_position",
                &[&table],
            )
            .await
            .expect("list columns");
        rows.iter().map(|row| row.get::<_, String>(0)).collect()
    }

    /// Data type of a column, as reported by `information_schema`.
    pub async fn column_type(&self, table: &str, column: &str) -> Option<String> {
        let client = self.client().await;
        client
            .query_opt(
                "select data_type from information_schema.columns \
                 where table_schema = current_schema() and table_name = $1 and column_name = $2",
                &[&table, &column],
            )
            .await
            .expect("column type")
            .map(|row| row.get(0))
    }
}

/// Open a single unpooled connection, driving its connection task in the
/// background.
pub async fn admin_connect(url: &str) -> Result<tokio_postgres::Client, tokio_postgres::Error> {
    let (client, connection) = tokio_postgres::connect(url, NoTls).await?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(client)
}

/// Prepare an isolated schema, or `None` when Postgres is not available.
pub async fn test_db(label: &str) -> Option<TestDb> {
    let explicit = std::env::var("LAZYPOCK_TEST_DATABASE_URL").ok();
    let required = std::env::var("LAZYPOCK_REQUIRE_DB_TESTS").is_ok();
    let url = explicit
        .clone()
        .unwrap_or_else(|| DEFAULT_TEST_URL.to_string());

    let admin = match admin_connect(&url).await {
        Ok(admin) => admin,
        Err(err) => {
            if explicit.is_some() || required {
                panic!("Postgres is configured but unreachable: {err}");
            }
            eprintln!(
                "skipping {label}: Postgres unreachable ({err}). \
                 Start deploy/docker-compose.test.yml or set TEST_DATABASE_URL."
            );
            return None;
        }
    };

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let schema = format!(
        "t_{}_{}_{}",
        std::process::id(),
        nanos,
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );

    admin
        .batch_execute(&format!("create schema \"{schema}\""))
        .await
        .expect("create test schema");

    let test = TestDb {
        db: Db::connect(
            &url,
            &DbOptions {
                options: Some(format!("-c search_path={schema}")),
                ..Default::default()
            },
        )
        .await
        .expect("connect the pooled database handle"),
        admin_url: url,
        schema,
    };

    Some(test)
}

/// Spawn a connection that `LISTEN`s on `channel` and forwards notifications.
///
/// `tokio-postgres` surfaces notifications through the connection driver
/// (`Connection::poll_message`) rather than the client, so the connection has to
/// be driven manually here.
pub fn spawn_notification_listener(
    url: &str,
    channel: &str,
) -> tokio::sync::mpsc::UnboundedReceiver<tokio_postgres::Notification> {
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let url = url.to_string();
    let channel = channel.to_string();

    tokio::spawn(async move {
        let (client, mut connection) = tokio_postgres::connect(&url, NoTls)
            .await
            .expect("listener connect");

        // The connection must be driven *while* LISTEN is issued: `batch_execute`
        // waits for a server response, which only arrives if the socket is being
        // polled. Hence the driver task first, LISTEN second.
        let driver = tokio::spawn(async move {
            let _ = std::future::poll_fn(|cx| loop {
                match connection.poll_message(cx) {
                    std::task::Poll::Ready(Some(Ok(
                        tokio_postgres::AsyncMessage::Notification(notification),
                    ))) => {
                        let _ = sender.send(notification);
                    }
                    std::task::Poll::Ready(Some(Ok(_))) => continue,
                    std::task::Poll::Ready(Some(Err(_)) | None) => {
                        return std::task::Poll::Ready(())
                    }
                    std::task::Poll::Pending => return std::task::Poll::Pending,
                }
            })
            .await;
        });

        if let Err(err) = client.batch_execute(&format!("listen {channel}")).await {
            eprintln!("notification listener could not LISTEN: {err}");
            driver.abort();
            return;
        }

        // LISTEN is session-level, so it survives dropping the client; the
        // connection stays alive inside the driver task.
        let _ = driver.await;
    });

    receiver
}

/// A test schema with the system migrations already applied.
pub async fn migrated_test_db(label: &str) -> Option<TestDb> {
    let test = test_db(label).await?;
    let mut client = test.client().await;
    lazypock_db::migrate::run(&mut client)
        .await
        .expect("apply system migrations");
    Some(test)
}
