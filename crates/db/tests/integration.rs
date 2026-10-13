//! Integration tests for the pool and the migration runner against a real
//! Postgres.
//!
//! Bring one up with:
//!
//! ```text
//! docker compose -f deploy/docker-compose.test.yml up -d --wait
//! cargo test -p lazypock-db
//! ```
//!
//! `TEST_DATABASE_URL` overrides the default. When it is *not* set and Postgres
//! is unreachable the tests skip with a message; when it *is* set (or
//! `LAZYPOCK_REQUIRE_DB_TESTS=1`) an unreachable database is a hard failure, so
//! CI can never silently skip them.
//!
//! Every test runs in its own randomly named schema, so tests are isolated and
//! can run in parallel.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lazypock_db::deadpool_postgres::Client;
use lazypock_db::migrate::{self, SYSTEM_MIGRATIONS};
use lazypock_db::pool::{Db, DbOptions};
use lazypock_db::tokio_postgres::{self, NoTls};

const DEFAULT_TEST_URL: &str = "postgres://postgres:postgres@localhost:55432/lazypock_test";

/// System tables that must exist after migration 1.
const EXPECTED_TABLES: &[&str] = &[
    "_audit",
    "_collections",
    "_crons",
    "_fields",
    "_file_outbox",
    "_image_presets",
    "_jobs",
    "_logs",
    "_migrations",
    "_rt_log",
    "_sessions",
    "_settings",
];

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TestDb {
    db: Db,
    admin_url: String,
    schema: String,
}

impl TestDb {
    async fn client(&self) -> Client {
        self.db.client().await.expect("pooled client")
    }

    fn db_options(&self) -> DbOptions {
        DbOptions {
            options: Some(format!("-c search_path={}", self.schema)),
            ..Default::default()
        }
    }

    /// Best-effort cleanup; a leftover schema in a disposable test database is
    /// harmless, so this never fails a test.
    async fn cleanup(&self) {
        if let Ok(admin) = admin_connect(&self.admin_url).await {
            let _ = admin
                .batch_execute(&format!(
                    "drop schema if exists \"{}\" cascade",
                    self.schema
                ))
                .await;
        }
    }

