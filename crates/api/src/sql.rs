//! Read-only queries: the Studio's SQL console, and the introspection the view
//! previews need.
//!
//! The console is superuser-only and read-only in two independent ways, because
//! one is not enough:
//!
//! * A leading-keyword check refuses anything that is not `select`, `with`,
//!   `explain`, `values` or `table`. That produces a clear message rather than a
//!   database error, and it is the check a reader can see.
//! * The statement runs in a **read-only transaction**, which is the part
//!   Postgres enforces. A `with x as (insert ... returning *) select * from x`
//!   passes the keyword check and is still refused by the database, and so is a
//!   `select` that calls a function which writes.
//!
//! A statement timeout bounds how long a query can hold a pooled connection, and
//! the rows returned are capped.

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

/// How long one query may run.
///
/// A console query holds a pooled connection for its whole life, so an unbounded
/// one is a way to take the server down from the UI.
const STATEMENT_TIMEOUT_MS: u64 = 10_000;

/// The most rows the console returns.
const ROW_LIMIT: i64 = 1_000;

/// A column of a query's result.
pub struct Column {
    /// The column name.
    pub name: String,
    /// The field type it would become, for display.
    pub type_name: &'static str,
}

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
    let statement = trim_statement(&body.sql);
    ensure_read_only(&statement)?;

    let (columns, objects) = describe(&state, &statement, ROW_LIMIT).await?;

    // The console renders a grid, so the rows are arrays in column order. The
    // order comes from the described columns rather than from the objects, which
    // is what makes it stable.
    let rows: Vec<Vec<Value>> = objects
        .iter()
        .map(|object| {
            columns
                .iter()
                .map(|column| object.get(&column.name).cloned().unwrap_or(Value::Null))
                .collect()
        })
        .collect();

    Ok(Json(json!({
        "columns": columns.iter().map(|column| column.name.clone()).collect::<Vec<_>>(),
        "rows": rows,
    })))
}

/// Run a statement read-only and describe what it returns.
///
/// Returns the columns and up to `limit` rows as JSON objects. The transaction is
/// always rolled back: nothing here is meant to write, and a read-only
/// transaction has nothing to commit anyway.
pub async fn describe(
    state: &AppState,
    statement: &str,
    limit: i64,
) -> Result<(Vec<Column>, Vec<Value>), ApiError> {
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

    let described = describe_in(&transaction, statement, limit).await;

    transaction
        .rollback()
        .await
        .map_err(|err| ApiError::new(db_error(err)))?;

    described
}

/// The statement's text, without the trailing semicolon a person types.
pub fn trim_statement(sql: &str) -> String {
    sql.trim().trim_end_matches(';').trim().to_string()
}

/// Refuse a statement that is plainly not a read-only one.
///
/// This is the visible half of the enforcement, and it exists to explain rather
/// than to guard: the read-only transaction is what actually stops a write.
pub fn ensure_read_only(statement: &str) -> Result<(), ApiError> {
    if statement.is_empty() {
        return Err(ApiError::new(Error::invalid("A query is required.")));
    }
    let keyword = leading_keyword(statement);
    if !READ_ONLY_KEYWORDS.contains(&keyword.as_str()) {
        return Err(ApiError::new(Error::invalid(format!(
            "Only read-only statements can run here. {keyword:?} is not one of \
             select, with, explain, values or table."
        ))));
    }
    Ok(())
}

/// Run and describe, inside a transaction the caller owns.
async fn describe_in(
    transaction: &Transaction<'_>,
    statement: &str,
    limit: i64,
) -> Result<(Vec<Column>, Vec<Value>), ApiError> {
    if leading_keyword(statement) == "explain" {
        return explain(transaction, statement).await;
    }

    // The wrapping is also how the plan is validated: Postgres parses and plans
    // the statement, so an invalid one is refused before any row is fetched.
    let prepared = transaction
        .prepare(&format!("select * from ({statement}) as _lazypock_query"))
        .await
        .map_err(query_error)?;
    let columns: Vec<Column> = prepared
        .columns()
        .iter()
        .map(|column| Column {
            name: column.name().to_string(),
            type_name: field_type(column.type_()),
        })
        .collect();

    // Let Postgres do the conversion: it knows every type, and this keeps the
    // result working for a column type nobody thought of.
    let wrapped = format!("select to_json(t) as row from ({statement}) as t limit {limit}");
    let rows = transaction
        .query(&wrapped, &[])
        .await
        .map_err(query_error)?;
    let objects = rows.iter().map(|row| row.get::<_, Value>("row")).collect();

    Ok((columns, objects))
}

/// `explain` is not a subquery, so it cannot be wrapped the way `select` is.
///
/// It returns a handful of rows by construction, and its columns are text or
/// `json` — so no wrapping is needed, and the values convert directly.
async fn explain(
    transaction: &Transaction<'_>,
    statement: &str,
) -> Result<(Vec<Column>, Vec<Value>), ApiError> {
    let prepared = transaction.prepare(statement).await.map_err(query_error)?;
    let columns: Vec<Column> = prepared
        .columns()
        .iter()
        .map(|column| Column {
            name: column.name().to_string(),
            type_name: field_type(column.type_()),
        })
        .collect();

    let rows = transaction
        .query(&prepared, &[])
        .await
        .map_err(query_error)?;

    let objects = rows
        .iter()
        .map(|row| {
            let object: serde_json::Map<String, Value> = row
                .columns()
                .iter()
                .enumerate()
                .map(|(index, column)| {
                    let value = match *column.type_() {
                        Type::JSON | Type::JSONB => row.get::<_, Value>(index),
                        _ => row
                            .get::<_, Option<String>>(index)
                            .map(Value::String)
                            .unwrap_or(Value::Null),
                    };
                    (column.name().to_string(), value)
                })
                .collect();
            Value::Object(object)
        })
        .collect();

    Ok((columns, objects))
}

/// The field type a column would become.
///
/// The previews show this next to each column, so it says something a person
/// recognises rather than a Postgres type name.
fn field_type(column_type: &Type) -> &'static str {
    match *column_type {
        Type::BOOL => "bool",
        Type::INT2 | Type::INT4 | Type::INT8 | Type::FLOAT4 | Type::FLOAT8 | Type::NUMERIC => {
            "number"
        }
        Type::TIMESTAMP | Type::TIMESTAMPTZ | Type::DATE | Type::TIME | Type::TIMETZ => "date",
        Type::JSON | Type::JSONB => "json",
        _ => "text",
    }
}

/// A database error, as a person writing a query should see it.
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

    #[test]
    fn a_trailing_semicolon_is_not_part_of_the_statement() {
        assert_eq!(trim_statement("select 1;\n"), "select 1");
        assert_eq!(trim_statement("  select 1  "), "select 1");
    }
}
