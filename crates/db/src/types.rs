//! Collection and field types: the names stored in `_collections.type` and
//! `_fields.type`, and what each maps to in Postgres.
//!
//! The set matches the rest of lazypock so existing metadata loads unchanged.
//! This crate can *create* a subset (see [`FieldType::column_type`]); the rest
//! still load and show up in the schema, they just cannot be added here yet.

use crate::error::{self};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use lazypock_core::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionType {
    Base,
    Auth,
    View,
}

impl CollectionType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Base => "base",
            Self::Auth => "auth",
            Self::View => "view",
        }
    }
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "base" => Ok(Self::Base),
            "auth" => Ok(Self::Auth),
            "view" => Ok(Self::View),
            other => Err(error::invalid_schema(format!(
                "unknown collection type {other:?}"
            ))),
        }
    }
    pub fn is_auth(self) -> bool {
        self == Self::Auth
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldType {
    Text,
    Editor,
    Email,
    Url,
    Number,
    Bool,
    Date,
    Select,
    File,
    Relation,
    Json,
    GeoPoint,
    Autodate,
    Password,
}

/// How a column compares in a filter or sorts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Int,
    Number,
    Bool,
    Date,
    Json,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Text => "text",
            Kind::Int => "integer",
            Kind::Number => "number",
            Kind::Bool => "bool",
            Kind::Date => "date",
            Kind::Json => "json",
        }
    }
}

pub(crate) fn option_bool(options: &Value, key: &str) -> bool {
    options.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn is_multiple(options: &Value) -> bool {
    options
        .get("maxSelect")
        .and_then(Value::as_i64)
        .is_some_and(|n| n > 1)
}

impl FieldType {
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

    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "text" => Self::Text,
            "editor" => Self::Editor,
            "email" => Self::Email,
            "url" => Self::Url,
            "number" => Self::Number,
            "bool" => Self::Bool,
            "date" => Self::Date,
            "select" => Self::Select,
            "file" => Self::File,
            "relation" => Self::Relation,
            "json" => Self::Json,
            "geoPoint" => Self::GeoPoint,
            "autodate" => Self::Autodate,
            "password" => Self::Password,
            other => {
                return Err(error::invalid_schema(format!(
                    "unknown field type {other:?}"
                )));
            }
        })
    }

    /// Postgres column type, for the types this crate can create.
    pub fn column_type(self, options: &Value) -> Result<&'static str> {
        let unsupported = |what: &str| {
            Err(error::invalid_schema(format!(
                "{what} fields are not supported yet"
            )))
        };
        Ok(match self {
            Self::Text | Self::Editor | Self::Email | Self::Url | Self::Password => "text",
            Self::Number if option_bool(options, "onlyInt") => "bigint",
            Self::Number => "double precision",
            Self::Bool => "boolean",
            Self::Date => "timestamptz",
            Self::Select if is_multiple(options) => return unsupported("multiple select"),
            Self::Select => "text",
            Self::Json => "jsonb",
            Self::File => return unsupported("file"),
            Self::Relation => return unsupported("relation"),
            Self::GeoPoint => return unsupported("geoPoint"),
            Self::Autodate => return unsupported("autodate"),
        })
    }

    /// How a filter compares this column; `None` = cannot be used in a filter.
    pub fn kind(self, options: &Value) -> Option<Kind> {
        match self {
            Self::Text | Self::Editor | Self::Email | Self::Url => Some(Kind::Text),
            Self::Select if !is_multiple(options) => Some(Kind::Text),
            Self::Number if option_bool(options, "onlyInt") => Some(Kind::Int),
            Self::Number => Some(Kind::Number),
            Self::Bool => Some(Kind::Bool),
            Self::Date | Self::Autodate => Some(Kind::Date),
            Self::Json | Self::GeoPoint => Some(Kind::Json),
            Self::Select | Self::File | Self::Relation | Self::Password => None,
        }
    }
}

macro_rules! serde_as_str {
    ($t:ty) => {
        impl Serialize for $t {
            fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
                s.serialize_str(self.as_str())
            }
        }
        impl<'de> Deserialize<'de> for $t {
            fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                <$t>::parse(&s).map_err(serde::de::Error::custom)
            }
        }
    };
}
serde_as_str!(CollectionType);
serde_as_str!(FieldType);

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn names_round_trip() {
        for t in [
            "text", "editor", "email", "url", "number", "bool", "date", "select", "file",
            "relation", "json", "geoPoint", "autodate", "password",
        ] {
            assert_eq!(FieldType::parse(t).unwrap().as_str(), t);
        }
        for t in ["base", "auth", "view"] {
            assert_eq!(CollectionType::parse(t).unwrap().as_str(), t);
        }
        assert!(FieldType::parse("nope").is_err());
    }

    #[test]
    fn column_types_and_kinds() {
        assert_eq!(
            FieldType::Number.column_type(&json!({})).unwrap(),
            "double precision"
        );
        assert_eq!(
            FieldType::Number
                .column_type(&json!({"onlyInt": true}))
                .unwrap(),
            "bigint"
        );
        assert_eq!(
            FieldType::Number.kind(&json!({"onlyInt": true})),
            Some(Kind::Int)
        );
        assert!(FieldType::Relation.column_type(&json!({})).is_err());
        assert!(
            FieldType::Select
                .column_type(&json!({"maxSelect": 3}))
                .is_err()
        );
        assert_eq!(FieldType::Select.column_type(&json!({})).unwrap(), "text");
        assert_eq!(FieldType::Password.kind(&json!({})), None);
    }
}
