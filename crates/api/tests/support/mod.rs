//! Shared harness for the `lazypock-api` integration tests.
//!
//! Each test gets its own randomly named schema, so tests run in parallel and
//! cannot see each other's data. `TEST_DATABASE_URL` overrides the default; when
//! it is not set and Postgres is unreachable the tests skip with a message, but
//! when it *is* set (or `LAZYPOCK_REQUIRE_DB_TESTS=1`) an unreachable database
//! is a hard failure, so CI can never silently skip them.
#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use lazypock_core::config::Config;
use lazypock_db::pool::{Db, DbOptions};
use lazypock_db::schema::SchemaCache;
use lazypock_db::tokio_postgres::{self, NoTls};

/// Default connection, matching `deploy/docker-compose.test.yml`.
pub const DEFAULT_TEST_URL: &str = "postgres://postgres:postgres@localhost:55432/lazypock_test";

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A running application bound to one disposable schema.
pub struct TestApp {
    /// The application state, ready to hand to `router`.
    pub state: lazypock_api::AppState,
    /// The same database handle, for arranging fixtures.
    pub db: Db,
    /// Connection string of the whole test database.
    admin_url: String,
    /// The schema this test is scoped to.
    schema: String,
}

impl TestApp {
    /// Check out a pooled connection.
    pub async fn client(&self) -> lazypock_db::deadpool_postgres::Client {
        self.db.client().await.expect("pooled client")
    }

    /// Re-read the collection schema into the cache.
    ///
    /// The cache is loaded once at boot and, in production, refreshed by the
    /// `lazypock_schema_changed` notification (Track F). Until that listener
    /// exists, a test that creates collections has to ask for the reload.
    pub async fn reload_schema(&self) {
        let client = self.db.client().await.expect("schema client");
        self.state
            .schema
            .reload(&client)
            .await
            .expect("reload schema");
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
}

async fn admin_connect(url: &str) -> Result<tokio_postgres::Client, tokio_postgres::Error> {
    let (client, connection) = tokio_postgres::connect(url, NoTls).await?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(client)
}

/// Prepare an isolated schema with the system migrations applied.
pub async fn test_app(label: &str) -> Option<TestApp> {
    let explicit = std::env::var("TEST_DATABASE_URL").ok();
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

    let cache = Arc::new(SchemaCache::new());
    {
        let client = db.client().await.expect("schema client");
        cache.reload(&client).await.expect("load schema");
    }

    let config = Config::from_source(|key| match key {
        "DATABASE_URL" => Some(url.clone()),
        _ => None,
    })
    .expect("test configuration");

    Some(TestApp {
        state: lazypock_api::AppState::new(config, db.clone(), cache),
        db,
        admin_url: url,
        schema,
    })
}
