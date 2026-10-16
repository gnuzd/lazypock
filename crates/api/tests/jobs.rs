//! The background job queue over `_jobs`.

mod support;

use std::time::Duration;

use serde_json::json;

use lazypock_api::jobs::{self, NewJob, KIND_SQL};
use support::{test_app, TestApp};

/// The worker name a pass under test claims with.
const WORKER: &str = "test-worker";

/// One job's columns, as these tests need to inspect them.
struct Row {
    status: String,
    attempts: i32,
    last_error: Option<String>,
    /// Whether the job would be claimed by the next pass.
    due: bool,
}

async fn row(app: &TestApp, id: &str) -> Row {
    let stored = app
        .client()
        .await
        .query_one(
            "select status, attempts, last_error, (run_at <= now()) as due \
               from _jobs where id = $1",
            &[&id],
        )
        .await
        .expect("the job is still there");
    Row {
        status: stored.get("status"),
        attempts: stored.get("attempts"),
        last_error: stored.get("last_error"),
        due: stored.get("due"),
    }
}

/// Whether a table exists in this test's schema.
async fn table_exists(app: &TestApp, name: &str) -> bool {
    app.client()
        .await
        .query_one(
            "select to_regclass($1) is not null as there",
            &[&name.to_string()],
        )
        .await
        .expect("a row")
        .get("there")
}

#[tokio::test]
async fn a_queued_job_runs_once_and_is_marked_done() {
    let Some(app) = test_app("jobs_done").await else {
        return;
    };

    let id = jobs::enqueue(
        &app.client().await,
        NewJob::new(
            KIND_SQL,
            json!({ "statement": "create table if not exists job_probe (id int)" }),
        ),
    )
    .await
    .expect("enqueued");

    let ran = jobs::run_due(&app.state, WORKER, 10, 300.0)
        .await
        .expect("a pass");
    assert_eq!(ran, 1);

    let stored = row(&app, &id).await;
    assert_eq!(stored.status, "done");
    assert_eq!(stored.attempts, 1);
    assert_eq!(stored.last_error, None);
    assert!(
        table_exists(&app, "job_probe").await,
        "the job's statement did not run"
    );

    // A done job is not picked up again.
    let again = jobs::run_due(&app.state, WORKER, 10, 300.0)
        .await
        .expect("a second pass");
    assert_eq!(again, 0);

    app.cleanup().await;
}

#[tokio::test]
async fn a_job_that_is_not_due_is_left_alone() {
    let Some(app) = test_app("jobs_not_due").await else {
        return;
    };

    let id = jobs::enqueue(
        &app.client().await,
        NewJob::new(KIND_SQL, json!({ "statement": "select 1" })).after(Duration::from_secs(3600)),
    )
    .await
    .expect("enqueued");

    let ran = jobs::run_due(&app.state, WORKER, 10, 300.0)
        .await
        .expect("a pass");
    assert_eq!(ran, 0);
    assert_eq!(row(&app, &id).await.status, "pending");

    app.cleanup().await;
}

#[tokio::test]
async fn a_failing_job_is_delayed_then_retired_once_its_attempts_run_out() {
    let Some(app) = test_app("jobs_retry").await else {
        return;
    };

    let id = jobs::enqueue(
        &app.client().await,
        NewJob::new(
            KIND_SQL,
            json!({ "statement": "select * from definitely_not_a_table" }),
        )
        .with_max_attempts(2),
    )
    .await
    .expect("enqueued");

    // First attempt: the failure is recorded and the retry is scheduled, not run
    // straight away — a dependency that is down must not be hammered.
    assert_eq!(
        jobs::run_due(&app.state, WORKER, 10, 300.0)
            .await
            .expect("a pass"),
        1
    );
    let stored = row(&app, &id).await;
    assert_eq!(stored.status, "pending");
    assert_eq!(stored.attempts, 1);
    assert!(!stored.due, "the retry was not delayed");
    assert!(
        stored
            .last_error
            .as_deref()
            .is_some_and(|message| message.contains("definitely_not_a_table")),
        "the recorded reason should name what went wrong, got {:?}",
        stored.last_error
    );

    // Make it due and run again: the second attempt exhausts the budget.
    app.client()
        .await
        .execute("update _jobs set run_at = now() where id = $1", &[&id])
        .await
        .expect("the retry is made due");
    assert_eq!(
        jobs::run_due(&app.state, WORKER, 10, 300.0)
            .await
            .expect("a second pass"),
        1
    );

    let stored = row(&app, &id).await;
    assert_eq!(stored.status, "dead");
    assert_eq!(stored.attempts, 2);
    assert!(stored.last_error.is_some());

    app.cleanup().await;
}

/// A kind nothing handles must not look like a job that worked.
#[tokio::test]
async fn an_unknown_kind_is_retired_rather_than_succeeding_quietly() {
    let Some(app) = test_app("jobs_unknown").await else {
        return;
    };

    let id = jobs::enqueue(
        &app.client().await,
        NewJob::new("no-such-kind", json!({})).with_max_attempts(1),
    )
    .await
    .expect("enqueued");

    assert_eq!(
        jobs::run_due(&app.state, WORKER, 10, 300.0)
            .await
            .expect("a pass"),
        1
    );

    let stored = row(&app, &id).await;
    assert_eq!(stored.status, "dead");
    assert!(
        stored
            .last_error
            .as_deref()
            .is_some_and(|message| message.contains("no-such-kind")),
        "the reason should name the kind, got {:?}",
        stored.last_error
    );

    app.cleanup().await;
}

/// A worker that claimed a job and then died must not strand it.
#[tokio::test]
async fn a_job_whose_worker_stopped_is_handed_back() {
    let Some(app) = test_app("jobs_reclaim").await else {
        return;
    };

    let id = jobs::enqueue(
        &app.client().await,
        NewJob::new(KIND_SQL, json!({ "statement": "select 1" })),
    )
    .await
    .expect("enqueued");

    app.client()
        .await
        .execute(
            "update _jobs set status = 'running', locked_by = 'gone', \
                    locked_at = now() - interval '1 hour' where id = $1",
            &[&id],
        )
        .await
        .expect("the abandoned claim is staged");

    // The pass hands the job back and then runs it, so it ends up done.
    let ran = jobs::run_due(&app.state, WORKER, 10, 300.0)
        .await
        .expect("a pass");
    assert_eq!(ran, 1);

    let stored = row(&app, &id).await;
    assert_eq!(stored.status, "done");
    // The abandoned attempt is counted, and then this one.
    assert_eq!(stored.attempts, 2);

    app.cleanup().await;
}

/// The other half of the lease: a job another worker is still running stays put.
#[tokio::test]
async fn a_job_with_a_live_lease_is_left_to_its_worker() {
    let Some(app) = test_app("jobs_lease").await else {
        return;
    };

    let id = jobs::enqueue(
        &app.client().await,
        NewJob::new(KIND_SQL, json!({ "statement": "select 1" })),
    )
    .await
    .expect("enqueued");

    app.client()
        .await
        .execute(
            "update _jobs set status = 'running', locked_by = 'other', locked_at = now() \
              where id = $1",
            &[&id],
        )
        .await
        .expect("the live claim is staged");

    let ran = jobs::run_due(&app.state, WORKER, 10, 300.0)
        .await
        .expect("a pass");
    assert_eq!(ran, 0, "a job under a live lease was taken away");
    assert_eq!(row(&app, &id).await.status, "running");

    app.cleanup().await;
}
