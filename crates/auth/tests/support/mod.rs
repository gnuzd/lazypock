//! Harness for the `lazypock-auth` integration tests.
//!
//! Each test gets its own randomly named schema. `TEST_DATABASE_URL` overrides
//! the default; when it is not set and Postgres is unreachable the tests skip
//! with a message, but when it *is* set (or `LAZYPOCK_REQUIRE_DB_TESTS=1`) an
//! unreachable database is a hard failure, so CI can never silently skip them.
//!
//! Only what the auth flows need lives here — no HTTP helpers, no table
//! introspection. The richer harness used by the database and API suites is
//! defined alongside those tests.
#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use lazypock_db::pool::{Db, DbOptions};
use lazypock_db::tokio_postgres::{self, NoTls};

/// Default connection: the development database.
pub const DEFAULT_TEST_URL: &str = "postgres://postgres:postgres@localhost:5432/lazypock_dev";

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A database handle scoped to one disposable schema.
pub struct TestDb {
    /// Pooled handle; every connection is scoped to `schema`.
    pub db: Db,
    admin_url: String,
    schema: String,
}

impl TestDb {
    /// Check out a pooled connection.
    pub async fn client(&self) -> lazypock_db::deadpool_postgres::Client {
        self.db.client().await.expect("pooled client")
    }

    /// Best-effort cleanup; a leftover schema in a disposable test database is
    /// harmless, so this never fails a test.
    pub async fn cleanup(&self) {
        if let Ok((client, connection)) = tokio_postgres::connect(&self.admin_url, NoTls).await {
            tokio::spawn(async move {
                let _ = connection.await;
            });
            let _ = client
                .batch_execute(&format!(
                    "drop schema if exists \"{}\" cascade",
                    self.schema
                ))
                .await;
        }
    }
}

/// Prepare an isolated schema with the system migrations applied.
pub async fn migrated_test_db(label: &str) -> Option<TestDb> {
    // `LAZYPOCK_TEST_DATABASE_URL` first, then the app's own variable, so exporting
    // a single `LAZYPOCK_DATABASE_URL` is enough to point the tests somewhere.
    let explicit = std::env::var("LAZYPOCK_TEST_DATABASE_URL")
        .or_else(|_| std::env::var("LAZYPOCK_DATABASE_URL"))
        .ok();
    let required = std::env::var("LAZYPOCK_REQUIRE_DB_TESTS").is_ok();
    let url = explicit
        .clone()
        .unwrap_or_else(|| DEFAULT_TEST_URL.to_string());

    let admin = match tokio_postgres::connect(&url, NoTls).await {
        Ok((client, connection)) => {
            tokio::spawn(async move {
                let _ = connection.await;
            });
            client
        }
        Err(err) => {
            if explicit.is_some() || required {
                panic!("Postgres is configured but unreachable: {err}");
            }
            eprintln!("skipping {label}: Postgres unreachable ({err})");
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

    let db = Db::connect(
        &url,
        &DbOptions {
            options: Some(format!("-c search_path={schema}")),
            ..Default::default()
        },
    )
    .await
    .expect("connect the pooled database handle");

    {
        let mut client = db.client().await.expect("migration client");
        lazypock_db::migrate::run(&mut client)
            .await
            .expect("apply system migrations");
    }

    Some(TestDb {
        db,
        admin_url: url,
        schema,
    })
}
