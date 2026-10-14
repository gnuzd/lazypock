//! Validating incoming record values against a collection's fields, and
//! encoding them for binding as parameters.
//!
//! Nothing reaches SQL before it passes through here, and every rejection is
//! reported per field so the HTTP layer can render PocketBase's error `data`
//! shape: `{ "field": { "code": "...", "message": "..." } }`.

use std::collections::BTreeMap;

use lazypock_core::error::{Error, Result};
use serde_json::{Map, Value};

use crate::collections::{Collection, Field};
use crate::types::{self, CollectionType, FieldType};

/// Keys a client may send that the server owns and ignores on write.
pub const SERVER_MANAGED_KEYS: &[&str] = &["created", "updated", "collectionId", "collectionName"];

/// Keys an **auth** collection accepts that are columns rather than
/// user-defined fields.
///
/// `password` is deliberately not a column: it is hashed into `password_hash` by
/// the caller, which owns the hashing. `password_hash` and `token_key` are never
/// accepted from a client.
pub const AUTH_INPUT_KEYS: &[&str] = &["email", "verified", "password", "passwordConfirm"];

/// Shortest password accepted for an auth-collection account.
pub const MIN_PASSWORD_LEN: usize = 8;

/// One field's validation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldError {
    /// Stable machine-readable code.
    pub code: &'static str,
    /// Human-readable message.
    pub message: String,
}

impl FieldError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

/// Validation failures keyed by field name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ValidationErrors {
    /// Failures, ordered by field name for a stable response.
    pub fields: BTreeMap<String, FieldError>,
}

impl ValidationErrors {
    /// Whether anything failed.
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Record a failure for `field`, keeping the first one per field.
    pub fn insert(&mut self, field: &str, code: &'static str, message: impl Into<String>) {
        self.fields
            .entry(field.to_string())
            .or_insert_with(|| FieldError::new(code, message));
    }

    /// The PocketBase `data` shape.
    pub fn to_json(&self) -> Value {
        let mut out = Map::new();
        for (field, error) in &self.fields {
            out.insert(
                field.clone(),
                serde_json::json!({ "code": error.code, "message": error.message }),
            );
        }
        Value::Object(out)
    }

