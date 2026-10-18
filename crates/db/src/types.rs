//! Collection and field domain types, identifier validation and the mapping
//! from lazypock field types to Postgres column types.
//!
//! Two invariants live here, both security-relevant because these names end up
//! in DDL:
//!
//! 1. **Identifiers are strictly validated** before they are ever quoted into a
//!    statement: `^[a-z][a-z0-9_]*$`, at most 63 bytes, never `_`-prefixed
//!    (that prefix is reserved for system tables).
//! 2. **Unknown field types and unsupported option values are rejected**, never
//!    silently ignored.

use std::fmt;

use lazypock_core::error::{Error, Result};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

/// Maximum Postgres identifier length, in bytes.
pub const MAX_IDENTIFIER_LEN: usize = 63;

/// Columns present on every collection table.
pub const SYSTEM_COLUMNS: &[&str] = &["id", "created", "updated"];

/// Extra system columns on auth collections.
pub const AUTH_SYSTEM_COLUMNS: &[&str] = &[
    "email",
    "password_hash",
    "verified",
    "token_key",
    "emailVisibility",
];

/// The auth columns that are credentials.
///
/// These are the ones that must never be **written from input** (they are supplied
/// already hashed by the caller that owns hashing) and never **read back**: the map
/// a filter and a projection are built from is also what `fields=*` returns, so a
/// credential left in it is a credential returned to a client.
///
/// `email` and `verified` are deliberately *not* here — they are ordinary columns
/// that a caller sets and reads.
pub const CREDENTIAL_COLUMNS: &[&str] = &["password_hash", "token_key"];

/// The auth columns and their SQL definitions — the single source the DDL builds
/// from, so a column the field metadata advertises cannot be missing from the table.
///
/// It used to push four hardcoded columns while a separate constant listed the
/// names; adding a name to the constant produced a table without the column, and the
/// failure was \`column users.emailVisibility does not exist\`.
pub fn auth_system_column_definitions() -> Vec<(&'static str, &'static str)> {
    vec![
        ("email", "text"),
        ("password_hash", "text"),
        ("verified", "boolean not null default false"),
        ("token_key", "text"),
        ("emailVisibility", "boolean not null default false"),
    ]
}

/// The fields an auth collection has whether or not anybody declared them.
///
/// The **SQL twin** of this is \`migrations/0006_auth_system_fields.sql\`, which
/// backfilled the collections that existed before it. Keep the two in step: the
/// migration cannot see collections created later, and this cannot see ones created
/// earlier.
///
/// `token_key` is deliberately **absent**: it is a server-managed credential, not a
/// field, so it should not appear in a collection's definition — which is what the
/// Studio renders and what the SDK/codegen reads. It is still a real column
/// ([`auth_system_column_definitions`]) that the auth layer reads and writes;
/// `password_hash` stays a field because the Studio needs it to render its password
/// input. `migrations/0010_token_key_is_not_a_field.sql` removes the field rows that
/// 0006 created.
pub fn auth_system_fields() -> Vec<(&'static str, FieldType, bool, bool, serde_json::Value)> {
    vec![
        ("email", FieldType::Email, true, true, serde_json::json!({})),
        (
            "password_hash",
            FieldType::Password,
            false,
            false,
            serde_json::json!({ "hidden": true, "writeOnly": true }),
        ),
        (
            "emailVisibility",
            FieldType::Bool,
            false,
            false,
            serde_json::json!({}),
        ),
        (
            "verified",
            FieldType::Bool,
            false,
            false,
            serde_json::json!({ "hidden": true }),
        ),
    ]
}

/// The collection a relation field points at, under either name.
///
/// PocketBase stores `collectionId`; the implementation this replaced — and the
/// Studio built for it — send `collection`, carrying a collection *name*
/// (`{"collection": "users"}`). Both are accepted, and the value may be either an id or a name, which is how the
/// DDL already resolved it. Read through here rather than off `options` directly, so
/// validation and the foreign-key builder cannot end up looking in different places —
/// which is precisely how a relation field came to be refused for a missing option it
/// had, under another name.
pub fn relation_target(options: &serde_json::Value) -> Option<&str> {
    options
        .get("collectionId")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            options
                .get("collection")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.is_empty())
        })
}

