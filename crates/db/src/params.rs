//! Typed parameter binding that is safe behind PgBouncer (transaction mode).
//!
//! `Client::query(sql, params)` creates a *named* prepared statement, which a
//! transaction-mode pooler cannot route (the next query may land on another
//! server connection). `query_typed` sends Parse/Bind/Execute in one round trip
//! on the *unnamed* statement instead, so nothing is left on the server — at the
//! price of declaring each parameter's Postgres type, which we always know.

use crate::error::PgExt;
use sea_query::{Value, Values};
use tokio_postgres::types::{ToSql, Type};
use tokio_postgres::{GenericClient, Row};

use lazypock_core::Result;

/// `(&value, TYPE)` with the `dyn ToSql` coercion `query_typed` needs.
macro_rules! typed {
    ($v:expr, $t:ident) => {
        (
            &$v as &(dyn tokio_postgres::types::ToSql + Sync),
            tokio_postgres::types::Type::$t,
        )
    };
}
pub(crate) use typed;

/// Owned parameters converted from sea-query `Values`.
pub(crate) struct Params(Vec<(Box<dyn ToSql + Sync + Send>, Type)>);

impl Params {
    pub(crate) fn from_values(values: &Values) -> Result<Self> {
        let mut out: Vec<(Box<dyn ToSql + Sync + Send>, Type)> = Vec::with_capacity(values.0.len());
        for v in &values.0 {
            let item: (Box<dyn ToSql + Sync + Send>, Type) = match v {
                Value::String(s) => (Box::new(s.as_ref().map(|b| (**b).clone())), Type::TEXT),
                Value::BigInt(i) => (Box::new(*i), Type::INT8),
                Value::BigUnsigned(u) => (Box::new(u.map(|u| u as i64)), Type::INT8),
                Value::Double(f) => (Box::new(*f), Type::FLOAT8),
                Value::Bool(b) => (Box::new(*b), Type::BOOL),
                Value::Json(j) => (Box::new(j.as_ref().map(|b| (**b).clone())), Type::JSONB),
                other => {
                    return Err(lazypock_core::Error::Internal(format!(
                        "unsupported bind value {other:?}"
                    )));
                }
            };
            out.push(item);
        }
        Ok(Params(out))
    }

    fn as_typed(&self) -> Vec<(&(dyn ToSql + Sync), Type)> {
        self.0
            .iter()
            .map(|(b, t)| (&**b as &(dyn ToSql + Sync), t.clone()))
            .collect()
    }
}

/// Run a sea-query-built statement with its bound values.
pub(crate) async fn query_values<C: GenericClient>(
    client: &C,
    sql: &str,
    values: &Values,
) -> Result<Vec<Row>> {
    let params = Params::from_values(values)?;
    Ok(client.query_typed(sql, &params.as_typed()).await.pg()?)
}
