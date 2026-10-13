//! Integration tests for the connection pool and the migration runner.
//!
//! Requires a Postgres; see `tests/support/mod.rs`.

mod support;

use std::time::Duration;

use lazypock_db::migrate::{self, SYSTEM_MIGRATIONS};
use lazypock_db::pool::{Db, DbOptions};

use support::test_db;

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
