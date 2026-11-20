//! Query helper: every parameter is `text` with an explicit cast in the SQL
//! (`$1::text`, `$2::text::int`), sent with `query_typed`, so nothing is prepared
//! under a name — safe behind a transaction-mode pooler such as PgBouncer.

use lazypock_core::Result;
use lazypock_db::pg_error;
use tokio_postgres::types::{ToSql, Type};
use tokio_postgres::{GenericClient, Row};

pub(crate) async fn q<C: GenericClient>(
    client: &C,
    sql: &str,
    params: &[&str],
) -> Result<Vec<Row>> {
    let typed: Vec<(&(dyn ToSql + Sync), Type)> = params
        .iter()
        .map(|p| (p as &(dyn ToSql + Sync), Type::TEXT))
        .collect();
    client.query_typed(sql, &typed).await.map_err(pg_error)
}

pub(crate) async fn q_opt<C: GenericClient>(
    client: &C,
    sql: &str,
    params: &[&str],
) -> Result<Option<Row>> {
    Ok(q(client, sql, params).await?.into_iter().next())
}
