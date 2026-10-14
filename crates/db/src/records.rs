//! Reading and writing single records.
//!
//! Writes validate first ([`crate::validation`]), then bind every value as a
//! parameter, and return the stored row as a JSON document built by Postgres —
//! the same shape the read path produces, so a create and a fetch are
//! indistinguishable to a client.

use serde_json::{Map, Value};
use tokio_postgres::types::ToSql;

use lazypock_core::error::{Error, Result};

use crate::collections::{Collection, Field};
use crate::ddl::quote_ident;
use crate::pool::db_error;
use crate::query::compile::FilterContext;
use crate::query::projection::{ExpandSpec, FieldSelection};
use crate::query::read;
use crate::runner::QueryRunner;
use crate::types::FieldType;
use crate::validation::{self, ValidationErrors};

/// Why a record write failed.
#[derive(Debug)]
pub enum RecordWriteError {
    /// Field-level validation failures, rendered as PocketBase's error `data`.
    Validation(ValidationErrors),
    /// Anything else.
    Other(Error),
}

impl From<Error> for RecordWriteError {
    fn from(error: Error) -> Self {
        Self::Other(error)
    }
}

impl RecordWriteError {
    /// HTTP status this failure maps to.
    pub fn status(&self) -> u16 {
        match self {
            Self::Validation(_) => 400,
            Self::Other(error) => error.status(),
        }
    }

    /// Machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Validation(_) => "validation_error",
            Self::Other(error) => error.code(),
        }
    }

    /// Client-facing message.
    pub fn message(&self) -> String {
        match self {
            Self::Validation(errors) => format!("Failed to save the record. {}", errors.summary()),
            Self::Other(error) => error.message().to_string(),
        }
    }

    /// Per-field details, empty for non-validation failures.
    pub fn data(&self) -> Value {
        match self {
            Self::Validation(errors) => errors.to_json(),
            Self::Other(_) => Value::Object(Map::new()),
        }
    }
}

impl std::fmt::Display for RecordWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for RecordWriteError {}

/// Insert a record and return it.
pub async fn create(
    runner: &dyn QueryRunner,
    collection: &Collection,
    context: &FilterContext,
    input: Value,
) -> std::result::Result<Value, RecordWriteError> {
    create_with(runner, collection, context, input, &[]).await
}

/// Insert a record, adding server-managed columns the caller has computed.
///
/// `extra` carries values such as an auth account's password hash and token key,
/// which the record layer cannot derive itself.
pub async fn create_with(
    runner: &dyn QueryRunner,
    collection: &Collection,
    context: &FilterContext,
    input: Value,
    extra: &[(String, Option<String>)],
) -> std::result::Result<Value, RecordWriteError> {
    let object = expect_object(&input)?;
    validation::validate(collection, object, false).map_err(RecordWriteError::Validation)?;

    let id = resolve_id(object)?;
    let mut columns: Vec<String> = vec!["id".to_string()];
    let mut placeholders: Vec<String> = vec!["$1::text".to_string()];
    let mut params: Vec<String> = vec![id];

    for writable in writable_columns(collection).map_err(RecordWriteError::Other)? {
        let Some(value) = object.get(writable.name) else {
            continue;
        };
        columns.push(writable.name.to_string());
        placeholders.push(writable_placeholder(&writable, value, &mut params)?);
    }

    for (name, value) in extra {
        columns.push(name.clone());
        placeholders.push(text_placeholder(value, &mut params));
    }

    let document = record_document(collection, context)?;
    let sql = format!(
        "insert into {} ({}) values ({}) returning {document} as record",
        quote_ident(&collection.name),
        columns
            .iter()
            .map(|column| quote_ident(column))
            .collect::<Vec<_>>()
            .join(", "),
        placeholders.join(", "),
    );

    let row = runner
        .run_query_one(&sql, &bound(&params))
        .await
        .map_err(|err| map_write_error(err, &collection.name))?;
    Ok(row.get("record"))
}

/// Update a record, returning the stored row, or `None` when it does not exist.
pub async fn update(
    runner: &dyn QueryRunner,
    collection: &Collection,
    context: &FilterContext,
    id: &str,
    input: Value,
) -> std::result::Result<Option<Value>, RecordWriteError> {
    update_with(runner, collection, context, id, input, &[]).await
}

