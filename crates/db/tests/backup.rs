//! Export and import against a real database.
//!
//! Requires a Postgres; see `tests/support/mod.rs`.

mod support;

use lazypock_db::backup::{
    self, AtomicMode, ImportAction, ImportOptions, ImportOutcome, ARCHIVE_FORMAT,
};
use lazypock_db::collections::{self, CreateCollection, CreateField};
use lazypock_db::types::{CollectionType, FieldType};
use serde_json::json;

use support::migrated_test_db;

fn collection(name: &str, fields: Vec<CreateField>) -> CreateCollection {
    CreateCollection {
        name: name.into(),
        collection_type: CollectionType::Base,
        fields,
        rules: json!({}),
        options: json!({}),
        adopt_existing_table: false,
    }
}

fn field(name: &str, field_type: FieldType) -> CreateField {
    CreateField {
        name: name.into(),
        field_type,
        required: false,
        unique: false,
        options: json!({}),
    }
}

/// Two collections with rows, ready to export.
async fn fixture(client: &mut lazypock_db::tokio_postgres::Client) {
    for name in ["posts", "orders"] {
        collections::create(
            client,
            collection(name, vec![field("title", FieldType::Text)]),
        )
        .await
        .unwrap_or_else(|err| panic!("create {name}: {err}"));
    }

    client
        .batch_execute(
            "insert into posts (id, title) values ('p1', 'first'), ('p2', 'second');
             insert into orders (id, title) values ('o1', 'only order');",
        )
        .await
        .expect("seed");
}

/// Every row of a table, as JSON, ordered so comparisons are stable.
async fn rows(client: &lazypock_db::tokio_postgres::Client, table: &str) -> serde_json::Value {
    let rows = client
        .query(
            &format!("select to_jsonb(t) as row from {table} t order by id"),
            &[],
        )
        .await
        .unwrap_or_else(|err| panic!("read {table}: {err}"));
    serde_json::Value::Array(rows.iter().map(|row| row.get("row")).collect())
}

#[tokio::test]
async fn an_export_imports_back_into_an_emptied_database() {
    let Some(db) = migrated_test_db("backup_round_trip").await else {
        return;
    };
    let mut client = db.client().await;
    fixture(&mut client).await;

    let before_posts = rows(&client, "posts").await;
    let before_orders = rows(&client, "orders").await;

    // Export, then carry it over the wire format.
    let archive = backup::export(&client).await.expect("export");
    assert_eq!(archive.schema.collections.len(), 2, "both collections");
    assert_eq!(archive.record_count(), 3);
    let bytes = backup::write(&archive).expect("write");
    let read_back = backup::read(&bytes).expect("read");

    // Wipe the data to prove the import puts it back rather than finding it there.
    client
        .batch_execute("delete from posts; delete from orders;")
        .await
        .expect("wipe");
    assert_eq!(rows(&client, "posts").await, json!([]));

    let options = ImportOptions::default();
    match backup::import(&mut client, &read_back, &options)
        .await
        .expect("import")
    {
        ImportOutcome::Applied(report) => {
            assert_eq!(report.errors, Vec::new(), "{report:?}");
            assert_eq!(report.imported.len(), 2, "{report:?}");
            let restored: u64 = report.imported.iter().map(|c| c.records_imported).sum();
            assert_eq!(restored, 3);
        }
        ImportOutcome::NeedsConfirmation(preflight) => {
            panic!("three records should not need confirmation: {preflight:?}")
        }
    }

    // The rows are back, identical.
    assert_eq!(rows(&client, "posts").await, before_posts);
    assert_eq!(rows(&client, "orders").await, before_orders);

    db.cleanup().await;
}

#[tokio::test]
async fn importing_a_partial_archive_leaves_the_other_tables_untouched() {
    // The rule, at the database level: an archive naming only `posts` must not
    // change `orders` in any way.
    let Some(db) = migrated_test_db("backup_partial").await else {
        return;
    };
    let mut client = db.client().await;
    fixture(&mut client).await;

    let orders_before = rows(&client, "orders").await;
    let posts_before = rows(&client, "posts").await;

    let mut archive = backup::export(&client).await.expect("export");
    // Keep only `posts`, as a partial backup would.
    archive.schema.collections.retain(|c| c.name == "posts");
    archive.data.retain(|(name, _)| name == "posts");
    archive.manifest.collections.retain(|c| c.name == "posts");

    // Change `posts` so the import has something visible to do.
    client
        .batch_execute("update posts set title = 'locally edited' where id = 'p1'")
        .await
        .expect("edit");

    let bytes = backup::write(&archive).expect("write");
    let read_back = backup::read(&bytes).expect("read");
    assert!(
        !read_back
            .schema
            .collections
            .iter()
            .any(|c| c.name == "orders"),
        "the archive must not mention orders"
    );

    match backup::import(&mut client, &read_back, &ImportOptions::default())
        .await
        .expect("import")
    {
        ImportOutcome::Applied(report) => {
            assert_eq!(
                report
                    .imported
                    .iter()
                    .map(|c| c.name.as_str())
                    .collect::<Vec<_>>(),
                vec!["posts"],
                "only the named collection is planned"
            );
        }
        other => panic!("unexpected {other:?}"),
    }

    // `orders` is byte-for-byte what it was.
    assert_eq!(
        rows(&client, "orders").await,
        orders_before,
        "a table the archive omits must not change at all"
    );
    // `posts` was restored to the archive's value, and nothing was deleted.
    assert_eq!(rows(&client, "posts").await, posts_before);
    let count: i64 = client
        .query_one("select count(*) from posts", &[])
        .await
        .expect("count")
        .get(0);
    assert_eq!(count, 2, "an import never deletes");

    db.cleanup().await;
}

