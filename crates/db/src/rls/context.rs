//! The per-request context.
//!
//! Every request runs in a transaction that (1) switches to `lazypock_anon` or
//! `lazypock_authenticated` and (2) publishes who is asking as
//! transaction-local settings (`request.auth.id`, ...) which the `auth.*` SQL
//! helpers (migrations 0003, 0017) read from inside policies.
//!
//! All of it is `set_config(.., true)`: undone at COMMIT/ROLLBACK, so nothing
//! leaks to the next user of a pooled connection — including one shared through
//! PgBouncer in transaction mode.

use crate::error::PgExt;
use crate::params::typed;
use deadpool_postgres::Object;
use tokio_postgres::Transaction;

use lazypock_core::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The connecting (schema-owner) role: unrestricted. Internal work only.
    Owner,
    Anon,
    Authenticated,
}

impl Role {
    fn name(self) -> Option<&'static str> {
        match self {
            Role::Owner => None,
            Role::Anon => Some("lazypock_anon"),
            Role::Authenticated => Some("lazypock_authenticated"),
        }
    }
}

/// What the request is doing. Read by the generated `SELECT` policies so a public
/// `viewRule` cannot widen a filtered `listRule` (and the reverse).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    List,
    View,
    Update,
    Delete,
    Write,
}

impl Action {
    pub fn name(self) -> &'static str {
        match self {
            Action::List => "list",
            Action::View => "view",
            Action::Update => "update",
            Action::Delete => "delete",
            Action::Write => "write",
        }
    }
}

#[derive(Clone, Debug)]
pub struct RequestContext {
    pub role: Role,
    pub action: Action,
    pub auth_id: String,
    pub auth_email: String,
    pub auth_collection_id: String,
    pub auth_collection_name: String,
    /// JSON object of the signed-in record's columns, for `@request.auth.<name>`.
    pub auth_claims: String,
    pub connection_id: String,
}

impl RequestContext {
    pub fn anonymous() -> Self {
        RequestContext {
            role: Role::Anon,
            action: Action::Write,
            auth_id: String::new(),
            auth_email: String::new(),
            auth_collection_id: String::new(),
            auth_collection_name: String::new(),
            auth_claims: "{}".into(),
            connection_id: String::new(),
        }
    }

    pub fn owner() -> Self {
        RequestContext {
            role: Role::Owner,
            ..Self::anonymous()
        }
    }

    pub fn authenticated(
        id: impl Into<String>,
        email: impl Into<String>,
        collection_id: impl Into<String>,
        collection_name: impl Into<String>,
    ) -> Self {
        RequestContext {
            role: Role::Authenticated,
            auth_id: id.into(),
            auth_email: email.into(),
            auth_collection_id: collection_id.into(),
            auth_collection_name: collection_name.into(),
            ..Self::anonymous()
        }
    }

    pub fn with_auth_claims(mut self, claims: impl Into<String>) -> Self {
        self.auth_claims = claims.into();
        self
    }

    pub fn with_connection_id(mut self, id: impl Into<String>) -> Self {
        self.connection_id = id.into();
        self
    }

    pub fn with_action(mut self, action: Action) -> Self {
        self.action = action;
        self
    }
}

/// Start a transaction and apply `ctx` to it.
pub async fn begin<'a>(conn: &'a mut Object, ctx: &RequestContext) -> Result<Transaction<'a>> {
    let tx = begin_owner(conn).await?;
    apply(&tx, ctx).await?;
    Ok(tx)
}

/// Start a plain transaction as the connecting role (schema work, trusted code).
pub async fn begin_owner(conn: &mut Object) -> Result<Transaction<'_>> {
    // Deref to the plain driver client: deadpool's own `Transaction` wrapper adds
    // a prepared-statement cache, which poolers cannot support.
    let client: &mut tokio_postgres::Client = conn;
    client.transaction().await.pg()
}

async fn apply(tx: &Transaction<'_>, ctx: &RequestContext) -> Result<()> {
    // `set_config('role', NULL, ..)` resets to the session user: that is the owner.
    let role = ctx.role.name();
    let action = ctx.action.name();
    tx.query_typed(
        "select set_config('role', $1::text, true), \
                set_config('request.auth.id', $2::text, true), \
                set_config('request.auth.email', $3::text, true), \
                set_config('request.auth.collection_id', $4::text, true), \
                set_config('request.auth.collection_name', $5::text, true), \
                set_config('request.connection_id', $6::text, true), \
                set_config('request.action', $7::text, true), \
                set_config('request.auth.claims', $8::text, true)",
        &[
            typed!(role, TEXT),
            typed!(ctx.auth_id, TEXT),
            typed!(ctx.auth_email, TEXT),
            typed!(ctx.auth_collection_id, TEXT),
            typed!(ctx.auth_collection_name, TEXT),
            typed!(ctx.connection_id, TEXT),
            typed!(action, TEXT),
            typed!(ctx.auth_claims, TEXT),
        ],
    )
    .await
    .pg()?;
    Ok(())
}

/// Drop back to the connecting (owner) role for the rest of this transaction.
///
/// Used to read a freshly written record back: a create is judged by `createRule`
/// alone, so the response must not depend on `viewRule` (a user may be allowed to
/// create what they are not allowed to list).
pub async fn assume_owner<C: tokio_postgres::GenericClient>(client: &C) -> Result<()> {
    client
        .query_typed("select set_config('role', null, true)", &[])
        .await
        .pg()?;
    Ok(())
}