/// Update a record, adding server-managed columns the caller has computed.
pub async fn update_with(
    runner: &dyn QueryRunner,
    collection: &Collection,
    context: &FilterContext,
    id: &str,
    input: Value,
    extra: &[(String, Option<String>)],
) -> std::result::Result<Option<Value>, RecordWriteError> {
    let object = expect_object(&input)?;
    validation::validate(collection, object, true).map_err(RecordWriteError::Validation)?;

    let mut assignments: Vec<String> = Vec::new();
    let mut params: Vec<String> = Vec::new();

    for writable in writable_columns(collection).map_err(RecordWriteError::Other)? {
        let Some(value) = object.get(writable.name) else {
            continue;
        };
        let placeholder = writable_placeholder(&writable, value, &mut params)?;
        assignments.push(format!("{} = {placeholder}", quote_ident(writable.name)));
    }

    for (name, value) in extra {
        let placeholder = text_placeholder(value, &mut params);
        assignments.push(format!("{} = {placeholder}", quote_ident(name)));
    }

    // A write happened, so the row's `updated` moves even if nothing else did.
    assignments.push("\"updated\" = now()".to_string());

    params.push(id.to_string());
    let id_placeholder = format!("${}::text", params.len());

    let document = record_document(collection, context)?;
    let sql = format!(
        "update {} set {} where \"id\" = {id_placeholder} returning {document} as record",
        quote_ident(&collection.name),
        assignments.join(", "),
    );

    let row = runner
        .run_query_opt(&sql, &bound(&params))
        .await
        .map_err(|err| map_write_error(err, &collection.name))?;
    Ok(row.map(|row| row.get("record")))
}

/// Delete a record. Returns whether a row was removed.
pub async fn delete(
    runner: &dyn QueryRunner,
    collection: &Collection,
    id: &str,
) -> std::result::Result<bool, RecordWriteError> {
    let sql = format!(
        "delete from {} where \"id\" = $1::text",
        quote_ident(&collection.name)
    );
    let params = [id.to_string()];
    let removed = runner
        .run_execute(&sql, &bound(&params))
        .await
        .map_err(|err| map_write_error(err, &collection.name))?;
    Ok(removed > 0)
}

/// Fetch one record, or `None` when it does not exist.
pub async fn get_one(
    runner: &dyn QueryRunner,
    collection: &Collection,
    context: &FilterContext,
    id: &str,
    selection: &FieldSelection,
    expand: &ExpandSpec,
) -> Result<Option<Value>> {
    let document =
        read::record_document(context, &quote_ident(&collection.name), selection, expand)?;
    let sql = format!(
        "select {document} as record from {} where \"id\" = $1::text",
        quote_ident(&collection.name)
    );
    let params = [id.to_string()];
    let row = runner
        .run_query_opt(&sql, &bound(&params))
        .await
        .map_err(db_error)?;
    Ok(row.map(|row| row.get("record")))
}

fn expect_object(input: &Value) -> std::result::Result<&Map<String, Value>, RecordWriteError> {
    input.as_object().ok_or_else(|| {
        RecordWriteError::Other(Error::invalid(
            "the request body must be a JSON object of field values",
        ))
    })
}

/// Honour a client-supplied id when it is valid, otherwise generate one.
fn resolve_id(object: &Map<String, Value>) -> std::result::Result<String, RecordWriteError> {
    match object.get("id") {
        None | Some(Value::Null) => Ok(lazypock_core::id::new_id()),
        Some(Value::String(raw)) if raw.is_empty() => Ok(lazypock_core::id::new_id()),
        Some(Value::String(raw)) => {
            if !lazypock_core::id::is_valid_id(raw) {
                return Err(RecordWriteError::Other(Error::invalid(
                    "`id` must be 15 characters from [a-zA-Z0-9]",
                )));
            }
            Ok(raw.clone())
        }
        Some(_) => Err(RecordWriteError::Other(Error::invalid(
            "`id` must be a string",
        ))),
    }
}