    /// A one-line summary, for logs.
    pub fn summary(&self) -> String {
        self.fields
            .iter()
            .map(|(field, error)| format!("{field}: {}", error.message))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// Validate an incoming record.
///
/// `partial` is set for updates, where an absent field means "leave it alone"
/// rather than "use the default".
pub fn validate(
    collection: &Collection,
    input: &Map<String, Value>,
    partial: bool,
) -> std::result::Result<(), ValidationErrors> {
    let mut errors = ValidationErrors::default();

    let is_auth = collection.collection_type == CollectionType::Auth;

    for key in input.keys() {
        if key == "id" || SERVER_MANAGED_KEYS.contains(&key.as_str()) {
            continue;
        }
        // An auth collection has writable columns that are not user fields.
        if is_auth && AUTH_INPUT_KEYS.contains(&key.as_str()) {
            continue;
        }
        if collection.field(key).is_none() {
            errors.insert(
                key,
                "validation_unknown_field",
                format!("Unknown field {key:?}."),
            );
        }
    }

    for field in &collection.fields {
        // Autodate values are owned by the server; input is ignored.
        if field.field_type == FieldType::Autodate {
            continue;
        }

        match input.get(&field.name) {
            None => {
                if field.required && !partial {
                    errors.insert(
                        &field.name,
                        "validation_required",
                        format!("{} is required.", field.name),
                    );
                }
            }
            Some(Value::Null) => {
                if field.required {
                    errors.insert(
                        &field.name,
                        "validation_required",
                        format!("{} is required.", field.name),
                    );
                }
            }
            Some(value) => {
                // An empty string counts as missing for a required text field.
                if field.required && is_blank_text(field, value) {
                    errors.insert(
                        &field.name,
                        "validation_required",
                        format!("{} is required.", field.name),
                    );
                    continue;
                }
                if let Err(error) = validate_value(field, value) {
                    errors.insert(&field.name, error.code, error.message);
                }
            }
        }
    }

    if is_auth {
        validate_auth_keys(input, partial, &mut errors);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Validate the built-in keys of an auth collection.
///
/// These are columns rather than user-defined fields, so the field loop above
/// never sees them.
fn validate_auth_keys(input: &Map<String, Value>, partial: bool, errors: &mut ValidationErrors) {
    match input.get("email") {
        None if partial => {}
        None | Some(Value::Null) => {
            errors.insert("email", "validation_required", "An email is required.");
        }
        Some(value) => match value.as_str() {
            Some(text) if looks_like_email(text) => {}
            Some(_) => {
                errors.insert(
                    "email",
                    "validation_invalid_email",
                    "The email must be a valid address.",
                );
            }
            None => {
                errors.insert(
                    "email",
                    "validation_invalid_value",
                    "The email must be a string.",
                );
            }
        },
    }

    if let Some(value) = input.get("verified") {
        if !value.is_null() && parse_bool(value).is_none() {
            errors.insert(
                "verified",
                "validation_invalid_value",
                "verified must be a boolean.",
            );
        }
    }

    let password = input.get("password");
    match password {
        // Omitting the password on an update leaves it alone.
        None if partial => {}
        None | Some(Value::Null) => {
            errors.insert(
                "password",
                "validation_required",
                "A password is required when creating an account.",
            );
        }
        Some(value) => match value.as_str() {
            Some(text) if text.chars().count() >= MIN_PASSWORD_LEN => {}
            Some(_) => {
                errors.insert(
                    "password",
                    "validation_min_text",
                    format!("The password must be at least {MIN_PASSWORD_LEN} characters."),
                );
            }
            None => {
                errors.insert(
                    "password",
                    "validation_invalid_value",
                    "The password must be a string.",
                );
            }
        },
    }

    // PocketBase's confirmation field: when it is sent, it must match.
    if let Some(Value::String(confirmation)) = input.get("passwordConfirm") {
        if password.and_then(Value::as_str) != Some(confirmation.as_str()) {
            errors.insert(
                "passwordConfirm",
                "validation_mismatch",
                "The passwords do not match.",
            );
        }
    }
}

fn is_blank_text(field: &Field, value: &Value) -> bool {
    is_text_like(field.field_type) && value.as_str().is_some_and(|text| text.trim().is_empty())
}

fn is_text_like(field_type: FieldType) -> bool {
    matches!(
        field_type,
        FieldType::Text
            | FieldType::Editor
            | FieldType::Email
            | FieldType::Url
            | FieldType::Password
    )
}

fn validate_value(field: &Field, value: &Value) -> std::result::Result<(), FieldError> {
    let name = &field.name;

    match field.field_type {
        FieldType::Text | FieldType::Editor => {
            let text = expect_string(name, value)?;
            check_length(field, name, text)
        }
        FieldType::Email => {
            let text = expect_string(name, value)?;
            check_length(field, name, text)?;
            if !looks_like_email(text) {
                return Err(FieldError::new(
                    "validation_invalid_email",
                    format!("{name} must be a valid email address."),
                ));
            }
            Ok(())
        }
        FieldType::Url => {
            let text = expect_string(name, value)?;
            check_length(field, name, text)?;
            if !looks_like_url(text) {
                return Err(FieldError::new(
                    "validation_invalid_url",
                    format!("{name} must start with http:// or https://."),
                ));
            }
            Ok(())
        }
        FieldType::Password => {
            let text = expect_string(name, value)?;
            check_length(field, name, text)
        }
        FieldType::Number => validate_number(field, value),
        FieldType::Bool => parse_bool(value).map(|_| ()).ok_or_else(|| {
            FieldError::new(
                "validation_invalid_value",
                format!("{name} must be a boolean."),
            )
        }),
        FieldType::Date => {
            let text = expect_string(name, value)?;
            if !looks_like_date(text) {
                return Err(FieldError::new(
                    "validation_invalid_date",
                    format!("{name} must be a date, for example 2026-01-02T15:04:05Z."),
                ));
            }
            Ok(())
        }
        FieldType::Select => {
            let allowed = types::option_str_list(&field.options, "values");
            if field.is_multiple() {
                let chosen = expect_string_list(field, value)?;
                check_max_select(field, chosen.len())?;
                for item in &chosen {
                    check_allowed(name, item, &allowed)?;
                }
                Ok(())
            } else {
                let chosen = expect_string(name, value)?;
                check_allowed(name, chosen, &allowed)
            }
        }
        FieldType::File => {
            if field.is_multiple() {
                let names = expect_string_list(field, value)?;
                check_max_select(field, names.len())
            } else {
                expect_string(name, value).map(|_| ())
            }
        }
        FieldType::Relation => {
            if field.is_multiple() {
                let ids = expect_string_list(field, value)?;
                check_max_select(field, ids.len())?;
                for id in &ids {
                    if !lazypock_core::id::is_valid_id(id) {
                        return Err(FieldError::new(
                            "validation_invalid_relation",
                            format!("{name} contains an invalid record id."),
                        ));
                    }
                }
                Ok(())
            } else {
                let id = expect_string(name, value)?;
                if !lazypock_core::id::is_valid_id(id) {
                    return Err(FieldError::new(
                        "validation_invalid_relation",
                        format!("{name} must be a record id."),
                    ));
                }
                Ok(())
            }
        }
        FieldType::Json => Ok(()),
        FieldType::GeoPoint => {
            let object = value.as_object().ok_or_else(|| {
                FieldError::new(
                    "validation_invalid_value",
                    format!("{name} must be an object with `lon` and `lat`."),
                )
            })?;
            let lon = object.get("lon").and_then(Value::as_f64);
            let lat = object.get("lat").and_then(Value::as_f64);
            match (lon, lat) {
                (Some(lon), Some(lat))
                    if (-180.0..=180.0).contains(&lon) && (-90.0..=90.0).contains(&lat) =>
                {
                    Ok(())
                }
                _ => Err(FieldError::new(
                    "validation_invalid_geo_point",
                    format!("{name} must have a `lon` within ±180 and a `lat` within ±90."),
                )),
            }
        }
        FieldType::Autodate => Ok(()),
    }
}

fn expect_string<'a>(name: &str, value: &'a Value) -> std::result::Result<&'a str, FieldError> {
    value.as_str().ok_or_else(|| {
        FieldError::new(
            "validation_invalid_value",
            format!("{name} must be a string."),
        )
    })
}

fn expect_string_list(
    field: &Field,
    value: &Value,
) -> std::result::Result<Vec<String>, FieldError> {
    let name = &field.name;
    let items = value.as_array().ok_or_else(|| {
        FieldError::new(
            "validation_invalid_value",
            format!("{name} must be an array."),
        )
    })?;
    items
        .iter()
        .map(|item| {
            item.as_str().map(str::to_string).ok_or_else(|| {
                FieldError::new(
                    "validation_invalid_value",
                    format!("{name} must contain only strings."),
                )
            })
        })
        .collect()
}

fn check_length(field: &Field, name: &str, text: &str) -> std::result::Result<(), FieldError> {
    let length = text.chars().count() as f64;
    if let Some(min) = types::option_f64(&field.options, "min") {
        if length < min {
            return Err(FieldError::new(
                "validation_min_text",
                format!("{name} must be at least {min} characters."),
            ));
        }
    }
    if let Some(max) = types::option_f64(&field.options, "max") {
        if length > max {
            return Err(FieldError::new(
                "validation_max_text",
                format!("{name} must be at most {max} characters."),
            ));
        }
    }
    Ok(())
}

fn validate_number(field: &Field, value: &Value) -> std::result::Result<(), FieldError> {
    let name = &field.name;
    let number = parse_number(value).ok_or_else(|| {
        FieldError::new(
            "validation_not_a_number",
            format!("{name} must be a number."),
        )
    })?;

    if types::option_bool(&field.options, "onlyInt") && number.fract() != 0.0 {
        return Err(FieldError::new(
            "validation_not_an_integer",
            format!("{name} must be a whole number."),
        ));
    }
    if let Some(min) = types::option_f64(&field.options, "min") {
        if number < min {
            return Err(FieldError::new(
                "validation_min_number",
                format!("{name} must be at least {min}."),
            ));
        }
    }
    if let Some(max) = types::option_f64(&field.options, "max") {
        if number > max {
            return Err(FieldError::new(
                "validation_max_number",
                format!("{name} must be at most {max}."),
            ));
        }
    }
    Ok(())
}

fn check_max_select(field: &Field, count: usize) -> std::result::Result<(), FieldError> {
    let limit = types::option_u64(&field.options, "maxSelect").unwrap_or(1) as usize;
    if count > limit {
        return Err(FieldError::new(
            "validation_too_many_values",
            format!("{} accepts at most {limit} value(s).", field.name),
        ));
    }
    Ok(())
}

fn check_allowed(
    name: &str,
    chosen: &str,
    allowed: &[String],
) -> std::result::Result<(), FieldError> {
    if allowed.iter().any(|candidate| candidate == chosen) {
        return Ok(());
    }
    Err(FieldError::new(
        "validation_not_in_values",
        format!("{name} must be one of: {}.", allowed.join(", ")),
    ))
}

/// Parse a JSON number, accepting a numeric string as well.
fn parse_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// Parse a boolean, accepting the usual string spellings.
fn parse_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(flag) => Some(*flag),
        Value::String(text) => match text.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => Some(true),
            "false" | "0" | "no" | "off" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

/// A deliberately lenient structural email check: one `@`, something either
/// side, and a dot in the domain.
fn looks_like_email(value: &str) -> bool {
    if value.len() > 320 || value.chars().any(char::is_whitespace) {
        return false;
    }
    let mut parts = value.split('@');
    let (Some(local), Some(domain), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !local.is_empty()
        && domain
            .split_once('.')
            .is_some_and(|(host, tld)| !host.is_empty() && !tld.is_empty())
}

fn looks_like_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

/// `YYYY-MM-DD`, optionally with a time part.
fn looks_like_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 10 {
        return false;
    }
    let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
    digits(0..4) && bytes[4] == b'-' && digits(5..7) && bytes[7] == b'-' && digits(8..10)
}

/// Encode a validated value for binding as a text parameter.
///
/// Returns `Ok(None)` for SQL NULL.
pub fn encode_value(field: &Field, value: &Value) -> Result<Option<String>> {
    if value.is_null() {
        return Ok(None);
    }

    let encoded = match field.field_type {
        FieldType::Text
        | FieldType::Editor
        | FieldType::Email
        | FieldType::Url
        | FieldType::Password
        | FieldType::Date => require_str(field, value)?.to_string(),
        FieldType::Number => parse_number(value)
            .ok_or_else(|| Error::invalid(format!("{} must be a number", field.name)))?
            .to_string(),
        FieldType::Bool => parse_bool(value)
            .ok_or_else(|| Error::invalid(format!("{} must be a boolean", field.name)))?
            .to_string(),
        FieldType::Select | FieldType::Relation => {
            if field.is_multiple() {
                pg_text_array(&require_str_list(field, value)?)
            } else {
                require_str(field, value)?.to_string()
            }
        }
        FieldType::File => {
            if field.is_multiple() {
                serde_json::to_string(value).map_err(|err| {
                    Error::invalid(format!("{} is not encodable: {err}", field.name))
                })?
            } else {
                require_str(field, value)?.to_string()
            }
        }
        FieldType::Json | FieldType::GeoPoint => serde_json::to_string(value)
            .map_err(|err| Error::invalid(format!("{} is not encodable: {err}", field.name)))?,
        FieldType::Autodate => return Ok(None),
    };

    Ok(Some(encoded))
}

fn require_str<'a>(field: &Field, value: &'a Value) -> Result<&'a str> {
    value
        .as_str()
        .ok_or_else(|| Error::invalid(format!("{} must be a string", field.name)))
}

