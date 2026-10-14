//! The collections a fresh installation is expected to have.

mod support;

use lazypock_db::collections;
use lazypock_db::defaults;
use support::migrated_test_db;

#[tokio::test]
async fn a_fresh_installation_gets_a_users_collection() {
    let Some(db) = migrated_test_db("defaults_users").await else {
        return;
    };
    let mut client = db.client().await;

    assert!(
        collections::find(&client, "users")
            .await
            .expect("find")
            .is_none(),
        "a fresh installation starts without one"
    );

    assert!(defaults::ensure_users(&mut client).await.expect("ensure"));

    let users = collections::find(&client, "users")
        .await
        .expect("find")
        .expect("the users collection");
    assert_eq!(users.name, "users");
    assert!(
        users.collection_type.is_auth(),
        "it must be an auth collection, or accounts cannot log in"
    );

    // It carries the fields a profile needs, on top of the auth columns the type
    // adds for its own use.
    let names: Vec<&str> = users.fields.iter().map(|f| f.name.as_str()).collect();
    assert!(names.contains(&"name"), "{names:?}");
    assert!(names.contains(&"avatar"), "{names:?}");

    // And the rules let a user manage only their own record, with registration open.
    assert_eq!(users.rules["createRule"], serde_json::json!(""));
    for rule in ["listRule", "viewRule", "updateRule", "deleteRule"] {
        let expression = users.rules[rule].as_str().expect("a rule");
        assert!(
            expression.contains("@request.auth.id"),
            "{rule} is not scoped to the caller: {expression:?}"
        );
    }

    db.cleanup().await;
}

#[tokio::test]
async fn running_it_again_leaves_an_existing_collection_alone() {
    let Some(db) = migrated_test_db("defaults_idempotent").await else {
        return;
    };
    let mut client = db.client().await;

    assert!(defaults::ensure_users(&mut client).await.expect("first"));
    assert!(
        !defaults::ensure_users(&mut client).await.expect("second"),
        "the second run must create nothing"
    );

    let count: i64 = client
        .query_one(
            "select count(*) from _collections where name = 'users'",
            &[],
        )
        .await
        .expect("count")
        .get(0);
    assert_eq!(count, 1, "no duplicate");

    let fields: i64 = client
        .query_one(
            "select count(*) from _fields f join _collections c on c.id = f.collection_id \
             where c.name = 'users'",
            &[],
        )
        .await
        .expect("count")
        .get(0);
    assert_eq!(fields, 2, "and its fields are not duplicated either");

    db.cleanup().await;
}

#[tokio::test]
async fn an_existing_users_table_is_left_alone_rather_than_converted() {
    // Somebody else's application may already have a `users` table, of any shape. The
    // rule is that it is untouched: converting it would either fail — `create`
    // insists on every auth column, which at boot would stop the server starting
    // against a valid database — or rewrite their table. Reflection adopts it as an
    // ordinary collection instead, and the operator decides what it should be.
    let Some(db) = migrated_test_db("defaults_existing_users").await else {
        return;
    };
    let mut client = db.client().await;

    client
        .batch_execute(
            "create table users (
                id text primary key,
                email text not null,
                display_name text
             );
             insert into users (id, email, display_name) values ('u1', 'a@example.com', 'Ada');",
        )
        .await
        .expect("the existing table");

    // It is not converted, and it does not error: a boot must survive this.
    assert!(!defaults::ensure_users(&mut client).await.expect("ensure"));

    // Nothing was created, and the rows are untouched.
    assert!(
        collections::find(&client, "users")
            .await
            .expect("find")
            .is_none(),
        "no collection is registered by this function"
    );
    let count: i64 = client
        .query_one("select count(*) from users", &[])
        .await
        .expect("count")
        .get(0);
    assert_eq!(count, 1);
    let name: String = client
        .query_one("select display_name from users where id = 'u1'", &[])
        .await
        .expect("row")
        .get(0);
    assert_eq!(name, "Ada", "their column and value survive");
    let columns: i64 = client
        .query_one(
            "select count(*) from information_schema.columns \
              where table_name = 'users' and table_schema = current_schema()",
            &[],
        )
        .await
        .expect("columns")
        .get(0);
    assert_eq!(columns, 3, "no column was added to their table");

    // Reflection is what makes it visible, and it leaves it closed to the public.
    lazypock_db::reflect::reflect(&client)
        .await
        .expect("reflect");
    let adopted = collections::find(&client, "users")
        .await
        .expect("find")
        .expect("the reflected collection");
    assert_eq!(
        adopted.rules,
        serde_json::json!({}),
        "an adopted table must stay superuser-only"
    );

    db.cleanup().await;
}