/// Kind of collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CollectionType {
    /// A plain table.
    Base,
    /// A table with the auth system columns.
    Auth,
    /// A read-only Postgres view over user-written SQL.
    View,
}

impl CollectionType {
    /// Wire/database name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Auth => "auth",
            Self::View => "view",
        }
    }

    /// Parse a stored collection type, rejecting anything unknown.
    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "base" => Ok(Self::Base),
            "auth" => Ok(Self::Auth),
            "view" => Ok(Self::View),
            other => Err(Error::invalid(format!(
                "unknown collection type {other:?}; expected base, auth or view"
            ))),
        }
    }

    /// Whether this collection is backed by a table.
    pub fn is_table(self) -> bool {
        !matches!(self, Self::View)
    }

    /// Whether this collection holds login records.
    pub fn is_auth(self) -> bool {
        matches!(self, Self::Auth)
    }
}

impl fmt::Display for CollectionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for CollectionType {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CollectionType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(D::Error::custom)
    }
}

/// A lazypock field type, using PocketBase wire names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldType {
    /// Plain text.
    Text,
    /// Markdown rich text (TipTap).
    Editor,
    /// Text restricted to an email address.
    Email,
    /// Text restricted to a URL.
    Url,
    /// Number (double precision, or bigint with `onlyInt`).
    Number,
    /// Boolean.
    Bool,
    /// Timestamp.
    Date,
    /// One or many values from a fixed list.
    Select,
    /// One or many stored file names.
    File,
    /// One or many references to another collection.
    Relation,
    /// Arbitrary JSON.
    Json,
    /// `{ "lon": f64, "lat": f64 }`.
    GeoPoint,
    /// Timestamp maintained by the server.
    Autodate,
    /// Auth collection password (auth collections only).
    Password,
}

impl FieldType {
    /// Wire/database name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Editor => "editor",
            Self::Email => "email",
            Self::Url => "url",
            Self::Number => "number",
            Self::Bool => "bool",
            Self::Date => "date",
            Self::Select => "select",
            Self::File => "file",
            Self::Relation => "relation",
            Self::Json => "json",
            Self::GeoPoint => "geoPoint",
            Self::Autodate => "autodate",
            Self::Password => "password",
        }
    }

    /// Parse a stored field type, rejecting anything unknown.
    ///
    /// The documented import format (and PocketBase) spell some of these
    /// differently from the names stored here, so the aliases are accepted too:
    /// `datetime`, `multi_select`, `multi_file` and `geo`. They are synonyms, not
    /// separate types — a multi select is a `select` with `maxSelect > 1` — so this
    /// normalises them instead of carrying a second spelling through the DDL. The
    /// published JSON Schema (`/backup.schema.json`) lists the aliases, and the
    /// Studio's AI import prompt emits them, which is why refusing them made a
    /// documented payload fail with `unknown field type "datetime"`.
    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "text" => Ok(Self::Text),
            "editor" => Ok(Self::Editor),
            "email" => Ok(Self::Email),
            "url" => Ok(Self::Url),
            "number" => Ok(Self::Number),
            "bool" => Ok(Self::Bool),
            "date" | "datetime" => Ok(Self::Date),
            "select" | "multi_select" => Ok(Self::Select),
            "file" | "multi_file" => Ok(Self::File),
            "relation" => Ok(Self::Relation),
            "json" => Ok(Self::Json),
            "geoPoint" | "geo" => Ok(Self::GeoPoint),
            "autodate" => Ok(Self::Autodate),
            "password" => Ok(Self::Password),
            other => Err(Error::invalid(format!(
                "unknown field type {other:?}; supported types are text, editor, email, url, \
                 number, bool, date (or datetime), select (or multi_select), file (or \
                 multi_file), relation, json, geo (or geoPoint), autodate and password"
            ))),
        }
    }

    /// Whether this type stores multiple values.
    pub fn supports_multiple(self) -> bool {
        matches!(self, Self::Select | Self::File | Self::Relation)
    }
}

impl fmt::Display for FieldType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for FieldType {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for FieldType {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::parse(&raw).map_err(D::Error::custom)
    }
}