fn require_str_list(field: &Field, value: &Value) -> Result<Vec<String>> {
    let items = value
        .as_array()
        .ok_or_else(|| Error::invalid(format!("{} must be an array", field.name)))?;
    items
        .iter()
        .map(|item| {
            item.as_str()
                .map(str::to_string)
                .ok_or_else(|| Error::invalid(format!("{} must contain only strings", field.name)))
        })
        .collect()
}

/// Render a `text[]` literal.
///
/// The value is still bound as a parameter; this quotes and escapes inside that
/// parameter so the server parses it as an array of literals.
fn pg_text_array(values: &[String]) -> String {
    let mut out = String::with_capacity(values.len() * 4 + 2);
    out.push('{');
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('"');
        for character in value.chars() {
            if matches!(character, '\\' | '"') {
                out.push('\\');
            }
            out.push(character);
        }
        out.push('"');
    }
    out.push('}');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CollectionType;
    use serde_json::json;
    use std::time::SystemTime;

    fn field(name: &str, field_type: FieldType, options: Value, required: bool) -> Field {
        Field {
            id: format!("f_{name}"),
            collection_id: "c1".into(),
            name: name.into(),
            field_type,
            required,
            is_unique: false,
            options,
            position: 0,
        }
    }

    fn collection(fields: Vec<Field>) -> Collection {
        Collection {
            id: "c1".into(),
            name: "posts".into(),
            collection_type: CollectionType::Base,
            options: json!({}),
            rules: json!({}),
            fields,
            created: SystemTime::now(),
            updated: SystemTime::now(),
        }
    }

    fn input(value: Value) -> Map<String, Value> {
        value.as_object().expect("object").clone()
    }

    fn errors(collection: &Collection, value: Value, partial: bool) -> ValidationErrors {
        validate(collection, &input(value), partial).expect_err("should have failed")
    }

    fn ok(collection: &Collection, value: Value, partial: bool) {
        if let Err(errors) = validate(collection, &input(value), partial) {
            panic!("unexpected validation failure: {}", errors.summary());
        }
    }

    #[test]
    fn required_fields_must_be_present_and_non_empty() {
        let collection = collection(vec![field("title", FieldType::Text, json!({}), true)]);

        let failures = errors(&collection, json!({}), false);
        assert_eq!(failures.fields["title"].code, "validation_required");

        let failures = errors(&collection, json!({"title": null}), false);
        assert_eq!(failures.fields["title"].code, "validation_required");

        // Whitespace counts as missing for a required text field.
        let failures = errors(&collection, json!({"title": "   "}), false);
        assert_eq!(failures.fields["title"].code, "validation_required");

        // On a partial update an absent field means "leave it alone".
        ok(&collection, json!({}), true);
        // ... but an explicit null is still an attempt to clear it.
        assert!(validate(&collection, &input(json!({"title": null})), true).is_err());
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let collection = collection(vec![field("title", FieldType::Text, json!({}), false)]);
        let failures = errors(&collection, json!({"titel": "typo"}), false);
        assert_eq!(failures.fields["titel"].code, "validation_unknown_field");

        // System keys are accepted and ignored.
        ok(
            &collection,
            json!({"title": "a", "created": "x", "updated": "x", "collectionId": "c", "collectionName": "posts"}),
            false,
        );
    }

    #[test]
    fn text_length_bounds_are_enforced() {
        let collection = collection(vec![field(
            "title",
            FieldType::Text,
            json!({"min": 3, "max": 5}),
            false,
        )]);

        ok(&collection, json!({"title": "abc"}), false);
        ok(&collection, json!({"title": "abcde"}), false);

        let failures = errors(&collection, json!({"title": "ab"}), false);
        assert_eq!(failures.fields["title"].code, "validation_min_text");
        let failures = errors(&collection, json!({"title": "abcdef"}), false);
        assert_eq!(failures.fields["title"].code, "validation_max_text");
        let failures = errors(&collection, json!({"title": 5}), false);
        assert_eq!(failures.fields["title"].code, "validation_invalid_value");
    }

    #[test]
    fn emails_and_urls_are_checked_structurally() {
        let collection = collection(vec![
            field("email", FieldType::Email, json!({}), false),
            field("link", FieldType::Url, json!({}), false),
        ]);

        ok(
            &collection,
            json!({"email": "a@example.com", "link": "https://example.com"}),
            false,
        );

        for bad in ["nope", "a@b", "@example.com", "a b@example.com", "a@@b.com"] {
            let failures = errors(&collection, json!({"email": bad}), false);
            assert_eq!(
                failures.fields["email"].code, "validation_invalid_email",
                "{bad:?}"
            );
        }
        for bad in ["example.com", "ftp://example.com", "/relative"] {
            let failures = errors(&collection, json!({"link": bad}), false);
            assert_eq!(
                failures.fields["link"].code, "validation_invalid_url",
                "{bad:?}"
            );
        }
    }

    #[test]
    fn numbers_are_range_and_integrality_checked() {
        let collection = collection(vec![
            field(
                "views",
                FieldType::Number,
                json!({"onlyInt": true, "min": 0, "max": 100}),
                false,
            ),
            field("ratio", FieldType::Number, json!({}), false),
        ]);

        ok(&collection, json!({"views": 10, "ratio": 1.5}), false);
        // A numeric string is accepted (multipart forms send strings).
        ok(&collection, json!({"views": "10"}), false);

        let failures = errors(&collection, json!({"views": 1.5}), false);
        assert_eq!(failures.fields["views"].code, "validation_not_an_integer");
        let failures = errors(&collection, json!({"views": -1}), false);
        assert_eq!(failures.fields["views"].code, "validation_min_number");
        let failures = errors(&collection, json!({"views": 101}), false);
        assert_eq!(failures.fields["views"].code, "validation_max_number");
        let failures = errors(&collection, json!({"views": "many"}), false);
        assert_eq!(failures.fields["views"].code, "validation_not_a_number");
    }

    #[test]
    fn booleans_dates_and_geo_points_are_checked() {
        let collection = collection(vec![
            field("published", FieldType::Bool, json!({}), false),
            field("when", FieldType::Date, json!({}), false),
            field("where", FieldType::GeoPoint, json!({}), false),
        ]);

        ok(
            &collection,
            json!({"published": true, "when": "2026-01-02T15:04:05Z", "where": {"lon": 1.0, "lat": 2.0}}),
            false,
        );
        ok(
            &collection,
            json!({"published": "false", "when": "2026-01-02"}),
            false,
        );

        let failures = errors(&collection, json!({"published": "maybe"}), false);
        assert_eq!(
            failures.fields["published"].code,
            "validation_invalid_value"
        );
        let failures = errors(&collection, json!({"when": "next tuesday"}), false);
        assert_eq!(failures.fields["when"].code, "validation_invalid_date");
        let failures = errors(
            &collection,
            json!({"where": {"lon": 200.0, "lat": 0.0}}),
            false,
        );
        assert_eq!(
            failures.fields["where"].code,
            "validation_invalid_geo_point"
        );
        let failures = errors(&collection, json!({"where": "nowhere"}), false);
        assert_eq!(failures.fields["where"].code, "validation_invalid_value");
    }

    #[test]
    fn selects_are_restricted_to_their_values() {
        let collection = collection(vec![
            field(
                "status",
                FieldType::Select,
                json!({"values": ["draft", "published"], "maxSelect": 1}),
                false,
            ),
            field(
                "tags",
                FieldType::Select,
                json!({"values": ["a", "b", "c"], "maxSelect": 2}),
                false,
            ),
        ]);

        ok(
            &collection,
            json!({"status": "draft", "tags": ["a", "b"]}),
            false,
        );

        let failures = errors(&collection, json!({"status": "archived"}), false);
        assert_eq!(failures.fields["status"].code, "validation_not_in_values");
        let failures = errors(&collection, json!({"tags": ["a", "b", "c"]}), false);
        assert_eq!(failures.fields["tags"].code, "validation_too_many_values");
        let failures = errors(&collection, json!({"tags": ["a", "z"]}), false);
        assert_eq!(failures.fields["tags"].code, "validation_not_in_values");
        let failures = errors(&collection, json!({"tags": "a"}), false);
        assert_eq!(failures.fields["tags"].code, "validation_invalid_value");
    }

    #[test]
    fn relations_require_real_record_ids() {
        let collection = collection(vec![
            field(
                "author",
                FieldType::Relation,
                json!({"collectionId": "c_users"}),
                false,
            ),
            field(
                "reviewers",
                FieldType::Relation,
                json!({"collectionId": "c_users", "maxSelect": 2}),
                false,
            ),
        ]);

        ok(
            &collection,
            json!({"author": "abcDEF123456789", "reviewers": ["abcDEF123456789"]}),
            false,
        );

        let failures = errors(&collection, json!({"author": "nope"}), false);
        assert_eq!(
            failures.fields["author"].code,
            "validation_invalid_relation"
        );
        let failures = errors(
            &collection,
            json!({"reviewers": ["abcDEF123456789", "abcDEF123456788", "abcDEF123456787"]}),
            false,
        );
        assert_eq!(
            failures.fields["reviewers"].code,
            "validation_too_many_values"
        );
    }

    #[test]
    fn every_failing_field_is_reported_at_once() {
        let collection = collection(vec![
            field("title", FieldType::Text, json!({}), true),
            field("views", FieldType::Number, json!({"onlyInt": true}), false),
        ]);
        let failures = errors(&collection, json!({"views": 1.5, "unknown": 1}), false);
        assert_eq!(failures.fields.len(), 3);
        assert!(failures.fields.contains_key("title"));
        assert!(failures.fields.contains_key("views"));
        assert!(failures.fields.contains_key("unknown"));
        // The JSON shape matches PocketBase's error `data`.
        let json = failures.to_json();
        assert_eq!(json["views"]["code"], json!("validation_not_an_integer"));
        assert!(json["views"]["message"].is_string());
    }

    #[test]
    fn autodate_fields_ignore_input() {
        let collection = collection(vec![field(
            "published_at",
            FieldType::Autodate,
            json!({"onCreate": true}),
            true,
        )]);
        ok(&collection, json!({}), false);
        ok(&collection, json!({"published_at": "whatever"}), false);
        assert!(encode_value(&collection.fields[0], &json!("whatever"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn values_are_encoded_for_their_column_types() {
        let number = field("views", FieldType::Number, json!({"onlyInt": true}), false);
        assert_eq!(
            encode_value(&number, &json!(10)).unwrap().as_deref(),
            Some("10")
        );
        assert_eq!(
            encode_value(&number, &json!("10")).unwrap().as_deref(),
            Some("10")
        );
        assert_eq!(encode_value(&number, &json!(null)).unwrap(), None);

        let boolean = field("published", FieldType::Bool, json!({}), false);
        assert_eq!(
            encode_value(&boolean, &json!(true)).unwrap().as_deref(),
            Some("true")
        );
        assert_eq!(
            encode_value(&boolean, &json!("off")).unwrap().as_deref(),
            Some("false")
        );

        let json_field = field("meta", FieldType::Json, json!({}), false);
        assert_eq!(
            encode_value(&json_field, &json!({"a": [1, 2]}))
                .unwrap()
                .as_deref(),
            Some("{\"a\":[1,2]}")
        );

        let multi = field(
            "tags",
            FieldType::Select,
            json!({"values": ["a", "b"], "maxSelect": 3}),
            false,
        );
        assert_eq!(
            encode_value(&multi, &json!(["a", "b"])).unwrap().as_deref(),
            Some("{\"a\",\"b\"}")
        );
        assert_eq!(
            encode_value(&multi, &json!([])).unwrap().as_deref(),
            Some("{}")
        );
    }

    #[test]
    fn text_arrays_escape_quotes_and_backslashes() {
        assert_eq!(pg_text_array(&["a".into(), "b".into()]), "{\"a\",\"b\"}");
        assert_eq!(pg_text_array(&[]), "{}");
        assert_eq!(
            pg_text_array(&["say \"hi\"".into(), "back\\slash".into()]),
            "{\"say \\\"hi\\\"\",\"back\\\\slash\"}"
        );
        assert_eq!(pg_text_array(&["comma,ok".into()]), "{\"comma,ok\"}");
    }
}
