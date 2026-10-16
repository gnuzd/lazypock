//! The Studio's SQL console: `POST /api/sql/query`.
//!
//! Superuser-only, and read-only in two independent ways, because one is not
//! enough:
//!
//! * A leading-keyword check refuses anything that is not `select`, `with`,
//!   `explain`, `values` or `table`. That produces a clear message rather than a
//!   database error, and it is the check a reader can see.
//! * The statement runs in a **read-only transaction**, which is the part
//!   Postgres enforces. A `with x as (insert ... returning *) select * from x`
//!   passes the keyword check and is still refused by the database, and so is a
//!   `select` that calls a function which writes.
//!
//! A statement timeout bounds how long a console query can hold a pooled
//! connection, and the rows returned are capped.

use axum::extract::State;
use axum::Json;
use lazypock_core::error::Error;
use lazypock_db::deadpool_postgres::Transaction;
use lazypock_db::pool::db_error;
use lazypock_db::tokio_postgres::types::Type;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::auth::Superuser;
use crate::error::ApiError;
use crate::state::AppState;

/// The statements the console accepts, by their first keyword.
const READ_ONLY_KEYWORDS: &[&str] = &["select", "with", "explain", "values", "table"];

/// How long one console query may run.
///
/// A console query holds a pooled connection for its whole life, so an unbounded
/// one is a way to take the server down from the UI.
const STATEMENT_TIMEOUT_MS: u64 = 10_000;

/// The most rows the console returns.
const ROW_LIMIT: i64 = 1_000;

/// What `POST /api/sql/query` sends.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlQuery {
    /// The statement to run.
    pub sql: String,
}

/// `POST /api/sql/query` — run one read-only statement.
pub async fn query(
    State(state): State<AppState>,
    _: Superuser,
    Json(body): Json<SqlQuery>,
) -> Result<Json<Value>, ApiError> {
    // A trailing semicolon is what a person types; it is not part of the statement
    // once it is wrapped in a subquery.
    let statement = body.sql.trim().trim_end_matches(';').trim().to_string();
    if statement.is_empty() {
        return Err(ApiError::new(Error::invalid("A query is required.")));
    }

    let keyword = leading_keyword(&statement);
    if !READ_ONLY_KEYWORDS.contains(&keyword.as_str()) {
        return Err(ApiError::new(Error::invalid(format!(
            "The SQL console runs read-only statements only. {keyword:?} is not one of \
             select, with, explain, values or table."
        ))));
    }

    let mut client = state.db.client().await.map_err(ApiError::new)?;
    let transaction = client
        .build_transaction()
        .read_only(true)
        .start()
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    // `set local`, so it reverts with the transaction and cannot leak onto the
    // connection when it goes back in the pool.
    transaction
        .batch_execute(&format!(
            "set local statement_timeout = {STATEMENT_TIMEOUT_MS}"
        ))
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    let (columns, rows) = run(&transaction, &statement, ROW_LIMIT).await?;

    // Nothing was written, so rolling back is the honest end. It also releases the
    // read-only transaction promptly rather than leaving it to the drop.
    transaction
        .rollback()
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    Ok(Json(json!({ "columns": columns, "rows": rows })))
}

/// Run a validated statement, returning its column names and its rows.
async fn run(
    transaction: &Transaction<'_>,
    statement: &str,
    limit: i64,
) -> Result<(Vec<String>, Vec<Vec<Value>>), ApiError> {
    if leading_keyword(statement) == "explain" {
        return explain(transaction, statement).await;
    }

    // Column order comes from the prepared statement rather than from the rows,
    // for two reasons: a `jsonb` object does not preserve key order, and a query
    // that matches nothing has no rows at all — which is exactly when the console
    // most needs to show headers.
    let described = transaction
        .prepare(&format!("select * from ({statement}) as _lazypock_query"))
        .await
        .map_err(query_error)?;
    let columns: Vec<String> = described
        .columns()
        .iter()
        .map(|column| column.name().to_string())
        .collect();

    // Let Postgres do the conversion: it knows every type, and this keeps the
    // console working for a column type nobody thought of.
    let wrapped = format!("select to_json(t) as row from ({statement}) as t limit {limit}");
    let rows = transaction
        .query(&wrapped, &[])
        .await
        .map_err(query_error)?;

    let rows = rows
        .iter()
        .map(|row| {
            let object: Value = row.get("row");
            columns
                .iter()
                .map(|name| object.get(name).cloned().unwrap_or(Value::Null))
                .collect()
        })
        .collect();

    Ok((columns, rows))
}

/// `explain` is not a subquery, so it cannot be wrapped the way `select` is.
///
/// It returns a handful of rows by construction, and its columns are text or
/// `json` — so no wrapping is needed, and the values convert directly.
async fn explain(
    transaction: &Transaction<'_>,
    statement: &str,
) -> Result<(Vec<String>, Vec<Vec<Value>>), ApiError> {
    let described = transaction.prepare(statement).await.map_err(query_error)?;
    let columns: Vec<String> = described
        .columns()
        .iter()
        .map(|column| column.name().to_string())
        .collect();

    let rows = transaction
        .query(&described, &[])
        .await
        .map_err(query_error)?;

    let rows = rows
        .iter()
        .map(|row| {
            row.columns()
                .iter()
                .enumerate()
                .map(|(index, column)| match *column.type_() {
                    Type::JSON | Type::JSONB => row.get::<_, Value>(index),
                    _ => row
                        .get::<_, Option<String>>(index)
                        .map(Value::String)
                        .unwrap_or(Value::Null),
                })
                .collect()
        })
        .collect();

    Ok((columns, rows))
}

/// A database error, as the console should show it.
///
/// The driver's own `Display` is only "db error"; this keeps Postgres's message,
/// which is the part that says what is wrong with the query.
fn query_error(err: lazypock_db::tokio_postgres::Error) -> ApiError {
    ApiError::new(Error::invalid(format!(
        "Invalid query. Raw error: {}",
        db_error(err).message()
    )))
}

/// The statement's first word, lower-cased, skipping any leading parentheses.
fn leading_keyword(sql: &str) -> String {
    sql.trim_start_matches(|c: char| c.is_whitespace() || c == '(')
        .split(|c: char| c.is_whitespace() || c == '(')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_leading_keyword_ignores_case_whitespace_and_parentheses() {
        assert_eq!(leading_keyword("select 1"), "select");
        assert_eq!(leading_keyword("  SELECT 1"), "select");
        assert_eq!(leading_keyword("(select 1)"), "select");
        assert_eq!(
            leading_keyword("with x as (select 1) select * from x"),
            "with"
        );
        assert_eq!(leading_keyword("explain select 1"), "explain");
        assert_eq!(leading_keyword("insert into t values (1)"), "insert");
        assert_eq!(leading_keyword(""), "");
    }

    #[test]
    fn every_read_only_keyword_is_recognised() {
        for keyword in READ_ONLY_KEYWORDS {
            assert_eq!(leading_keyword(&format!("{keyword} 1")), *keyword);
        }
    }
}
