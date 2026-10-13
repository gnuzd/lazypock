//! Reading and writing single records.
//!
//! Writes validate first ([`crate::validation`]), then bind every value as a
//! parameter, and return the stored row as a JSON document built by Postgres —
//! the same shape the read path produces, so a create and a fetch are
//! indistinguishable to a client.

use serde_json::{Map, Value};
use tokio_postgres::types::ToSql;
use tokio_postgres::Client;

use lazypock_core::error::{Error, Result};

use crate::collections::Collection;
use crate::ddl::quote_ident;
use crate::pool::db_error;
use crate::query::compile::FilterContext;
use crate::query::projection::{ExpandSpec, FieldSelection};
use crate::query::read;
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
    client: &Client,
    collection: &Collection,
    context: &FilterContext,
    input: Value,
) -> std::result::Result<Value, RecordWriteError> {
    let object = expect_object(&input)?;
    validation::validate(collection, object, false).map_err(RecordWriteError::Validation)?;

    let id = resolve_id(object)?;
    let mut columns: Vec<String> = vec!["id".to_string()];
    let mut placeholders: Vec<String> = vec!["$1::text".to_string()];
    let mut params: Vec<String> = vec![id];

    for field in &collection.fields {
        if field.field_type == FieldType::Autodate {
            continue;
        }
        let Some(value) = object.get(&field.name) else {
            continue;
        };
        columns.push(field.name.clone());
        placeholders.push(placeholder(field, value, &mut params)?);
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

    let row = client
        .query_one(&sql, &bound(&params))
        .await
        .map_err(|err| map_write_error(err, &collection.name))?;
    Ok(row.get("record"))
}

/// Update a record, returning the stored row, or `None` when it does not exist.
pub async fn update(
    client: &Client,
    collection: &Collection,
    context: &FilterContext,
    id: &str,
    input: Value,
) -> std::result::Result<Option<Value>, RecordWriteError> {
    let object = expect_object(&input)?;
    validation::validate(collection, object, true).map_err(RecordWriteError::Validation)?;

    let mut assignments: Vec<String> = Vec::new();
    let mut params: Vec<String> = Vec::new();

    for field in &collection.fields {
        if field.field_type == FieldType::Autodate {
            continue;
        }
        let Some(value) = object.get(&field.name) else {
            continue;
        };
        let placeholder = placeholder(field, value, &mut params)?;
        assignments.push(format!("{} = {placeholder}", quote_ident(&field.name)));
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

    let row = client
        .query_opt(&sql, &bound(&params))
        .await
        .map_err(|err| map_write_error(err, &collection.name))?;
    Ok(row.map(|row| row.get("record")))
}

/// Delete a record. Returns whether a row was removed.
pub async fn delete(
    client: &Client,
    collection: &Collection,
    id: &str,
) -> std::result::Result<bool, RecordWriteError> {
    let sql = format!(
        "delete from {} where \"id\" = $1::text",
        quote_ident(&collection.name)
    );
    let params = [id.to_string()];
    let removed = client
        .execute(&sql, &bound(&params))
        .await
        .map_err(|err| map_write_error(err, &collection.name))?;
    Ok(removed > 0)
}

/// Fetch one record, or `None` when it does not exist.
pub async fn get_one(
    client: &Client,
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
    let row = client
        .query_opt(&sql, &bound(&params))
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

/// Render the bind placeholder for a field's value, appending to `params`.
fn placeholder(
    field: &crate::collections::Field,
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
