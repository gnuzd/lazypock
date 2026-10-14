//! The collections a fresh installation is expected to have.
//!
//! PocketBase ships a `users` auth collection, and so did the implementation this
//! replaced. Without one there is nowhere to put accounts and a client's collection
//! list shows no auth collection at all, which reads as "the server is broken"
//! rather than "you have not created one yet".
//!
//! Idempotent, so it is safe to run on every boot: an installation that has one, or
//! that has renamed or customised it, is left exactly as it is.

use serde_json::json;
use tokio_postgres::Client;

use lazypock_core::error::Result;

use crate::collections::{self, CreateCollection, CreateField};
use crate::types::{CollectionType, FieldType};

/// The name of the built-in auth collection.
pub const USERS: &str = "users";

/// Whether a `users` collection was created.
pub type Created = bool;

/// Create the built-in `users` auth collection if it is missing.
///
/// Its rules are PocketBase's, and they are the reason this is safe to create
/// automatically: a user may read and change **only their own** record, and anyone
/// may register. A collection created with no rules would instead be
/// superuser-only, which would leave registration impossible and look like a bug.
pub async fn ensure_users(client: &mut Client) -> Result<Created> {
    if collections::find(client, USERS).await?.is_some() {
        return Ok(false);
    }

    // A `users` table that is not already a collection belongs to somebody else's
    // application, or is a different shape. **Left alone, and reported.**
    //
    // Converting it is not on the table: `create` would insist it already carry every
    // auth column and refuse otherwise — which, at boot, would stop the server from
    // starting against a perfectly valid existing database. Adopting and rewriting it
    // would be worse: it is their table. Schema reflection picks it up as an ordinary
    // collection, with superuser-only access, and the operator decides what it should
    // be.
    if collections::table_exists(client, USERS).await? {
        tracing::warn!(
            table = USERS,
            "a users table already exists but is not a collection; leaving it untouched. \
             It will be adopted as an ordinary collection with superuser-only access \
             rather than converted into an auth collection."
        );
        return Ok(false);
    }

    collections::create(
        client,
        CreateCollection {
            name: USERS.to_string(),
            collection_type: CollectionType::Auth,
            fields: vec![
                CreateField {
                    name: "name".into(),
                    field_type: FieldType::Text,
                    required: false,
                    unique: false,
                    options: json!({}),
                },
                CreateField {
                    name: "avatar".into(),
                    field_type: FieldType::File,
                    required: false,
                    unique: false,
                    options: json!({ "maxSelect": 1 }),
                },
            ],
            // PocketBase's defaults. `@request.auth.id` is supported by this
            // server's rule compiler, so these are the reference rules rather than
            // an approximation.
            rules: json!({
                "listRule": "id = @request.auth.id",
                "viewRule": "id = @request.auth.id",
                "createRule": "",
                "updateRule": "id = @request.auth.id",
                "deleteRule": "id = @request.auth.id",
            }),
            options: json!({}),
            adopt_existing_table: false,
        },
    )
    .await?;

    tracing::info!(collection = USERS, "created the built-in users collection");
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_collection_keeps_the_name_its_clients_expect() {
        // Hard-coded on the client side, in the previous implementation and in
        // PocketBase, so it is not ours to change.
        assert_eq!(USERS, "users");
    }

    #[test]
    fn the_default_rules_let_a_user_manage_only_their_own_record() {
        // Asserted as a property rather than a string: every rule that reads or
        // writes a record is scoped to the caller, and only creation is open.
        let rules = json!({
            "listRule": "id = @request.auth.id",
            "viewRule": "id = @request.auth.id",
            "createRule": "",
            "updateRule": "id = @request.auth.id",
            "deleteRule": "id = @request.auth.id",
        });

        for rule in ["listRule", "viewRule", "updateRule", "deleteRule"] {
            let expression = rules[rule].as_str().expect("a rule");
            assert!(
                expression.contains("@request.auth.id"),
                "{rule} must be scoped to the caller, got {expression:?}"
            );
        }

        // Registration is open, which is what makes the collection usable out of the
        // box, and it is the only rule that is.
        assert_eq!(rules["createRule"], json!(""));
    }
}
