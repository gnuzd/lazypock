//! The in-RAM schema cache must stay fresh while a node is running.

mod support;

use std::sync::Arc;
use std::time::Duration;

use lazypock_db::collections::{self, CreateCollection};
use lazypock_db::schema::SchemaCache;
use lazypock_db::types::CollectionType;
use lazypock_db::watch::watch_schema;
use serde_json::json;

use support::migrated_test_db;

/// How long the watcher waits before reconnecting after a dropped session.
const RECONNECT_DELAY: Duration = Duration::from_secs(1);

fn empty_collection(name: &str) -> CreateCollection {
    CreateCollection {
        name: name.into(),
        collection_type: CollectionType::Base,
        fields: Vec::new(),
        rules: json!({}),
        options: json!({}),
        adopt_existing_table: false,
    }
}

/// Wait until `name` appears in the cache, failing the test on timeout.
async fn wait_for(cache: &SchemaCache, name: &str, within: Duration) {
    tokio::time::timeout(within, async {
        loop {
            if cache.snapshot().get(name).is_some() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the schema cache never picked up {name:?}"));
}

/// Wait until `name` disappears from the cache.
async fn wait_gone(cache: &SchemaCache, name: &str, within: Duration) {
    tokio::time::timeout(within, async {
        loop {
            if cache.snapshot().get(name).is_none() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("the schema cache never dropped {name:?}"));
}

#[tokio::test]
async fn the_cache_refreshes_while_a_node_runs() {
    let Some(test) = migrated_test_db("watch_refresh").await else {
        return;
    };

    let cache = Arc::new(SchemaCache::new());
    {
        let client = test.client().await;
        cache.reload(&client).await.expect("initial schema load");
    }
    assert!(cache.snapshot().get("posts").is_none());

    let watcher = watch_schema(cache.clone(), test.db.clone(), test.admin_url.clone(), None);

    // The create happens on a connection other than the watcher's own, and emits
    // NOTIFY lazypock_schema_changed at commit.
    let mut client = test.client().await;
    collections::create(&mut client, empty_collection("posts"))
        .await
        .expect("create posts");
    wait_for(&cache, "posts", Duration::from_secs(10)).await;

    collections::create(&mut client, empty_collection("comments"))
        .await
        .expect("create comments");
    wait_for(&cache, "comments", Duration::from_secs(10)).await;

    // A deletion is reflected too.
    collections::delete(&mut client, "posts", true)
        .await
        .expect("delete posts");
    wait_gone(&cache, "posts", Duration::from_secs(10)).await;

    watcher.stop();
    test.cleanup().await;
}

#[tokio::test]
async fn changes_arrive_without_waiting_for_a_reconnect() {
    let Some(test) = migrated_test_db("watch_live").await else {
        return;
    };

    let cache = Arc::new(SchemaCache::new());
    let watcher = watch_schema(cache.clone(), test.db.clone(), test.admin_url.clone(), None);
    let mut client = test.client().await;

    // The first change may legitimately be picked up by the reload the watcher
    // performs right after `LISTEN`, so it is not evidence either way.
    collections::create(&mut client, empty_collection("first"))
        .await
        .expect("create first");
    wait_for(&cache, "first", Duration::from_secs(10)).await;

    // The session is established by now, so the next change must arrive well
    // inside the reconnect delay. A watcher that closes its own `LISTEN` session
    // (by dropping the client handle) can only notice after reconnecting, which
    // takes at least RECONNECT_DELAY.
    let window = RECONNECT_DELAY / 2;
    collections::create(&mut client, empty_collection("second"))
        .await
        .expect("create second");
    wait_for(&cache, "second", window).await;

    watcher.stop();
    test.cleanup().await;
}