/// Validate a collection name or other user-supplied identifier.
///
/// Deliberately strict: lowercase snake case, starting with a letter, at most
/// [`MAX_IDENTIFIER_LEN`] bytes, and never `_`-prefixed (reserved for system
/// tables). Identifiers are still quoted when used in SQL.
pub fn validate_identifier(kind: &str, name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::invalid(format!("{kind} must not be empty")));
    }
    if name.len() > MAX_IDENTIFIER_LEN {
        return Err(Error::invalid(format!(
            "{kind} {name:?} is longer than {MAX_IDENTIFIER_LEN} bytes"
        )));
    }
    if name.starts_with('_') {
        return Err(Error::invalid(format!(
            "{kind} {name:?} must not start with \"_\" (reserved for system tables)"
        )));
    }
    let mut chars = name.chars();
    let first = chars.next().expect("checked non-empty");
    if !first.is_ascii_lowercase() {
        return Err(Error::invalid(format!(
            "{kind} {name:?} must start with a lowercase letter"
        )));
    }
    // Mixed case is allowed after the first letter, so `firstName` works — which is
    // what PocketBase and its clients use, and what the user asked for.
    //
    // The original rule was lowercase throughout, on the reasoning that Postgres
    // folds unquoted identifiers to lowercase. That reasoning holds only if column
    // references are left unquoted, and this codebase quotes every one of them —
    // `quote_ident` is applied to the table and to each column in the filter
    // compiler, the read builder and the DDL. So the constraint was buying nothing
    // and costing camelCase.
    //
    // A lowercase first letter is still required: it keeps generated names
    // predictable, and nothing in the reference semantics needs a leading capital.
    if let Some(bad) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || *c == '_'))
    {
        return Err(Error::invalid(format!(
            "{kind} {name:?} contains {bad:?}; only letters, digits and \"_\" are allowed"
        )));
    }
    Ok(())
}

/// Validate a collection name.
pub fn validate_collection_name(name: &str) -> Result<()> {
    validate_identifier("collection name", name)
}

/// Validate a field name for a collection of the given type.
pub fn validate_field_name(name: &str, collection_type: CollectionType) -> Result<()> {
    validate_identifier("field name", name)?;
    if SYSTEM_COLUMNS.contains(&name) {
        return Err(Error::invalid(format!(
            "{name:?} is a built-in field and cannot be redefined"
        )));
    }
    if collection_type == CollectionType::Auth && AUTH_SYSTEM_COLUMNS.contains(&name) {
        return Err(Error::invalid(format!(
            "{name:?} is a built-in auth field and cannot be redefined"
        )));
    }
    Ok(())
}