fn record_document(
    collection: &Collection,
    context: &FilterContext,
) -> std::result::Result<String, RecordWriteError> {
    read::record_document(
        context,
        &quote_ident(&collection.name),
        &FieldSelection::all(),
        &ExpandSpec::none(),
    )
    .map_err(RecordWriteError::Other)
}

/// A column a client may write.
struct Writable<'a> {
    name: &'a str,
    /// The Postgres column type, used as the parameter cast.
    column_type: &'static str,
    /// The user field backing the column; `None` for an auth system column.
    field: Option<&'a Field>,
}

/// The columns a client may write.
///
/// That is the collection's own fields, plus the auth system columns for an auth
/// collection — those are columns rather than user-defined fields, so without
/// this an account could never be given an email.
fn writable_columns(collection: &Collection) -> Result<Vec<Writable<'_>>> {
    let mut columns = Vec::with_capacity(collection.fields.len() + 2);

    for field in &collection.fields {
        if field.field_type == FieldType::Autodate {
            // Server-owned: input is ignored rather than written.
            continue;
        }
        columns.push(Writable {
            name: field.name.as_str(),
            column_type: field.column_type()?,
            field: Some(field),
        });
    }

    if collection.collection_type.is_auth() {
        columns.push(Writable {
            name: "email",
            column_type: "text",
            field: None,
        });
        columns.push(Writable {
            name: "verified",
            column_type: "boolean",
            field: None,
        });
    }

    Ok(columns)
}

/// Render the bind placeholder for a writable column, appending to `params`.
fn writable_placeholder(
    writable: &Writable<'_>,
    value: &Value,
    params: &mut Vec<String>,
) -> std::result::Result<String, RecordWriteError> {
    let Some(field) = writable.field else {
        // An auth system column: plain text or boolean, already validated.
        let encoded = if value.is_null() {
            None
        } else if writable.column_type == "boolean" {
            Some(if value.as_bool().unwrap_or(false) {
                "true".to_string()
            } else {
                "false".to_string()
            })
        } else {
            Some(value.as_str().unwrap_or_default().to_string())
        };
        return Ok(text_placeholder(&encoded, params));
    };

    placeholder(field, value, params)
}

/// A placeholder for a raw text value, or SQL `null` when there is none.
fn text_placeholder(value: &Option<String>, params: &mut Vec<String>) -> String {
    match value {
        None => "null".to_string(),
        Some(text) => {
            params.push(text.clone());
            format!("${}::text", params.len())
        }
    }
}

/// Render the bind placeholder for a field's value, appending to `params`.
fn placeholder(
    field: &Field,
    value: &Value,
    params: &mut Vec<String>,
) -> std::result::Result<String, RecordWriteError> {
    let column_type = field.column_type().map_err(RecordWriteError::Other)?;
    match validation::encode_value(field, value).map_err(RecordWriteError::Other)? {
        // SQL NULL, typed by the column itself.
        None => Ok("null".to_string()),
        Some(encoded) => {
            params.push(encoded);
            let index = params.len();
            Ok(if column_type == "text" {
                format!("${index}::text")
            } else {
                format!("${index}::text::{column_type}")
            })
        }
    }
}

fn bound(params: &[String]) -> Vec<&(dyn ToSql + Sync)> {
    params
        .iter()
        .map(|param| param as &(dyn ToSql + Sync))
        .collect()
}

/// Turn a database constraint violation into a clean, actionable failure
/// instead of a raw driver error.
fn map_write_error(err: tokio_postgres::Error, collection: &str) -> RecordWriteError {
    if let Some(db_error) = err.as_db_error() {
        match db_error.code().code() {
            "23505" => {
                return RecordWriteError::Other(Error::conflict(format!(
                    "a record in {collection:?} with the same unique value already exists"
                )))
            }
            "23503" => {
                return RecordWriteError::Other(Error::invalid(format!(
                    "a relation in {collection:?} points to a record that does not exist"
                )))
            }
            "23502" => {
                return RecordWriteError::Other(Error::invalid(format!(
                    "a required column in {collection:?} was left empty"
                )))
            }
            _ => {}
        }
    }
    RecordWriteError::Other(db_error(err))
}
