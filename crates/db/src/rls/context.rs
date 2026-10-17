//! The per-request database context.

use lazypock_core::error::{Error, Result};

use crate::pool::db_error;
use crate::runner::QueryRunner;

/// Which database role a request runs as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// Stay on the connecting role, which owns the tables and so is not subject
    /// to their policies. Used for internal work and for superusers — which is
    /// exactly the "superuser bypasses the rules" semantic, and why no separate
    /// `BYPASSRLS` role is needed.
    Owner,
    /// An anonymous request: only what the public rules allow.
    Anon,
    /// A request authenticated as a record.
    Authenticated,
}

impl Role {
    /// The Postgres role name, or `None` to stay on the connecting role.
    pub fn name(self) -> Option<&'static str> {
        match self {
            Self::Owner => None,
            Self::Anon => Some("lazypock_anon"),
            Self::Authenticated => Some("lazypock_authenticated"),
        }
    }
}

/// The record action a restricted request performs.
///
/// `listRule` and `viewRule` are separate in PocketBase, but both compile to a
/// Postgres `SELECT` policy and permissive policies are OR-combined — so
/// without an explicit action a public `viewRule` would widen a filtered
/// `listRule` (and vice versa). The action is set per request in
/// `request.action` and read by the generated `SELECT` policy, so each rule
/// applies to exactly the operation it names.
///
/// `Update` and `Delete` exist because Postgres applies the `SELECT` policies
/// to the rows an `UPDATE`/`DELETE` reads to find its target: without a
/// `SELECT` mirror keyed to those actions the write would silently match no
/// rows. The mirror carries the rule's own predicate, so a row is writable
/// exactly when its `updateRule`/`deleteRule` says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// A collection list: `GET /api/collections/{c}/records`, and a
    /// collection-wide realtime subscription.
    List,
    /// A single-record read: `GET /api/collections/{c}/records/{id}`, a
    /// record-specific realtime subscription, and a batch `GET`.
    View,
    /// An update, including the read Postgres makes to locate the row.
    Update,
    /// A delete, including the read Postgres makes to locate the row.
    Delete,
    /// A create, or any other request that reads no record. No `SELECT` rule
    /// is keyed to it, so a stray read under it matches nothing — the
    /// fail-closed default.
    Write,
}

impl Action {
    /// The value the policies read from `request.action`.
    pub fn name(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::View => "view",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::Write => "write",
        }
    }
}

/// Everything a request tells the database about itself.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RequestContext {
    /// The role to switch to.
    pub role: Role,
    /// The record action the request performs.
    pub action: Action,
    /// The authenticated record id; empty when anonymous.
    pub auth_id: String,
    /// The authenticated record's email.
    pub auth_email: String,
    /// The authenticated record's collection id.
    pub auth_collection_id: String,
    /// The authenticated record's collection name.
    pub auth_collection_name: String,
    /// The client's connection id, used by realtime to suppress echoes.
    pub connection_id: String,
}

impl RequestContext {
    /// An unauthenticated request.
    ///
    /// `@request.auth.*` resolves to nothing, so a rule that depends on it
    /// matches nothing rather than everything.
    pub fn anonymous() -> Self {
        Self {
            role: Role::Anon,
            action: Action::Write,
            auth_id: String::new(),
            auth_email: String::new(),
            auth_collection_id: String::new(),
            auth_collection_name: String::new(),
            connection_id: String::new(),
        }
    }

    /// Internal work, or a superuser: no restrictions.
    pub fn owner() -> Self {
        Self {
            role: Role::Owner,
            ..Self::anonymous()
        }
    }

    /// A request authenticated as a record in `collection`.
    pub fn authenticated(
        id: impl Into<String>,
        email: impl Into<String>,
        collection_id: impl Into<String>,
        collection_name: impl Into<String>,
    ) -> Self {
        Self {
            role: Role::Authenticated,
            action: Action::Write,
            auth_id: id.into(),
            auth_email: email.into(),
            auth_collection_id: collection_id.into(),
            auth_collection_name: collection_name.into(),
            connection_id: String::new(),
        }
    }

    /// Attach the client's connection id.
    pub fn with_connection_id(mut self, connection_id: impl Into<String>) -> Self {
        self.connection_id = connection_id.into();
        self
    }

    /// Set the record action the policies read.
    ///
    /// The default is [`Action::Write`], so a read that forgets to declare
    /// itself is denied rather than silently matching a different rule.
    pub fn with_action(mut self, action: Action) -> Self {
        self.action = action;
        self
    }
}

/// The roles a request switches to.
pub const REQUEST_ROLES: &[&str] = &["lazypock_anon", "lazypock_authenticated"];

/// Which of the roles row-level security needs are missing from the database.
///
/// Reported at boot: without them a request cannot switch role, so the only safe
/// reading is that row-level security is not in force.
pub async fn missing_roles(conn: &dyn QueryRunner) -> Result<Vec<String>> {
    let names: Vec<String> = REQUEST_ROLES.iter().map(|role| role.to_string()).collect();
    let rows = conn
        .run_query(
            "select rolname from pg_roles where rolname = any($1::text[])",
            &[&names],
        )
        .await
        .map_err(db_error)?;

    let present: Vec<String> = rows.iter().map(|row| row.get(0)).collect();
    Ok(REQUEST_ROLES
        .iter()
        .filter(|role| !present.iter().any(|name| name == *role))
        .map(|role| role.to_string())
        .collect())
}

