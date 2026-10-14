//! Harness for the realtime integration tests.
//!
//! Each test gets its own randomly named schema, so tests run in parallel and
//! cannot see each other's data.
#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use lazypock_db::collections::{self, CreateCollection, CreateField};
use lazypock_db::pool::{Db, DbOptions};
use lazypock_db::rls::context::RequestContext;
use lazypock_db::schema::SchemaCache;
use lazypock_db::tokio_postgres::{self, NoTls};
use lazypock_db::types::{CollectionType, FieldType};
use lazypock_realtime::Hub;
use serde_json::Value;

pub const DEFAULT_TEST_URL: &str = "postgres://postgres:postgres@localhost:55432/lazypock_test";

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A hub with one collection, ready to dispatch against.
pub struct Fixture {
    pub db: Db,
    pub hub: Arc<Hub>,
    pub owner: RequestContext,
    admin_url: String,
    schema_name: String,
}

impl Fixture {
    pub async fn client(&self) -> lazypock_db::deadpool_postgres::Client {
        self.db.client().await.expect("pooled client")
    }

    pub async fn cleanup(&self) {
        if let Ok((client, connection)) = tokio_postgres::connect(&self.admin_url, NoTls).await {
            tokio::spawn(async move {
                let _ = connection.await;
            });
            let _ = client
                .batch_execute(&format!(
                    "drop schema if exists \"{}\" cascade",
                    self.schema_name
                ))
                .await;
        }
    }
}

/// A table with a `name` column, the given rules, and an empty row.
pub async fn fixture(label: &str, rules: Value) -> Option<Fixture> {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("warn")
        .with_test_writer()
        .try_init();
    let explicit = std::env::var("LAZYPOCK_TEST_DATABASE_URL").ok();
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
    let schema_name = format!(
        "t_{}_{}_{}",
        std::process::id(),
        nanos,
        COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    admin
        .batch_execute(&format!("create schema \"{schema_name}\""))
        .await
        .expect("create schema");

    let db = Db::connect(
        &url,
        &DbOptions {
            options: Some(format!("-c search_path={schema_name}")),
            ..Default::default()
        },
    )
    .await
    .expect("connect");

    {
        let mut client = db.client().await.expect("migration client");
        lazypock_db::migrate::run(&mut client)
            .await
            .expect("migrate");
    }

    {
        let mut client = db.client().await.expect("client");
        collections::create(
            &mut client,
            CreateCollection {
                name: "posts".into(),
                collection_type: CollectionType::Base,
                fields: vec![CreateField {
                    name: "author".into(),
                    field_type: FieldType::Text,
                    required: false,
                    unique: false,
                    options: serde_json::json!({}),
                }],
                rules,
                options: serde_json::json!({}),
                adopt_existing_table: false,
            },
        )
        .await
        .expect("create posts");
    }

    let schema = Arc::new(SchemaCache::new());
    {
        let client = db.client().await.expect("schema client");
        schema.reload(&client).await.expect("load schema");
    }

    Some(Fixture {
        hub: Hub::new(db.clone(), schema),
        db,
        owner: RequestContext::owner(),
        admin_url: url,
        schema_name,
    })
}
