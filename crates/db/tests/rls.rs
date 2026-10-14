//! Row-level security: does it actually enforce?
//!
//! Every assertion here runs through the real path a request takes — a
//! transaction with `SET LOCAL ROLE` and the `set_config` context — so a
//! "policy that exists but does nothing" fails the test.
//!
//! Requires a Postgres; see `tests/support/mod.rs`.

mod support;

use serde_json::{json, Value};

use lazypock_db::collections::{self, CreateCollection, CreateField};
use lazypock_db::pool::Db;
use lazypock_db::rls::context::{apply, missing_roles, RequestContext};
use lazypock_db::rls::policies;
use lazypock_db::types::{CollectionType, FieldType};

use support::{migrated_test_db, TestDb};

/// Create a `posts` collection with `rules` and three rows, two owned by `u1`.
async fn seed(test: &TestDb, rules: Value) {
    let mut client = test.client().await;
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
                options: json!({}),
            }],
            rules,
            options: json!({}),
            adopt_existing_table: false,
        },
    )
    .await
    .expect("create posts");

    client
        .batch_execute(
            r#"insert into "posts" ("id", "author") values ('p1', 'u1'), ('p2', 'u1'), ('p3', 'u2')"#,
        )
        .await
        .expect("seed rows");
}

/// The ids visible to a request running under `context`.
async fn visible(db: &Db, context: &RequestContext) -> Vec<String> {
    let mut client = db.client().await.expect("client");
    let transaction = client.transaction().await.expect("transaction");
    apply(&transaction, context).await.expect("apply context");

    let rows = transaction
        .query(r#"select "id" from "posts" order by "id""#, &[])
        .await
        .expect("select");
    let ids: Vec<String> = rows.iter().map(|row| row.get(0)).collect();

    transaction.commit().await.expect("commit");
    ids
}

fn user(id: &str) -> RequestContext {
    RequestContext::authenticated(id, format!("{id}@example.com"), "c1", "users")
}

#[tokio::test]
async fn the_migration_creates_the_roles_row_level_security_needs() {
    let Some(test) = migrated_test_db("rls_roles").await else {
        return;
    };
    let client = test.client().await;

    let missing = missing_roles(&client).await.expect("check roles");
    assert!(
        missing.is_empty(),
        "migration 0003 should have created them, missing: {missing:?}"
    );

    test.cleanup().await;
}

#[tokio::test]
async fn an_owner_rule_hides_everyone_elses_rows() {
    let Some(test) = migrated_test_db("rls_owner_rule").await else {
        return;
    };
    seed(&test, json!({ "listRule": "author = @request.auth.id" })).await;

    // Anonymous has no identity, so a rule that depends on one matches nothing
    // rather than everything.
    assert_eq!(
        visible(&test.db, &RequestContext::anonymous()).await,
        Vec::<String>::new()
    );

    assert_eq!(visible(&test.db, &user("u1")).await, vec!["p1", "p2"]);
    assert_eq!(visible(&test.db, &user("u2")).await, vec!["p3"]);
    assert_eq!(
        visible(&test.db, &user("u9")).await,
        Vec::<String>::new(),
        "a stranger must see nothing"
    );

    // Internal work and superusers run as the owner, which is unrestricted —
    // that is the "superuser bypasses the rules" semantic.
    assert_eq!(
        visible(&test.db, &RequestContext::owner()).await,
        vec!["p1", "p2", "p3"]
    );

    test.cleanup().await;
}

#[tokio::test]
async fn without_a_rule_everyone_but_the_service_is_denied() {
    let Some(test) = migrated_test_db("rls_default_deny").await else {
        return;
    };
    seed(&test, json!({})).await;

    // Default deny: row-level security is on and there is no policy at all.
    assert_eq!(
        visible(&test.db, &RequestContext::anonymous()).await,
        Vec::<String>::new()
    );
    assert_eq!(
        visible(&test.db, &user("u1")).await,
        Vec::<String>::new(),
        "an authenticated caller is denied too when no rule grants access"
    );

    // The owner still sees everything, so the server itself keeps working.
    assert_eq!(
        visible(&test.db, &RequestContext::owner()).await,
        vec!["p1", "p2", "p3"]
    );

    test.cleanup().await;
}

#[tokio::test]
async fn an_empty_rule_is_public() {
    let Some(test) = migrated_test_db("rls_public").await else {
        return;
    };
    seed(&test, json!({ "listRule": "" })).await;

    assert_eq!(
        visible(&test.db, &RequestContext::anonymous()).await,
        vec!["p1", "p2", "p3"]
    );

    test.cleanup().await;
}

#[tokio::test]
async fn writes_are_checked_as_well_as_reads() {
    let Some(test) = migrated_test_db("rls_writes").await else {
        return;
    };
    seed(
        &test,
        json!({
            "listRule": "author = @request.auth.id",
            "createRule": "author = @request.auth.id",
            "updateRule": "author = @request.auth.id",
            "deleteRule": "author = @request.auth.id",
        }),
    )
    .await;

    let mut client = test.db.client().await.expect("client");

    // Creating a row owned by somebody else is refused by the WITH CHECK.
    {
        let transaction = client.transaction().await.expect("tx");
        apply(&transaction, &user("u1")).await.expect("context");
        let result = transaction
            .execute(
                r#"insert into "posts" ("id", "author") values ('p9', 'u2')"#,
                &[],
            )
            .await;
        assert!(
            result.is_err(),
            "the with check must refuse a row owned by someone else"
        );
        transaction.rollback().await.expect("rollback");
    }

    // Their own row is allowed.
    {
        let transaction = client.transaction().await.expect("tx");
        apply(&transaction, &user("u1")).await.expect("context");
        transaction
            .execute(
                r#"insert into "posts" ("id", "author") values ('p9', 'u1')"#,
                &[],
            )
            .await
            .expect("own row");
        transaction.commit().await.expect("commit");
    }

    // Updating someone else's row touches nothing.
    {
        let transaction = client.transaction().await.expect("tx");
        apply(&transaction, &user("u1")).await.expect("context");
        let updated = transaction
            .execute(
                r#"update "posts" set "author" = 'u1' where "id" = 'p3'"#,
                &[],
            )
            .await
            .expect("update");
        assert_eq!(updated, 0, "another owner's row must not be updatable");
        transaction.commit().await.expect("commit");
    }

    // Deleting someone else's row removes nothing.
    {
        let transaction = client.transaction().await.expect("tx");
        apply(&transaction, &user("u1")).await.expect("context");
        let deleted = transaction
            .execute(r#"delete from "posts" where "id" = 'p3'"#, &[])
            .await
            .expect("delete");
        assert_eq!(deleted, 0, "another owner's row must not be deletable");
        transaction.commit().await.expect("commit");
    }

    // The owner can still see the row nobody was allowed to touch.
    assert!(visible(&test.db, &RequestContext::owner())
        .await
        .contains(&"p3".to_string()));

    test.cleanup().await;
}

#[tokio::test]
async fn removing_a_rule_stops_it_applying() {
    let Some(test) = migrated_test_db("rls_rule_removed").await else {
        return;
    };
    seed(&test, json!({ "listRule": "" })).await;
    assert_eq!(
        visible(&test.db, &RequestContext::anonymous()).await,
        vec!["p1", "p2", "p3"]
    );

    // Re-apply with the rule gone: policies are rebuilt, not patched, so the
    // table goes back to denying everyone.
    let client = test.client().await;
    let mut collection = collections::get(&client, "posts").await.expect("get");
    collection.rules = json!({});
    client
        .batch_execute("update _collections set rules = '{}'::jsonb where name = 'posts'")
        .await
        .expect("update rules");

    let all = collections::list(&client).await.expect("list");
    let context =
        lazypock_db::query::compile::FilterContext::for_collection(&collection, &all).unwrap();
    policies::apply(&client, &collection, &context)
        .await
        .expect("re-apply");

    assert_eq!(
        visible(&test.db, &RequestContext::anonymous()).await,
        Vec::<String>::new()
    );

    test.cleanup().await;
}

#[tokio::test]
async fn a_manage_rule_grants_every_action() {
    let Some(test) = migrated_test_db("rls_manage_rule").await else {
        return;
    };
    // `manage` is the only rule, and grants read and write for its own rows.
    seed(&test, json!({ "manageRule": "author = @request.auth.id" })).await;

    assert_eq!(visible(&test.db, &user("u1")).await, vec!["p1", "p2"]);
    assert_eq!(
        visible(&test.db, &user("u2")).await,
        vec!["p3"],
        "manage applies to reads as well"
    );
    assert_eq!(
        visible(&test.db, &RequestContext::anonymous()).await,
        Vec::<String>::new()
    );

    let mut client = test.db.client().await.expect("client");
    let transaction = client.transaction().await.expect("tx");
    apply(&transaction, &user("u1")).await.expect("context");
    let inserted = transaction
        .execute(
            r#"insert into "posts" ("id", "author") values ('p8', 'u1')"#,
            &[],
        )
        .await
        .expect("manage grants insert");
    assert_eq!(inserted, 1);
    transaction.commit().await.expect("commit");

    test.cleanup().await;
}