/// Read an option as a boolean, treating a missing value as `false`.
pub fn option_bool(options: &Value, key: &str) -> bool {
    options.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// Read an option as a number.
pub fn option_f64(options: &Value, key: &str) -> Option<f64> {
    options.get(key).and_then(Value::as_f64)
}

/// Read an option as an unsigned integer.
pub fn option_u64(options: &Value, key: &str) -> Option<u64> {
    options.get(key).and_then(Value::as_u64)
}

/// Read an option as a list of strings, defaulting to empty.
pub fn option_str_list(options: &Value, key: &str) -> Vec<String> {
    options
        .get(key)
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Whether the field holds multiple values (`maxSelect > 1`).
pub fn is_multiple(options: &Value) -> bool {
    option_u64(options, "maxSelect").unwrap_or(1) > 1
}

/// Validate the options of a field, rejecting unsupported values.
pub fn validate_options(field_type: FieldType, options: &Value) -> Result<()> {
    if !options.is_object() {
        return Err(Error::invalid(format!(
            "field options must be a JSON object, got {options}"
        )));
    }

    if field_type.supports_multiple() {
        if let Some(max_select) = option_u64(options, "maxSelect") {
            if max_select < 1 {
                return Err(Error::invalid(format!(
                    "maxSelect must be at least 1 for a {field_type} field, got {max_select}"
                )));
            }
        }
    } else if options.get("maxSelect").is_some() {
        return Err(Error::invalid(format!(
            "maxSelect is only meaningful for select, file and relation fields, not {field_type}"
        )));
    }

    let min = option_f64(options, "min");
    let max = option_f64(options, "max");
    if let (Some(min), Some(max)) = (min, max) {
        if min > max {
            return Err(Error::invalid(format!(
                "min ({min}) must not be greater than max ({max})"
            )));
        }
    }

    if field_type == FieldType::Select {
        match options.get("values") {
            Some(Value::Array(values)) => {
                if values.is_empty() {
                    return Err(Error::invalid("a select field needs at least one value"));
                }
            }
            Some(other) => {
                return Err(Error::invalid(format!(
                    "select values must be a JSON array, got {other}"
                )));
            }
            None => return Err(Error::invalid("a select field needs a `values` array")),
        }
    }

    if field_type == FieldType::Relation && relation_target(options).is_none() {
        return Err(Error::invalid(
            "a relation field needs a `collectionId` option naming its target \
             collection (PocketBase's name), or `collection` with its name",
        ));
    }

    // A configured constraint that is not enforced is worse than no constraint,
    // so refuse to store one that cannot be applied yet.
    if options.get("pattern").is_some_and(|value| !value.is_null()) {
        return Err(Error::unsupported(
            "text `pattern` validation is not implemented yet; remove the `pattern` option",
        ));
    }

    validate_file_options(field_type, options)?;
    validate_default(field_type, options)?;

    Ok(())
}

/// The size limit a file field declares, under either spelling.
///
/// PocketBase — and therefore the documented import format and the Studio's field
/// editor — call it `maxSize`, while this codebase's first spelling was
/// `maxFileSize`. Only the latter was read, so a documented `"maxSize": 5242880`
/// was stored and never enforced: a limit that reads as applied and is not. Both
/// are accepted here, and [`default_file_size_limit`] is the one place that decides
/// which value wins.
pub fn file_size_limit(options: &Value) -> Option<u64> {
    option_u64(options, "maxFileSize").or_else(|| option_u64(options, "maxSize"))
}

/// Check the options only a file field has.
///
/// These are enforced when a file is attached to a record, so they are checked
/// here for shape as well: an option that is stored but not understood would be
/// one more setting that reads as enforced and is not.
fn validate_file_options(field_type: FieldType, options: &Value) -> Result<()> {
    let is_file = field_type == FieldType::File;

    // Each of these is a no-op anywhere else, so accepting one would mean
    // accepting a setting that can never apply.
    for key in ["maxFileSize", "maxSize", "mimeTypes", "thumbs"] {
        if !is_file && options.get(key).is_some_and(|value| !value.is_null()) {
            return Err(Error::invalid(format!(
                "{key} is only meaningful for file fields, not {field_type}"
            )));
        }
    }

    if !is_file {
        return Ok(());
    }

    for key in ["maxFileSize", "maxSize"] {
        if options.get(key).is_some_and(|value| !value.is_null())
            && !option_u64(options, key).is_some_and(|size| size > 0)
        {
            return Err(Error::invalid(format!(
                "{key} must be a positive number of bytes"
            )));
        }
    }

    if let Some(value) = options.get("mimeTypes").filter(|value| !value.is_null()) {
        let Some(types) = value.as_array() else {
            return Err(Error::invalid("mimeTypes must be a JSON array of strings"));
        };
        if types.is_empty() {
            return Err(Error::invalid("mimeTypes must name at least one type"));
        }
        if !types
            .iter()
            .all(|entry| entry.as_str().is_some_and(|text| !text.trim().is_empty()))
        {
            return Err(Error::invalid(
                "mimeTypes entries must be non-empty strings",
            ));
        }
    }

    // Per-field thumbnails are not implemented. Storing the option would promise
    // thumbnails that never appear, so it is refused and points at the thing that
    // does work: an image preset is generated for every view of the file, which
    // is what per-field thumbnails were approximating anyway.
    if options.get("thumbs").is_some_and(|value| !value.is_null()) {
        return Err(Error::unsupported(
            "per-field `thumbs` is not implemented; declare an image preset instead",
        ));
    }

    Ok(())
}

/// The `default` a field declares, rendered as a SQL literal.
///
/// `default` is part of the documented import format ("Default value applied when
/// the field is omitted on write") and of the published JSON Schema, but nothing
/// read it: an import that set `"default": true` created a column with no default
/// at all. It is rendered here, once, so the DDL and its validation cannot disagree,
/// and every value is either typed correctly for the column or refused.
///
/// Returns `None` when the field declares no default (or `null`, which is treated as
/// "no default" — a nullable column is already the default).
pub fn default_sql(field_type: FieldType, options: &Value) -> Result<Option<String>> {
    // A reflected column's default is the SQL text Postgres reported (`"0"`,
    // `nextval('…')`), not a JSON value this server could apply. It belongs to the
    // database, so it is left there rather than validated or re-declared.
    if option_bool(options, "reflected") {
        return Ok(None);
    }

    let Some(value) = options.get("default").filter(|value| !value.is_null()) else {
        return Ok(None);
    };

    let literal = match field_type {
        FieldType::Select if is_multiple(options) => {
            return Err(Error::unsupported(
                "a multiple-value select cannot declare a default; set the values on write",
            ))
        }
        FieldType::Select => {
            let text = value.as_str().ok_or_else(|| {
                Error::invalid(format!(
                    "a select field's default must be a string, got {value}"
                ))
            })?;
            let allowed = option_str_list(options, "values");
            if !allowed.iter().any(|allowed| allowed == text) {
                return Err(Error::invalid(format!(
                    "the default {text:?} is not one of the select values {allowed:?}"
                )));
            }
            format!("{}::text", sql_literal(text))
        }
        FieldType::Text
        | FieldType::Editor
        | FieldType::Email
        | FieldType::Url
        | FieldType::Password => {
            let text = value.as_str().ok_or_else(|| {
                Error::invalid(format!(
                    "a {} field's default must be a string, got {value}",
                    field_type
                ))
            })?;
            format!("{}::text", sql_literal(text))
        }
        FieldType::Number => {
            let number = value
                .as_f64()
                .filter(|value| value.is_finite())
                .ok_or_else(|| {
                    Error::invalid(format!(
                        "a number field's default must be a finite number, got {value}"
                    ))
                })?;
            if option_bool(options, "onlyInt") && number.fract() != 0.0 {
                return Err(Error::invalid(format!(
                    "a whole-number field's default must not have a fraction, got {number}"
                )));
            }
            let cast = if option_bool(options, "onlyInt") {
                "bigint"
            } else {
                "double precision"
            };
            format!("{number}::{cast}")
        }
        FieldType::Bool => {
            let flag = value.as_bool().ok_or_else(|| {
                Error::invalid(format!(
                    "a bool field's default must be true or false, got {value}"
                ))
            })?;
            format!("{flag}::boolean")
        }
        FieldType::Date | FieldType::Autodate => {
            let text = value.as_str().ok_or_else(|| {
                Error::invalid(format!(
                    "a {} field's default must be a date string, got {value}",
                    field_type
                ))
            })?;
            format!("{}::timestamptz", sql_literal(text))
        }
        FieldType::Json | FieldType::GeoPoint => {
            format!("{}::jsonb", sql_literal(&value.to_string()))
        }
        FieldType::File => format!(
            "{}::text",
            sql_literal(value.as_str().ok_or_else(|| {
                Error::invalid(format!(
                    "a file field's default must be a string, got {value}"
                ))
            })?)
        ),
        FieldType::Relation => {
            return Err(Error::unsupported(
                "a relation field cannot declare a default; write the record id on insert",
            ))
        }
    };

    Ok(Some(literal))
}

fn validate_default(field_type: FieldType, options: &Value) -> Result<()> {
    default_sql(field_type, options).map(|_| ())
}

/// A SQL string literal.
///
/// Standard-conforming strings are assumed, so only the quote is doubled. The value
/// never reaches the statement as anything but a literal: a default is user input.
fn sql_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// Postgres column type for a field.
pub fn column_type(field_type: FieldType, options: &Value) -> Result<&'static str> {
    validate_options(field_type, options)?;
    Ok(match field_type {
        FieldType::Text
        | FieldType::Editor
        | FieldType::Email
        | FieldType::Url
        | FieldType::Password => "text",
        FieldType::Number if option_bool(options, "onlyInt") => "bigint",
        FieldType::Number => "double precision",
        FieldType::Bool => "boolean",
        FieldType::Date | FieldType::Autodate => "timestamptz",
        FieldType::Select if is_multiple(options) => "text[]",
        FieldType::Select => "text",
        FieldType::File if is_multiple(options) => "jsonb",
        FieldType::File => "text",
        FieldType::Relation if is_multiple(options) => "text[]",
        FieldType::Relation => "text",
        FieldType::Json | FieldType::GeoPoint => "jsonb",
    })
}

/// A backend-neutral zero value used to backfill a new `not null` column on a
/// table that already has rows.
///
/// Returns `None` when no sensible value exists (a required relation with no
/// target), so the caller can fail loudly instead of inventing data.
pub fn zero_default(field_type: FieldType, options: &Value) -> Option<&'static str> {
    Some(match field_type {
        FieldType::Text
        | FieldType::Editor
        | FieldType::Email
        | FieldType::Url
        | FieldType::Password => "''",
        FieldType::Number => "0",
        FieldType::Bool => "false",
        FieldType::Date | FieldType::Autodate => "now()",
        FieldType::Select if is_multiple(options) => "'{}'::text[]",
        FieldType::Select => "''",
        FieldType::File if is_multiple(options) => "'[]'::jsonb",
        FieldType::File => "''",
        FieldType::Json | FieldType::GeoPoint => "'{}'::jsonb",
        FieldType::Relation if is_multiple(options) => "'{}'::text[]",
        FieldType::Relation => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn field_types_round_trip_through_their_wire_names() {
        let all = [
            FieldType::Text,
            FieldType::Editor,
            FieldType::Email,
            FieldType::Url,
            FieldType::Number,
            FieldType::Bool,
            FieldType::Date,
            FieldType::Select,
            FieldType::File,
            FieldType::Relation,
            FieldType::Json,
            FieldType::GeoPoint,
            FieldType::Autodate,
            FieldType::Password,
        ];
        for field_type in all {
            assert_eq!(FieldType::parse(field_type.as_str()).unwrap(), field_type);
            let json = serde_json::to_string(&field_type).unwrap();
            assert_eq!(json, format!("\"{}\"", field_type.as_str()));
            let back: FieldType = serde_json::from_str(&json).unwrap();
            assert_eq!(back, field_type);
        }
    }

    #[test]
    fn unknown_field_types_and_collection_types_are_rejected() {
        let err = FieldType::parse("decimal").unwrap_err();
        assert!(err.message().contains("decimal"));
        let err = CollectionType::parse("widget").unwrap_err();
        assert!(err.message().contains("widget"));
        assert!(serde_json::from_str::<FieldType>("\"decimal\"").is_err());
    }

    #[test]
    fn collection_types_round_trip() {
        for value in [
            CollectionType::Base,
            CollectionType::Auth,
            CollectionType::View,
        ] {
            assert_eq!(CollectionType::parse(value.as_str()).unwrap(), value);
            assert_eq!(
                serde_json::to_string(&value).unwrap(),
                format!("\"{}\"", value.as_str())
            );
        }
    }

    #[test]
    fn identifiers_must_be_lowercase_snake_case() {
        for good in ["posts", "blog_posts", "posts2", "a"] {
            validate_collection_name(good).unwrap_or_else(|e| panic!("{good} rejected: {e}"));
        }
        for bad in [
            "",
            "_posts",
            "Posts",
            "2posts",
            "post-list",
            "post list",
            "posts'",
            "posts\"",
            "posts;drop",
            "pôsts",
        ] {
            assert!(
                validate_collection_name(bad).is_err(),
                "{bad:?} should have been rejected"
            );
        }
    }

    #[test]
    fn identifiers_are_length_capped() {
        let ok = "a".repeat(MAX_IDENTIFIER_LEN);
        validate_collection_name(&ok).unwrap();
        let too_long = "a".repeat(MAX_IDENTIFIER_LEN + 1);
        assert!(validate_collection_name(&too_long).is_err());
    }

    #[test]
    fn system_columns_cannot_be_redefined() {
        for reserved in SYSTEM_COLUMNS {
            assert!(
                validate_field_name(reserved, CollectionType::Base).is_err(),
                "{reserved} should be reserved"
            );
        }
        assert!(validate_field_name("email", CollectionType::Auth).is_err());
        // ... but a plain collection may have its own `email` field.
        validate_field_name("email", CollectionType::Base).unwrap();
    }

    #[test]
    fn column_types_match_the_documented_mapping() {
        let cases: &[(FieldType, Value, &str)] = &[
            (FieldType::Text, json!({}), "text"),
            (FieldType::Editor, json!({}), "text"),
            (FieldType::Email, json!({}), "text"),
            (FieldType::Url, json!({}), "text"),
            (FieldType::Password, json!({}), "text"),
            (FieldType::Number, json!({}), "double precision"),
            (FieldType::Number, json!({"onlyInt": true}), "bigint"),
            (FieldType::Bool, json!({}), "boolean"),
            (FieldType::Date, json!({}), "timestamptz"),
            (FieldType::Autodate, json!({}), "timestamptz"),
            (FieldType::Select, json!({"values": ["a"]}), "text"),
            (
                FieldType::Select,
                json!({"values": ["a"], "maxSelect": 3}),
                "text[]",
            ),
            (FieldType::File, json!({}), "text"),
            (FieldType::File, json!({"maxSelect": 2}), "jsonb"),
            (
                FieldType::Relation,
                json!({"collectionId": "users"}),
                "text",
            ),
            (
                FieldType::Relation,
                json!({"collectionId": "users", "maxSelect": 5}),
                "text[]",
            ),
            (FieldType::Json, json!({}), "jsonb"),
            (FieldType::GeoPoint, json!({}), "jsonb"),
        ];
        for (field_type, options, expected) in cases {
            assert_eq!(
                column_type(*field_type, options).unwrap(),
                *expected,
                "{field_type} with {options}"
            );
        }
    }

    #[test]
    fn unsupported_option_values_are_rejected() {
        let cases: &[(FieldType, Value)] = &[
            (FieldType::Select, json!({"values": ["a"], "maxSelect": 0})),
            (FieldType::File, json!({"maxSelect": 0})),
            (FieldType::Text, json!({"maxSelect": 3})),
            (FieldType::Text, json!({"min": 5, "max": 1})),
            (FieldType::Select, json!({})),
            (FieldType::Select, json!({"values": []})),
            (FieldType::Select, json!({"values": "a"})),
            (FieldType::Relation, json!({})),
            (FieldType::Text, json!("not an object")),
        ];
        for (field_type, options) in cases {
            assert!(
                validate_options(*field_type, options).is_err(),
                "{field_type} accepted {options}"
            );
        }
    }

    #[test]
    fn zero_defaults_are_provided_except_for_required_relations() {
        assert_eq!(zero_default(FieldType::Text, &json!({})), Some("''"));
        assert_eq!(zero_default(FieldType::Bool, &json!({})), Some("false"));
        assert_eq!(
            zero_default(FieldType::Select, &json!({"values": ["a"], "maxSelect": 2})),
            Some("'{}'::text[]")
        );
        assert_eq!(zero_default(FieldType::Relation, &json!({})), None);
    }

    /// A file field's limits are enforced when a file is attached, and they are
    /// checked for shape here so a limit that could never be applied is not stored
    /// in the first place.
    #[test]
    fn file_options_are_checked_where_they_are_set() {
        let file = FieldType::File;

        assert!(validate_options(file, &json!({})).is_ok());
        assert!(validate_options(file, &json!({ "maxFileSize": 1024 })).is_ok());
        assert!(validate_options(file, &json!({ "mimeTypes": ["image/png", "image/*"] })).is_ok());

        for options in [
            json!({ "maxFileSize": 0 }),
            json!({ "maxFileSize": -1 }),
            json!({ "maxFileSize": "1kb" }),
            json!({ "mimeTypes": [] }),
            json!({ "mimeTypes": "image/png" }),
            json!({ "mimeTypes": [""] }),
            json!({ "mimeTypes": [7] }),
        ] {
            assert!(
                validate_options(file, &options).is_err(),
                "a file field accepted {options}"
            );
        }
    }

    /// Per-field thumbnails are not implemented, so the option is refused rather
    /// than stored and ignored. Storing it would promise thumbnails that never
    /// appear.
    #[test]
    fn per_field_thumbs_are_refused_rather_than_stored() {
        let error = validate_options(FieldType::File, &json!({ "thumbs": ["100x100"] }))
            .expect_err("thumbs is not implemented, so it must be refused");
        assert!(
            error.message().contains("image preset"),
            "the refusal should point at what does work, got {}",
            error.message()
        );
    }

    /// Each of these is meaningless off a file field, so accepting one would mean
    /// accepting a setting that can never apply.
    #[test]
    fn file_options_are_refused_on_other_field_types() {
        for options in [
            json!({ "maxFileSize": 1024 }),
            json!({ "mimeTypes": ["image/png"] }),
            json!({ "thumbs": ["100x100"] }),
        ] {
            assert!(
                validate_options(FieldType::Text, &options).is_err(),
                "a text field accepted {options}"
            );
        }
    }
}