#[tokio::test]
async fn a_system_collection_in_an_archive_is_refused() {
    let Some(db) = migrated_test_db("backup_system").await else {
        return;
    };
    let mut client = db.client().await;
    fixture(&mut client).await;

    let superusers_before: i64 = client
        .query_one("select count(*) from _superusers", &[])
        .await
        .expect("count")
        .get(0);

    let mut archive = backup::export(&client).await.expect("export");
    // Someone edits the archive to carry a system table.
    archive.schema.collections.push(backup::ArchiveCollection {
        name: "_superusers".into(),
        collection_type: Some("auth".into()),
        schema: Vec::new(),
        rules: None,
        options: None,
        records: Vec::new(),
    });
    archive.data.push((
        "_superusers".into(),
        vec![json!({"id": "injected", "email": "attacker@example.com"})],
    ));

    let report = match backup::import(&mut client, &archive, &ImportOptions::default())
        .await
        .expect("import")
    {
        ImportOutcome::Applied(report) => report,
        other => panic!("unexpected {other:?}"),
    };

    assert_eq!(
        report.imported.len(),
        2,
        "only the two ordinary collections"
    );
    assert_eq!(report.skipped.len(), 1, "{report:?}");
    assert_eq!(report.skipped[0].0, "_superusers");
    assert!(report.skipped[0].1.contains("system"), "{report:?}");

    // Nothing was written to the system table.
    let superusers_after: i64 = client
        .query_one("select count(*) from _superusers", &[])
        .await
        .expect("count")
        .get(0);
    assert_eq!(superusers_after, superusers_before);
    let injected = client
        .query_opt("select 1 from _superusers where id = 'injected'", &[])
        .await
        .expect("query");
    assert!(injected.is_none(), "the record must not have been written");

    db.cleanup().await;
}

#[tokio::test]
async fn a_large_import_needs_confirmation_before_it_applies() {
    let Some(db) = migrated_test_db("backup_gate").await else {
        return;
    };
    let mut client = db.client().await;
    fixture(&mut client).await;

    let archive = backup::export(&client).await.expect("export");
    let bytes = backup::write(&archive).expect("write");
    let read_back = backup::read(&bytes).expect("read");

    // A threshold of zero means any import needs confirming.
    let unconfirmed = ImportOptions {
        atomic: AtomicMode::Batch,
        confirm: false,
        confirm_threshold: 0,
    };
    match backup::import(&mut client, &read_back, &unconfirmed)
        .await
        .expect("import")
    {
        ImportOutcome::NeedsConfirmation(preflight) => {
            assert!(preflight.requires_confirmation);
            assert_eq!(preflight.total_records, 3);
            assert_eq!(preflight.collections.len(), 2);
        }
        other => panic!("a large import must be gated, got {other:?}"),
    }

    // Confirming applies it, and nothing was written before the confirmation.
    let confirmed = ImportOptions {
        confirm: true,
        ..unconfirmed
    };
    match backup::import(&mut client, &read_back, &confirmed)
        .await
        .expect("import")
    {
        ImportOutcome::Applied(report) => assert_eq!(report.imported.len(), 2),
        other => panic!("unexpected {other:?}"),
    }

    db.cleanup().await;
}

#[tokio::test]
async fn the_plan_refuses_system_collections_and_names_the_reason() {
    // The pure rule, asserted once more at the crate boundary so a caller can rely
    // on it without reading the implementation.
    let archive = vec![backup::ArchiveCollection {
        name: "_settings".into(),
        collection_type: None,
        schema: Vec::new(),
        rules: None,
        options: None,
        records: Vec::new(),
    }];
    let plan = backup::plan(&archive, &[]);
    assert_eq!(plan.actions.len(), 1);
    assert!(matches!(plan.actions[0].1, ImportAction::Skip(_)));
    assert!(plan.is_empty());
}

#[tokio::test]
async fn the_atomic_modes_are_named_the_way_the_import_form_sends_them() {
    assert_eq!(
        AtomicMode::parse("batch").expect("batch"),
        AtomicMode::Batch
    );
    assert_eq!(
        AtomicMode::parse("per_collection").expect("per collection"),
        AtomicMode::PerCollection
    );
    assert_eq!(
        AtomicMode::parse("false").expect("continue"),
        AtomicMode::Continue
    );
    assert!(AtomicMode::parse("maybe").is_err());
    // The default is the safest one.
    assert_eq!(ImportOptions::default().atomic, AtomicMode::Batch);
    assert_eq!(ARCHIVE_FORMAT, "lazypock-archive");
}
