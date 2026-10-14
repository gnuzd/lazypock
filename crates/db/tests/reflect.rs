//! Reflecting an ordinary existing table into a collection.
//!
//! Requires a Postgres; see `tests/support/mod.rs`.

mod support;

use lazypock_db::reflect::{self, reflect as run_reflect};
use serde_json::{json, Value};

use support::migrated_test_db;

/// A table that looks like somebody else's application schema.
const LEGACY: &str = r#"
create type order_state as enum ('draft', 'paid');
create table orders (
    id          uuid primary key default gen_random_uuid(),
    reference   text not null unique,
    total       numeric(10,2) not null default 0,
    state       order_state not null default 'draft',
    tags        text[] not null default '{}',
    meta        jsonb,
    placed_at   timestamptz,
    created_at  timestamptz not null default now(),
    updated_at  timestamptz not null default now()
);
insert into orders (reference, total) values ('A-1', 42.50);
"#;

#[tokio::test]
async fn an_existing_table_becomes_a_collection_with_its_columns_as_fields() {
    let Some(db) = migrated_test_db("reflect_adopt").await else {
        return;
    };
    let client = db.client().await;
    client
        .batch_execute(LEGACY)
        .await
        .expect("the legacy schema");

    let report = run_reflect(&client).await.expect("reflect");
    assert!(report.adopted.contains(&"orders".to_string()), "{report:?}");

    // The collection is superuser-only: adopting a table must not publish it.
    let row = client
        .query_one(
            "select rules, options from _collections where name = 'orders'",
            &[],
        )
        .await
        .expect("the collection");
    let rules: Value = row.get("rules");
    assert_eq!(rules, json!({}), "an adopted collection must stay closed");
    let options: Value = row.get("options");
    assert_eq!(options["reflected"], json!(true));
    assert_eq!(options["primaryKey"], json!("id"));

    // Fields reflect the columns.
    let rows = client
        .query(
            "select f.name, f.type, f.required, f.is_unique \
               from _fields f join _collections c on c.id = f.collection_id \
              where c.name = 'orders' order by f.position",
            &[],
        )
        .await
        .expect("fields");
    let fields: Vec<(String, String, bool, bool)> = rows
        .iter()
        .map(|row| {
            (
                row.get::<_, String>("name"),
                row.get::<_, String>("type"),
                row.get::<_, bool>("required"),
                row.get::<_, bool>("is_unique"),
            )
        })
        .collect();

    let find = |name: &str| {
        fields
            .iter()
            .find(|(field, _, _, _)| field == name)
            .unwrap_or_else(|| panic!("no field named {name} in {fields:?}"))
    };

    assert_eq!(find("id").1, "text", "a uuid key is text to lazypock");
    assert!(!find("id").2, "a primary key is not required of the caller");
    assert_eq!(find("reference").1, "text");
    assert!(find("reference").2, "not null becomes required");
    assert!(find("reference").3, "a unique index becomes unique");
    assert_eq!(find("total").1, "number");
    assert!(
        !find("total").2,
        "a not-null column with a default is not required of the caller"
    );
    assert_eq!(find("state").1, "select", "an enum is a select");
    assert_eq!(find("tags").1, "json", "an array keeps every element");
    assert_eq!(find("meta").1, "json");
    assert_eq!(find("placed_at").1, "date");
    assert_eq!(
        find("created_at").1,
        "autodate",
        "a clock column with a default is the server's"
    );
    assert_eq!(find("updated_at").1, "autodate");

    // The enum's labels came across, not just its type.
    let state = client
        .query_one(
            "select f.options from _fields f join _collections c on c.id = f.collection_id \
              where c.name = 'orders' and f.name = 'state'",
            &[],
        )
        .await
        .expect("state field");
    let state: Value = state.get("options");
    assert_eq!(state["values"], json!(["draft", "paid"]));

    // The table's own data is untouched.
    let count: i64 = client
        .query_one("select count(*) from orders", &[])
        .await
        .expect("count")
        .get(0);
    assert_eq!(count, 1, "reflection must not touch the rows");

    db.cleanup().await;
}

#[tokio::test]
async fn reflecting_twice_does_not_duplicate_a_collection() {
    let Some(db) = migrated_test_db("reflect_idempotent").await else {
        return;
    };
    let client = db.client().await;
    client
        .batch_execute(LEGACY)
        .await
        .expect("the legacy schema");

    run_reflect(&client).await.expect("first reflect");
    let second = run_reflect(&client).await.expect("second reflect");

    assert!(
        !second.adopted.contains(&"orders".to_string()),
        "a second run must not adopt it again: {second:?}"
    );
    assert_eq!(second.fields, 0);

    let collections: i64 = client
        .query_one(
            "select count(*) from _collections where name = 'orders'",
            &[],
        )
        .await
        .expect("count")
        .get(0);
    assert_eq!(collections, 1);

    // ... and the fields were not doubled either.
    let fields: i64 = client
        .query_one(
            "select count(*) from _fields f join _collections c on c.id = f.collection_id \
              where c.name = 'orders'",
            &[],
        )
        .await
        .expect("count")
        .get(0);
    assert_eq!(fields, 9);

    db.cleanup().await;
}

#[tokio::test]
async fn lazypocks_own_tables_are_never_adopted() {
    let Some(db) = migrated_test_db("reflect_skips_internal").await else {
        return;
    };
    let client = db.client().await;

    let report = run_reflect(&client).await.expect("reflect");
    assert!(report.is_empty(), "nothing to adopt: {report:?}");

    // Its own system tables are named among the skips rather than silently absent.
    let skipped: Vec<&String> = report.skipped.iter().map(|(table, _)| table).collect();
    assert!(
        skipped.iter().any(|table| table.as_str() == "_collections"),
        "{skipped:?}"
    );

    db.cleanup().await;
}

#[tokio::test]
async fn a_table_without_an_id_column_is_still_reflected_but_records_its_key() {
    // Somebody else's table may key on something else. It still becomes a
    // collection, and the real key is recorded rather than assumed.
    let Some(db) = migrated_test_db("reflect_no_id").await else {
        return;
    };
    let client = db.client().await;
    client
        .batch_execute(
            "create table audit_log (entry_id bigserial primary key, note text not null);",
        )
        .await
        .expect("the fixture");

    let report = reflect::reflect(&client).await.expect("reflect");
    assert!(
        report.adopted.contains(&"audit_log".to_string()),
        "{report:?}"
    );

    let row = client
        .query_one(
            "select options from _collections where name = 'audit_log'",
            &[],
        )
        .await
        .expect("the collection");
    let options: Value = row.get("options");
    assert_eq!(
        options["primaryKey"],
        json!("entry_id"),
        "the real key is recorded, not assumed to be `id`"
    );

    db.cleanup().await;
}
