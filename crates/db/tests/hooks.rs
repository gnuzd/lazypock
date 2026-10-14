//! The seam that lets the record write path run hooks.
//!
//! Requires a Postgres; see `tests/support/mod.rs`.

mod support;

use std::sync::{Arc, Mutex};

use lazypock_core::error::Result;
use lazypock_db::collections::{self, Collection, CreateCollection, CreateField};
use lazypock_db::hooks::{HookEvent, NoHooks, RecordHooks};
use lazypock_db::query::compile::FilterContext;
use lazypock_db::query::projection::{ExpandSpec, FieldSelection};
use lazypock_db::records::{self, RecordWriteError};
use lazypock_db::runner::BoxFuture;
use lazypock_db::tokio_postgres::Client;
use lazypock_db::types::{CollectionType, FieldType};
use serde_json::{json, Value};

use support::migrated_test_db;

/// A hook implementation that records how it was called and rewrites the record.
struct Spy {
    calls: Arc<Mutex<Vec<(HookEvent, String)>>>,
    transform: Box<dyn Fn(Value) -> Value + Send + Sync>,
}

impl Spy {
    fn new(transform: impl Fn(Value) -> Value + Send + Sync + 'static) -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            transform: Box::new(transform),
        }
    }

    fn calls(&self) -> Vec<(HookEvent, String)> {
        self.calls.lock().expect("calls").clone()
    }
}

impl RecordHooks for Spy {
    fn before<'a>(
        &'a self,
        event: HookEvent,
        collection: &'a str,
        record: Value,
    ) -> BoxFuture<'a, Result<Value>> {
        Box::pin(async move {
            self.calls
                .lock()
                .expect("calls")
                .push((event, collection.to_string()));
            Ok((self.transform)(record))
        })
    }

    fn after<'a>(
        &'a self,
        _event: HookEvent,
        _collection: &'a str,
        _record: Value,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { Ok(()) })
    }
}

/// A `posts` collection with a required `slug`, which only a hook supplies.
async fn fixture(client: &mut Client) -> (Collection, FilterContext) {
    collections::create(
        client,
        CreateCollection {
            name: "posts".into(),
            collection_type: CollectionType::Base,
            fields: vec![
                CreateField {
                    name: "title".into(),
                    field_type: FieldType::Text,
                    required: false,
                    unique: false,
                    options: json!({}),
                },
                CreateField {
                    name: "slug".into(),
                    field_type: FieldType::Text,
                    required: true,
                    unique: false,
                    options: json!({}),
                },
            ],
            rules: json!({}),
            options: json!({}),
            adopt_existing_table: false,
        },
    )
    .await
    .expect("create posts");

    let all = collections::list(client).await.expect("all");
    let collection = all
        .iter()
        .find(|collection| collection.name == "posts")
        .cloned()
        .expect("posts");
    let context = FilterContext::for_collection(&collection, &all).expect("context");
    (collection, context)
}

#[tokio::test]
async fn a_before_hook_decides_what_is_written() {
    let Some(db) = migrated_test_db("hooks_before_decides").await else {
        return;
    };
    let mut client = db.client().await;
    let (collection, context) = fixture(&mut client).await;

    let spy = Spy::new(|mut record| {
        record["slug"] = json!("hello-world");
        record
    });

    // The caller sends no slug at all.
    let stored = records::create_with(
        &client,
        &collection,
        &context,
        json!({ "title": "Hello World" }),
        &[],
        &spy,
    )
    .await
    .expect("create");

    assert_eq!(
        stored["slug"],
        json!("hello-world"),
        "the hook's mutation should be what was written"
    );
    assert_eq!(spy.calls(), vec![(HookEvent::Create, "posts".to_string())]);

    // ... and it is what the database holds, not just what was returned.
    let read_back = records::get_one(
        &client,
        &collection,
        &context,
        stored["id"].as_str().expect("id"),
        &FieldSelection::all(),
        &ExpandSpec::none(),
    )
    .await
    .expect("read")
    .expect("a record");
    assert_eq!(read_back["slug"], json!("hello-world"));

    db.cleanup().await;
}

#[tokio::test]
async fn without_a_hook_a_required_field_is_still_required() {
    // Establishes that the field really is required, so the test above proves the
    // hook supplied it rather than the collection being lenient.
    let Some(db) = migrated_test_db("hooks_no_hook_required").await else {
        return;
    };
    let mut client = db.client().await;
    let (collection, context) = fixture(&mut client).await;

    let error = records::create_with(
        &client,
        &collection,
        &context,
        json!({ "title": "Hello World" }),
        &[],
        &NoHooks,
    )
    .await
    .expect_err("a required field should still be required");

    assert!(
        matches!(error, RecordWriteError::Validation(_)),
        "expected a validation failure, got {error:?}"
    );

    db.cleanup().await;
}

#[tokio::test]
async fn a_hook_cannot_write_something_the_collection_rejects() {
    // The hook's output is validated like any other input, so a hook is not a way
    // around the collection's rules.
    let Some(db) = migrated_test_db("hooks_validated").await else {
        return;
    };
    let mut client = db.client().await;
    let (collection, context) = fixture(&mut client).await;

    // A hook that removes the required field again.
    let spy = Spy::new(|mut record| {
        record.as_object_mut().expect("object").remove("slug");
        record
    });

    let error = records::create_with(
        &client,
        &collection,
        &context,
        json!({ "title": "Hello World", "slug": "given" }),
        &[],
        &spy,
    )
    .await
    .expect_err("the hook's output should be validated");

    assert!(
        matches!(error, RecordWriteError::Validation(_)),
        "expected a validation failure, got {error:?}"
    );

    db.cleanup().await;
}

#[tokio::test]
async fn the_hook_sees_the_collection_it_is_writing_to() {
    let Some(db) = migrated_test_db("hooks_sees_collection").await else {
        return;
    };
    let mut client = db.client().await;
    let (collection, context) = fixture(&mut client).await;

    let spy = Spy::new(|record| record);
    records::create_with(
        &client,
        &collection,
        &context,
        json!({ "title": "T", "slug": "s" }),
        &[],
        &spy,
    )
    .await
    .expect("create");

    assert_eq!(spy.calls(), vec![(HookEvent::Create, "posts".to_string())]);

    db.cleanup().await;
}