/// Which of the request roles this connection cannot actually switch to.
///
/// Reported at boot, and stricter than checking that the roles exist: `SET ROLE`
/// requires *membership*, so a role can be present and still unreachable. The
/// connecting role must be a member of each restricted role, or be a superuser.
pub async fn unreachable_roles(client: &mut deadpool_postgres::Client) -> Result<Vec<String>> {
    let mut unreachable = Vec::new();
    for role in REQUEST_ROLES {
        let transaction = client.transaction().await.map_err(db_error)?;
        let switched = transaction
            // The role name comes from a closed set.
            .execute(&format!("set local role {role}"), &[])
            .await
            .is_ok();
        let _ = transaction.rollback().await;
        if !switched {
            unreachable.push(role.to_string());
        }
    }
    Ok(unreachable)
}

/// Hand the transaction back to the owning role.
///
/// The write path needs this. A write runs under the restricted role so the
/// collection's policies authorise it — the `WITH CHECK` then sees exactly the
/// row being written — but the stored row must be read back to build the
/// response, and Postgres applies the *select* policies to a `RETURNING` row, so
/// a caller allowed to create a record but not to see it would be refused
/// outright. Reading it back as the owner is what keeps "the write response is
/// the stored record", which is the contract PocketBase clients expect.
///
/// It is `SET LOCAL`, so it is undone at commit and cannot escape the
/// transaction. A caller that runs more statements afterwards must re-apply its
/// context, which is why the batch endpoint does so for every sub-request.
pub async fn assume_owner(runner: &dyn QueryRunner) -> Result<()> {
    runner
        .run_batch_execute("set local role none")
        .await
        .map_err(db_error)?;
    Ok(())
}

/// Apply the context to the current transaction.
///
/// One statement, not two: `set_config('role', …)` is the parameterised
/// equivalent of `SET LOCAL ROLE`, so switching role and publishing the request
/// claims cost one round trip instead of two. The role name comes from a closed
/// set, never from a request; `NULL` resets `role` to the session user, which is
/// how the owner path is expressed (`SET ROLE NONE` is special syntax and cannot
/// be passed as a role name).
///
/// Must be called inside a transaction: everything here is
/// `set_config(..., true)`, so it is undone at commit or rollback and cannot leak
/// onto a pooled connection that the next request will reuse.
pub async fn apply(runner: &dyn QueryRunner, context: &RequestContext) -> Result<()> {
    // `None` is the owner: a NULL value resets `role` to the session user.
    let role = context.role.name();
    runner
        .run_query_one(
            "select set_config('role', $1::text, true), \
                    set_config('request.auth.id', $2::text, true), \
                    set_config('request.auth.email', $3::text, true), \
                    set_config('request.auth.collection_id', $4::text, true), \
                    set_config('request.auth.collection_name', $5::text, true), \
                    set_config('request.connection_id', $6::text, true), \
                    set_config('request.action', $7::text, true)",
            &[
                &role,
                &context.auth_id,
                &context.auth_email,
                &context.auth_collection_id,
                &context.auth_collection_name,
                &context.connection_id,
                &context.action.name(),
            ],
        )
        .await
        .map_err(|err| match role {
            Some(role) => Error::internal(format!(
                "cannot switch to the {role} role: is it missing, or is the connecting role \
                 not a member of it? {err}"
            )),
            None => db_error(err),
        })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_restricted_roles_switch_the_session_role() {
        assert_eq!(Role::Owner.name(), None);
        assert_eq!(Role::Anon.name(), Some("lazypock_anon"));
        assert_eq!(Role::Authenticated.name(), Some("lazypock_authenticated"));
    }

    #[test]
    fn an_anonymous_context_carries_no_identity() {
        let context = RequestContext::anonymous();
        assert_eq!(context.role, Role::Anon);
        assert!(context.auth_id.is_empty());
        assert!(context.auth_email.is_empty());
        assert!(context.auth_collection_id.is_empty());
    }

    #[test]
    fn an_authenticated_context_carries_the_account() {
        let context = RequestContext::authenticated("u1", "a@example.com", "c1", "users")
            .with_connection_id("conn1");
        assert_eq!(context.role, Role::Authenticated);
        assert_eq!(context.auth_id, "u1");
        assert_eq!(context.auth_email, "a@example.com");
        assert_eq!(context.auth_collection_id, "c1");
        assert_eq!(context.auth_collection_name, "users");
        assert_eq!(context.connection_id, "conn1");
    }

    #[test]
    fn the_owner_context_is_unrestricted() {
        let context = RequestContext::owner();
        assert_eq!(context.role, Role::Owner);
        assert!(context.auth_id.is_empty());
    }

    #[test]
    fn the_default_action_is_write_and_can_be_overridden() {
        // Write is the fail-closed default: a read that never declares itself
        // must not accidentally match `listRule` or `viewRule`.
        assert_eq!(RequestContext::anonymous().action, Action::Write);
        assert_eq!(RequestContext::owner().action, Action::Write);
        let list = RequestContext::anonymous().with_action(Action::List);
        assert_eq!(list.action, Action::List);
        assert_eq!(Action::List.name(), "list");
        assert_eq!(Action::View.name(), "view");
        assert_eq!(Action::Update.name(), "update");
        assert_eq!(Action::Delete.name(), "delete");
        assert_eq!(Action::Write.name(), "write");
    }
}
