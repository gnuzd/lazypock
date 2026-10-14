//! Fan-out behaviour: who receives what, and who does not.

mod support;

use std::time::Duration;

use lazypock_db::rls::context::RequestContext;
use lazypock_realtime::{ChangeEvent, ChangeOp, Delivered, Hub, Subscription};
use serde_json::json;

use support::fixture;

/// Receive without blocking forever, so "did not receive" is testable.
async fn try_recv(subscription: &mut Subscription) -> Option<Delivered> {
    tokio::time::timeout(Duration::from_millis(150), subscription.recv())
        .await
        .ok()
        .flatten()
}

fn event(op: ChangeOp, id: &str, origin: Option<&str>) -> ChangeEvent {
    ChangeEvent {
        log_id: 0,
        collection: "posts".into(),
        op,
        record_id: id.into(),
        origin: origin.map(str::to_string),
    }
}

/// Insert a row, so a change event has something whose visibility can be checked.
///
/// The fan-out refuses to announce a record a subscriber cannot read, and a
/// record that does not exist is one of those.
async fn seed(test: &support::Fixture, id: &str, author: &str) {
    let client = test.client().await;
    client
        .batch_execute(&format!(
            r#"insert into "posts" ("id", "author") values ('{id}', '{author}')"#
        ))
        .await
        .expect("seed row");
}

fn user(id: &str) -> RequestContext {
    RequestContext::authenticated(id, format!("{id}@example.com"), "c1", "users")
}

#[tokio::test]
async fn a_change_reaches_every_connection_that_may_see_it() {
    let Some(test) = fixture("rt_fan_out", json!({ "listRule": "" })).await else {
        return;
    };

    seed(&test, "p1", "u1").await;
    let mut first = test
        .hub
        .subscribe("posts", None, test.owner.clone(), "conn1");
    let mut second = test
        .hub
        .subscribe("posts", None, test.owner.clone(), "conn2");

    test.hub.dispatch(event(ChangeOp::Create, "p1", None)).await;

    for (name, subscription) in [("first", &mut first), ("second", &mut second)] {
        let delivered = try_recv(subscription).await.expect("delivered");
        assert_eq!(delivered.event.record_id, "p1");
        assert_eq!(delivered.event.op, ChangeOp::Create);
        assert!(delivered.record.is_some(), "{name} should get the record");
    }

    test.cleanup().await;
}

#[tokio::test]
async fn the_originating_connection_is_not_echoed_to() {
    let Some(test) = fixture("rt_echo", json!({ "listRule": "" })).await else {
        return;
    };

    seed(&test, "p1", "u1").await;
    let mut origin = test
        .hub
        .subscribe("posts", None, test.owner.clone(), "conn1");
    let mut other = test
        .hub
        .subscribe("posts", None, test.owner.clone(), "conn2");

    test.hub
        .dispatch(event(ChangeOp::Create, "p1", Some("conn1")))
        .await;

    assert!(
        try_recv(&mut origin).await.is_none(),
        "the device that made the change must not be told about it"
    );
    assert!(
        try_recv(&mut other).await.is_some(),
        "a different connection of the same user must still be told"
    );

    test.cleanup().await;
}

#[tokio::test]
async fn a_change_without_an_origin_reaches_everybody() {
    let Some(test) = fixture("rt_no_origin", json!({ "listRule": "" })).await else {
        return;
    };

    seed(&test, "p1", "u1").await;
    // A hook, a job or direct SQL has no connection, so nobody is suppressed.
    let mut first = test
        .hub
        .subscribe("posts", None, test.owner.clone(), "conn1");
    test.hub.dispatch(event(ChangeOp::Create, "p1", None)).await;

    assert!(try_recv(&mut first).await.is_some());
    test.cleanup().await;
}

