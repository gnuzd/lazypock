//! The `sort` query parameter: `-created,title`.
//!
//! Sorting is limited to real columns of the collection. Sorting through a
//! relation needs a join, which is the `expand` work; until that exists a dotted
//! sort path is refused loudly rather than silently ignored.

use lazypock_core::error::{Error, Result};

use super::compile::FilterContext;
use crate::ddl::quote_ident;

/// Maximum number of sort fields accepted.
pub const MAX_SORT_FIELDS: usize = 8;

/// One sort key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortField {
    /// Dotted field path.
    pub path: Vec<String>,
    /// `-` prefix: descending.
    pub descending: bool,
}

/// A parsed sort specification, applied in order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SortSpec {
    /// Sort keys.
    pub fields: Vec<SortField>,
}

impl SortSpec {
    /// The PocketBase default ordering.
    pub fn default_spec() -> Result<Self> {
        parse("-created")
    }
}

/// Parse a `sort` value.
pub fn parse(input: &str) -> Result<SortSpec> {
    let mut fields = Vec::new();

    for (index, raw) in input.split(',').enumerate() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let (descending, path) = match raw.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, raw.strip_prefix('+').unwrap_or(raw)),
        };
        if path.is_empty() {
            return Err(Error::invalid(format!(
                "sort key {} is missing a field name",
                index + 1
            )));
        }

        let segments: Vec<String> = path.split('.').map(str::to_string).collect();
        for segment in &segments {
            validate_segment(segment, path)?;
        }

        fields.push(SortField {
            path: segments,
            descending,
        });
    }

    if fields.is_empty() {
        return Err(Error::invalid("sort must name at least one field"));
    }
    if fields.len() > MAX_SORT_FIELDS {
        return Err(Error::invalid(format!(
            "too many sort fields: {} (maximum {MAX_SORT_FIELDS})",
            fields.len()
        )));
    }

    Ok(SortSpec { fields })
}

/// Compile a sort specification into an `order by` body.
pub fn compile(spec: &SortSpec, context: &FilterContext) -> Result<String> {
    if spec.fields.is_empty() {
        return Err(Error::internal("sort specification has no fields"));
    }

    let mut parts = Vec::with_capacity(spec.fields.len());
    for field in &spec.fields {
        if field.path.len() != 1 {
            return Err(Error::unsupported(format!(
                "sorting by `{}` is not supported yet: sorting through a relation needs a join",
                field.path.join(".")
            )));
        }
        let column = &field.path[0];
        if !context.has_column(column) {
            return Err(Error::invalid(format!(
                "unknown sort field {column:?} in collection {:?}",
                context.table
            )));
        }
        parts.push(format!(
            "{} {}",
            quote_ident(column),
            if field.descending { "desc" } else { "asc" }
        ));
    }

    Ok(parts.join(", "))
}

fn validate_segment(segment: &str, path: &str) -> Result<()> {
    if segment.is_empty() {
        return Err(Error::invalid(format!("malformed sort field {path:?}")));
    }
    let mut characters = segment.chars();
    let first = characters.next().expect("non-empty");
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(Error::invalid(format!(
            "malformed sort field {path:?}: `{segment}` must start with a letter"
        )));
    }
    if let Some(bad) = characters.find(|c| !(c.is_ascii_alphanumeric() || *c == '_')) {
        return Err(Error::invalid(format!(
            "malformed sort field {path:?}: unexpected {bad:?}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::{Collection, Field};
    use crate::types::{CollectionType, FieldType};
    use serde_json::json;
    use std::time::SystemTime;

    fn context() -> FilterContext {
        let collection = Collection {
            id: "c1".into(),
            name: "posts".into(),
            collection_type: CollectionType::Base,
            options: json!({}),
            rules: json!({}),
            fields: vec![
                Field {
                    id: "f1".into(),
                    collection_id: "c1".into(),
                    name: "title".into(),
                    field_type: FieldType::Text,
                    required: false,
                    is_unique: false,
                    options: json!({}),
                    position: 0,
                },
                Field {
                    id: "f2".into(),
                    collection_id: "c1".into(),
                    name: "author".into(),
                    field_type: FieldType::Relation,
                    required: false,
                    is_unique: false,
                    options: json!({"collectionId": "c_users"}),
                    position: 1,
                },
            ],
            created: SystemTime::now(),
            updated: SystemTime::now(),
        };
        FilterContext::for_collection(&collection, std::slice::from_ref(&collection)).unwrap()
    }

    #[test]
    fn parses_ascending_and_descending_keys() {
        let spec = parse("-created,title").unwrap();
        assert_eq!(spec.fields.len(), 2);
        assert_eq!(spec.fields[0].path, vec!["created"]);
        assert!(spec.fields[0].descending);
        assert_eq!(spec.fields[1].path, vec!["title"]);
        assert!(!spec.fields[1].descending);
    }

    #[test]
    fn tolerates_whitespace_and_explicit_ascending() {
        let spec = parse("  -views , +title ").unwrap();
        assert!(spec.fields[0].descending);
        assert_eq!(spec.fields[0].path, vec!["views"]);
        assert!(!spec.fields[1].descending);
    }

    #[test]
    fn compiles_to_order_by_bodies() {
        let context = context();
        let spec = parse("-created,title").unwrap();
        assert_eq!(
            compile(&spec, &context).unwrap(),
            "\"created\" desc, \"title\" asc"
        );
    }

    #[test]
    fn default_spec_is_newest_first() {
        let context = context();
        assert_eq!(
            compile(&SortSpec::default_spec().unwrap(), &context).unwrap(),
            "\"created\" desc"
        );
    }

    #[test]
    fn rejects_unwhitelisted_and_malformed_fields() {
        let context = context();
        // Syntactically malformed keys never reach the compiler.
        for bad in ["title;drop", "1title", "tit le", "title-x", "()", "- "] {
            assert!(parse(bad).is_err(), "{bad:?} should not parse");
        }
        // A well-formed name that is not part of the collection is refused at
        // compile time, against the whitelist.
        let spec = parse("nope").unwrap();
        let err = compile(&spec, &context).unwrap_err();
        assert!(err.message().contains("unknown sort field"), "{err}");
    }

    #[test]
    fn rejects_empty_and_oversized_specs() {
        assert!(parse("").is_err());
        assert!(parse(" , , ").is_err());
        let many = (0..MAX_SORT_FIELDS + 1)
            .map(|i| format!("f{i}"))
            .collect::<Vec<_>>()
            .join(",");
        assert!(parse(&many).is_err());
    }

    #[test]
    fn sorting_through_a_relation_is_refused_explicitly() {
        let context = context();
        let spec = parse("-author.name").unwrap();
        let err = compile(&spec, &context).unwrap_err();
        assert!(err.message().contains("not supported yet"), "{err}");
    }
}