    async fn tables(&self) -> Vec<String> {
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
}

async fn admin_connect(url: &str) -> Result<tokio_postgres::Client, tokio_postgres::Error> {
    let (client, connection) = tokio_postgres::connect(url, NoTls).await?;
    tokio::spawn(async move {
        let _ = connection.await;
    });
    Ok(client)
}

async fn test_db(label: &str) -> Option<TestDb> {
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

#[tokio::test]
async fn applies_every_system_migration_from_scratch() {
    let Some(test) = test_db("applies_every_system_migration_from_scratch").await else {
        return;
    };

    let mut client = test.client().await;
    let report = migrate::run(&mut client).await.expect("migrate");

    assert_eq!(report.applied, vec![1], "migration 1 should apply");
    assert!(report.already_applied.is_empty());

    let tables = test.tables().await;
    for expected in EXPECTED_TABLES {
        assert!(
            tables.iter().any(|table| table == expected),
            "missing system table {expected}; got {tables:?}"
        );
    }

    test.cleanup().await;
}

#[tokio::test]
async fn migrating_twice_is_a_no_op() {
    let Some(test) = test_db("migrating_twice_is_a_no_op").await else {
        return;
    };

    let mut client = test.client().await;
    migrate::run(&mut client).await.expect("first run");

    let second = migrate::run(&mut client).await.expect("second run");
    assert!(second.applied.is_empty(), "nothing should re-apply");
    assert_eq!(second.already_applied, vec![1]);

    let count: i64 = client
        .query_one("select count(*) from _migrations", &[])
        .await
        .expect("count")
        .get(0);
    assert_eq!(count, SYSTEM_MIGRATIONS.len() as i64);

    test.cleanup().await;
}

#[tokio::test]
async fn concurrent_runs_apply_each_migration_exactly_once() {
    let Some(test) = test_db("concurrent_runs_apply_each_migration_exactly_once").await else {
        return;
    };

    // Two connections contend for the advisory lock at the same time.
    let mut first = test.client().await;
    let mut second = test.client().await;

    let (a, b) = tokio::join!(migrate::run(&mut first), migrate::run(&mut second));
    let (a, b) = (a.expect("run a"), b.expect("run b"));

    assert_eq!(
        a.applied.len() + b.applied.len(),
        1,
        "exactly one run must apply migration 1: {a:?} {b:?}"
    );

    let count: i64 = first
        .query_one("select count(*) from _migrations", &[])
        .await
        .expect("count")
        .get(0);
    assert_eq!(count, SYSTEM_MIGRATIONS.len() as i64);

    test.cleanup().await;
}

#[tokio::test]
async fn the_advisory_lock_is_released_after_a_run() {
    let Some(test) = test_db("the_advisory_lock_is_released_after_a_run").await else {
        return;
    };

    let mut client = test.client().await;
    migrate::run(&mut client).await.expect("first run");

    // A run on a *different* connection must not block; a leaked session-level
    // advisory lock would hang here forever.
    let mut other = test.client().await;
    tokio::time::timeout(Duration::from_secs(10), migrate::run(&mut other))
        .await
        .expect("migration run blocked: the advisory lock leaked")
        .expect("second run");

    test.cleanup().await;
}

#[tokio::test]
async fn editing_an_applied_migration_fails_loud() {
    let Some(test) = test_db("editing_an_applied_migration_fails_loud").await else {
        return;
    };

    let mut client = test.client().await;
    migrate::run(&mut client).await.expect("migrate");

    client
        .execute(
            "update _migrations set checksum = 'tampered' where version = 1",
            &[],
        )
        .await
        .expect("tamper");

    let err = migrate::run(&mut client)
        .await
        .expect_err("a tampered migration must be rejected");
    assert!(
        err.message().contains("modified"),
        "unexpected error: {}",
        err.message()
    );

    test.cleanup().await;
}

#[tokio::test]
async fn migrations_unknown_to_this_binary_fail_loud() {
    let Some(test) = test_db("migrations_unknown_to_this_binary_fail_loud").await else {
        return;
    };

    let mut client = test.client().await;
    migrate::run(&mut client).await.expect("migrate");

    client
        .execute(
            "insert into _migrations (version, name, checksum) values (9999, 'from_the_future', 'x')",
            &[],
        )
        .await
        .expect("insert");

    let err = migrate::run(&mut client)
        .await
        .expect_err("an unknown applied migration must be rejected");
    assert!(
        err.message().contains("newer than the server"),
        "unexpected error: {}",
        err.message()
    );

    test.cleanup().await;
}

#[tokio::test]
async fn status_reports_applied_state_and_checksums() {
    let Some(test) = test_db("status_reports_applied_state_and_checksums").await else {
        return;
    };

    let mut client = test.client().await;

    let before = migrate::status(&client).await.expect("status");
    assert_eq!(before.len(), SYSTEM_MIGRATIONS.len());
    assert!(before.iter().all(|status| !status.applied));

    migrate::run(&mut client).await.expect("migrate");

    let after = migrate::status(&client).await.expect("status");
    assert!(after
        .iter()
        .all(|status| status.applied && status.checksum_matches));

    test.cleanup().await;
}

#[tokio::test]
async fn health_check_and_the_prepared_statement_switch_work() {
    let Some(test) = test_db("health_check_and_the_prepared_statement_switch_work").await else {
        return;
    };

    test.db.health().await.expect("health");
    assert!(test.db.prepared_statements());

    // The switch documents the PgBouncer/Neon pooled-endpoint constraint
    // (PLAN.md 18.1); the pool must still connect and serve queries with it off.
    let without_cache = DbOptions {
        prepared_statements: false,
        ..test.db_options()
    };
    let db = Db::connect(&test.admin_url, &without_cache)
        .await
        .expect("connect without prepared statements");
    assert!(!db.prepared_statements());
    db.health()
        .await
        .expect("health without prepared statements");

    let client = db.client().await.expect("client");
    let value: i32 = client
        .query_one("select 41 + 1", &[])
        .await
        .expect("query")
        .get(0);
    assert_eq!(value, 42);

    test.cleanup().await;
}