#[tokio::test]
async fn row_level_security_filters_what_a_subscriber_receives() {
    let Some(test) = fixture("rt_rls", json!({ "listRule": "author = @request.auth.id" })).await
    else {
        return;
    };

    let allowed = test.hub.subscribe("posts", None, user("u1"), "conn1");
    let denied = test.hub.subscribe("posts", None, user("u2"), "conn2");
    let anonymous = test
        .hub
        .subscribe("posts", None, RequestContext::anonymous(), "conn3");

    // The row belongs to u1, so only u1 may be told about it.
    let mut allowed = allowed;
    let mut denied = denied;
    let mut anonymous = anonymous;
    {
        let client = test.client().await;
        client
            .batch_execute(r#"insert into "posts" ("id", "author") values ('p1', 'u1')"#)
            .await
            .expect("seed");
    }

    test.hub.dispatch(event(ChangeOp::Update, "p1", None)).await;

    assert!(
        try_recv(&mut allowed).await.is_some(),
        "the owner of the row may see it"
    );
    assert!(
        try_recv(&mut denied).await.is_none(),
        "another account must not be told about a row it cannot read"
    );
    assert!(
        try_recv(&mut anonymous).await.is_none(),
        "an anonymous subscriber must not be told about a private row"
    );

    test.cleanup().await;
}

#[tokio::test]
async fn a_delete_reaches_only_subscribers_that_saw_the_record() {
    let Some(test) = fixture(
        "rt_delete",
        json!({ "listRule": "author = @request.auth.id" }),
    )
    .await
    else {
        return;
    };

    let mut owner = test.hub.subscribe("posts", None, user("u1"), "conn1");
    let mut stranger = test.hub.subscribe("posts", None, user("u2"), "conn2");

    let client = test.client().await;
    client
        .batch_execute(r#"insert into "posts" ("id", "author") values ('p1', 'u1')"#)
        .await
        .expect("seed");

    // An update first, so the owner is known to have been shown the record.
    test.hub.dispatch(event(ChangeOp::Update, "p1", None)).await;
    assert!(try_recv(&mut owner).await.is_some());
    assert!(try_recv(&mut stranger).await.is_none());

    test.hub.dispatch(event(ChangeOp::Delete, "p1", None)).await;
    assert!(
        try_recv(&mut owner).await.is_some(),
        "somebody already shown the record should hear that it went"
    );
    assert!(
        try_recv(&mut stranger).await.is_none(),
        "somebody never shown the record must not learn that it existed"
    );

    test.cleanup().await;
}

#[tokio::test]
async fn a_record_subscription_ignores_other_records() {
    let Some(test) = fixture("rt_record_scope", json!({ "listRule": "" })).await else {
        return;
    };
    seed(&test, "p1", "u1").await;
    seed(&test, "p2", "u1").await;
    let mut one = test
        .hub
        .subscribe("posts", Some("p1".into()), test.owner.clone(), "conn1");

    test.hub.dispatch(event(ChangeOp::Create, "p2", None)).await;
    assert!(
        try_recv(&mut one).await.is_none(),
        "p2 is not subscribed to"
    );

    test.hub.dispatch(event(ChangeOp::Create, "p1", None)).await;
    assert!(try_recv(&mut one).await.is_some());

    test.cleanup().await;
}

#[tokio::test]
async fn unsubscribing_stops_delivery() {
    let Some(test) = fixture("rt_unsubscribe", json!({ "listRule": "" })).await else {
        return;
    };
    let mut subscription = test
        .hub
        .subscribe("posts", None, test.owner.clone(), "conn1");
    let id = subscription.id();
    assert_eq!(test.hub.subscription_count(), 1);

    test.hub.unsubscribe("posts", id);
    assert_eq!(test.hub.subscription_count(), 0);

    test.hub.dispatch(event(ChangeOp::Create, "p1", None)).await;
    assert!(try_recv(&mut subscription).await.is_none());

    test.cleanup().await;
}

#[tokio::test]
async fn a_subscription_survives_a_miss_then_delivers_again() {
    let Some(test) = fixture("rt_slow_client", json!({ "listRule": "" })).await else {
        return;
    };

    seed(&test, "p1", "u1").await;
    let mut subscription = test
        .hub
        .subscribe("posts", None, test.owner.clone(), "conn1");

    // Enough events to overflow the client's queue: the hub drops rather than
    // blocking the node.
    for _ in 0..lazypock_realtime::hub::CLIENT_QUEUE + 10 {
        test.hub.dispatch(event(ChangeOp::Update, "p1", None)).await;
    }

    assert!(
        test.hub.dropped_events() > 0,
        "a client that cannot keep up should lose events"
    );
    // The stream is still usable afterwards.
    assert!(try_recv(&mut subscription).await.is_some());

    test.cleanup().await;
}

#[tokio::test]
async fn replay_returns_events_the_subscriber_may_see() {
    let Some(test) = fixture("rt_replay", json!({ "listRule": "" })).await else {
        return;
    };
    let client = test.client().await;
    client
        .batch_execute(r#"insert into "posts" ("id", "author") values ('p1', 'u1')"#)
        .await
        .expect("seed");
    // The trigger is what writes the log, so a real change is needed.
    client
        .batch_execute(r#"update "posts" set "author" = 'u1' where "id" = 'p1'"#)
        .await
        .expect("update");

    // The trigger logs the insert as well, so both are replayed in order.
    let replayed = test.hub.replay("posts", 0, &test.owner).await;
    assert_eq!(replayed.len(), 2, "insert and update: {replayed:?}");
    assert_eq!(replayed[0].event.op, ChangeOp::Create);
    assert_eq!(replayed[1].event.op, ChangeOp::Update);
    assert_eq!(replayed[1].event.record_id, "p1");

    // `since` past the last id yields nothing.
    let last = replayed[1].event.log_id;
    assert!(test.hub.replay("posts", last, &test.owner).await.is_empty());

    let _: &Hub = &test.hub;
    test.cleanup().await;
}
