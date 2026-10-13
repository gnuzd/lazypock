//! The `fields` projection and the `expand` selection.
//!
//! Both parameters describe a *tree* over the collection: `fields=id,author.name`
//! says which keys appear, `expand=author,comments.user` says which relations are
//! fetched. A nested `fields` entry only has an effect when the same relation is
//! also expanded, which mirrors PocketBase.
//!
//! Every segment is validated against the schema as it is parsed, so an unknown
//! field or a path that does not go through a relation is an error at request
//! time rather than a missing key in the response.

use std::collections::BTreeMap;

use lazypock_core::error::{Error, Result};

use super::compile::FilterContext;

/// Maximum depth of an `expand` path.
pub const MAX_EXPAND_DEPTH: usize = 6;
/// Maximum number of relations expanded at a single level.
pub const MAX_EXPAND_ENTRIES: usize = 16;
/// Maximum number of `fields` entries at a single level.
pub const MAX_FIELD_ENTRIES: usize = 128;

/// Which keys of a record are returned at one level.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FieldSelection {
    /// Scalar columns to include, in request order.
    pub columns: Vec<String>,
    /// Include every scalar column (`fields` absent, or `fields=*`).
    pub all_columns: bool,
    /// Per-relation nested selections.
    pub nested: BTreeMap<String, FieldSelection>,
}

impl FieldSelection {
    /// Everything: the default when `fields` is not supplied.
    pub fn all() -> Self {
        Self {
            all_columns: true,
            ..Default::default()
        }
    }

    /// Parse a `fields` value.
    pub fn parse(input: Option<&str>, context: &FilterContext) -> Result<Self> {
        let Some(input) = input.map(str::trim).filter(|raw| !raw.is_empty()) else {
            return Ok(Self::all());
        };

        let mut selection = Self::default();
        let mut entries = 0usize;
        for raw in input.split(',') {
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            entries += 1;
            if entries > MAX_FIELD_ENTRIES {
                return Err(Error::invalid(format!(
                    "too many `fields` entries (maximum {MAX_FIELD_ENTRIES})"
                )));
            }
            let path: Vec<String> = split_path(raw)?;
            selection.insert_path(&path, context)?;
        }

        if !selection.all_columns && selection.columns.is_empty() && selection.nested.is_empty() {
            return Err(Error::invalid(
                "`fields` must name at least one field, or be `*`",
            ));
        }
        Ok(selection)
    }

    fn insert_path(&mut self, path: &[String], context: &FilterContext) -> Result<()> {
        let (segment, rest) = path
            .split_first()
            .ok_or_else(|| Error::invalid("empty field path"))?;

        if segment == "*" {
            if rest.is_empty() {
                self.all_columns = true;
                return Ok(());
            }
            return Err(Error::invalid(
                "`*` must stand alone; write `author.*` to select every key of an expanded relation",
            ));
        }

        if rest.is_empty() {
            if !context.has_column(segment) {
                return Err(Error::invalid(format!(
                    "unknown field {segment:?} in collection {:?}",
                    context.table
                )));
            }
            if !self.columns.iter().any(|column| column == segment) {
                self.columns.push(segment.clone());
            }
            return Ok(());
        }

        let relation = context.relations.get(segment).ok_or_else(|| {
            Error::invalid(format!(
                "{segment:?} is not a relation field of collection {:?}, so {:?} cannot be selected through it",
                context.table,
                path.join(".")
            ))
        })?;
        if rest.len() > MAX_EXPAND_DEPTH {
            return Err(Error::invalid(format!(
                "`fields` path `{}` is deeper than the supported {MAX_EXPAND_DEPTH} levels",
                path.join(".")
            )));
        }
        self.nested
            .entry(segment.clone())
            .or_default()
            .insert_path(rest, &relation.context)
    }

    /// The effective scalar columns for this level.
    pub fn effective_columns(&self, context: &FilterContext) -> Vec<String> {
        if self.all_columns {
            let mut columns: Vec<String> = context.columns.keys().cloned().collect();
            columns.sort();
            columns
        } else {
            self.columns.clone()
        }
    }

    /// The nested selection for a relation, if any.
    pub fn nested_for(&self, relation: &str) -> Option<&FieldSelection> {
        self.nested.get(relation)
    }
}

/// Which relations are expanded at each level.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExpandSpec {
    /// Per-relation nested expansions.
    pub nested: BTreeMap<String, ExpandSpec>,
    /// Expand every relation at this level (`expand=*`).
    pub all: bool,
}

impl ExpandSpec {
    /// No expansion.
    pub fn none() -> Self {
        Self::default()
    }

    /// Parse an `expand` value.
    pub fn parse(input: Option<&str>, context: &FilterContext) -> Result<Self> {
        let Some(input) = input.map(str::trim).filter(|raw| !raw.is_empty()) else {
            return Ok(Self::default());
        };

        let mut spec = Self::default();
        let mut entries = 0usize;
        for raw in input.split(',') {
            let raw = raw.trim();
            if raw.is_empty() {
                continue;
            }
            entries += 1;
            if entries > MAX_EXPAND_ENTRIES {
                return Err(Error::invalid(format!(
                    "too many `expand` entries (maximum {MAX_EXPAND_ENTRIES})"
                )));
            }
            if raw == "*" {
                spec.all = true;
                continue;
            }
            let path = split_path(raw)?;
            spec.insert_path(&path, context)?;
        }
        Ok(spec)
    }

    fn insert_path(&mut self, path: &[String], context: &FilterContext) -> Result<()> {
        let (segment, rest) = path
            .split_first()
            .ok_or_else(|| Error::invalid("empty expand path"))?;

        let relation = context.relations.get(segment).ok_or_else(|| {
            Error::invalid(format!(
                "{segment:?} is not a relation field of collection {:?} and cannot be expanded",
                context.table
            ))
        })?;

        if rest.is_empty() {
            self.nested.entry(segment.clone()).or_default();
            return Ok(());
        }

        if rest.len() >= MAX_EXPAND_DEPTH {
            return Err(Error::invalid(format!(
                "`expand` path `{}` is deeper than the supported {MAX_EXPAND_DEPTH} levels",
                path.join(".")
            )));
        }
        self.nested
            .entry(segment.clone())
            .or_default()
            .insert_path(rest, &relation.context)
    }

    /// The nested spec for a relation, if it is being expanded.
    pub fn nested_for(&self, relation: &str) -> Option<&ExpandSpec> {
        self.nested.get(relation)
    }

    /// Whether nothing at all is expanded.
    pub fn is_empty(&self) -> bool {
        !self.all && self.nested.is_empty()
    }

    /// Resolve the relations to expand at this level, in schema order.
    pub fn relations_to_expand<'a>(&'a self, context: &'a FilterContext) -> Vec<&'a str> {
        if self.all {
            let mut names: Vec<&str> = context.relations.keys().map(String::as_str).collect();
            names.sort_unstable();
            names
        } else {
            self.nested.keys().map(String::as_str).collect()
        }
    }
}

fn split_path(raw: &str) -> Result<Vec<String>> {
    let segments: Vec<String> = raw.split('.').map(str::to_string).collect();
    for segment in &segments {
        if segment.is_empty() {
            return Err(Error::invalid(format!("malformed path {raw:?}")));
        }
        if segment == "*" {
            continue;
        }
        let mut characters = segment.chars();
        let first = characters.next().expect("non-empty");
        if !(first.is_ascii_alphabetic() || first == '_') {
            return Err(Error::invalid(format!(
                "malformed path {raw:?}: `{segment}` must start with a letter"
            )));
        }
        if let Some(bad) = characters.find(|c| !(c.is_ascii_alphanumeric() || *c == '_')) {
            return Err(Error::invalid(format!(
                "malformed path {raw:?}: unexpected {bad:?}"
            )));
        }
    }
    Ok(segments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::{Collection, Field};
    use crate::types::{CollectionType, FieldType};
    use serde_json::json;
    use std::time::SystemTime;

    fn field(name: &str, field_type: FieldType, options: serde_json::Value) -> Field {
        Field {
            id: format!("f_{name}"),
            collection_id: "c1".into(),
            name: name.into(),
            field_type,
            required: false,
            is_unique: false,
            options,
            position: 0,
        }
    }

    fn collection(id: &str, name: &str, fields: Vec<Field>) -> Collection {
        Collection {
            id: id.into(),
            name: name.into(),
            collection_type: CollectionType::Base,
            options: json!({}),
            rules: json!({}),
            fields,
            created: SystemTime::now(),
            updated: SystemTime::now(),
        }
    }

    fn fixture() -> FilterContext {
        let comments = collection(
            "c_comments",
            "comments",
            vec![
                field("body", FieldType::Text, json!({})),
                field(
                    "post",
                    FieldType::Relation,
                    json!({"collectionId": "c_posts"}),
                ),
            ],
        );
        let users = collection(
            "c_users",
            "users",
            vec![field("name", FieldType::Text, json!({}))],
        );
        let posts = collection(
            "c_posts",
            "posts",
            vec![
                field("title", FieldType::Text, json!({})),
                field(
                    "author",
                    FieldType::Relation,
                    json!({"collectionId": "c_users"}),
                ),
                field(
                    "comments",
                    FieldType::Relation,
                    json!({"collectionId": "c_comments", "maxSelect": 10}),
                ),
            ],
        );
        let all = vec![users, comments, posts.clone()];
        FilterContext::for_collection(&posts, &all).unwrap()
    }

    #[test]
    fn absent_fields_means_every_column() {
        let context = fixture();
        let selection = FieldSelection::parse(None, &context).unwrap();
        assert!(selection.all_columns);
        let columns = selection.effective_columns(&context);
        assert!(columns.contains(&"title".to_string()));
        assert!(columns.contains(&"id".to_string()));
        assert!(columns.contains(&"created".to_string()));
        // Deterministic ordering.
        let mut sorted = columns.clone();
        sorted.sort();
        assert_eq!(columns, sorted);
    }

    #[test]
    fn explicit_fields_are_kept_in_request_order() {
        let context = fixture();
        let selection = FieldSelection::parse(Some("title,id,title"), &context).unwrap();
        assert!(!selection.all_columns);
        assert_eq!(selection.columns, vec!["title", "id"]);

        let selection = FieldSelection::parse(Some("*"), &context).unwrap();
        assert!(selection.all_columns);
    }

    #[test]
    fn nested_field_paths_follow_relations() {
        let context = fixture();
        let selection =
            FieldSelection::parse(Some("id,author.name,comments.body"), &context).unwrap();
        assert_eq!(selection.columns, vec!["id"]);
        assert_eq!(
            selection.nested_for("author").unwrap().columns,
            vec!["name"]
        );
        assert_eq!(
            selection.nested_for("comments").unwrap().columns,
            vec!["body"]
        );
    }

    #[test]
    fn unknown_or_non_relation_paths_are_rejected() {
        let context = fixture();
        for bad in [
            "nope",
            "title.name",
            "author.nope",
            "author.name.first",
            "author.*.x",
            "title;drop",
            ",,",
            "()",
        ] {
            assert!(
                FieldSelection::parse(Some(bad), &context).is_err(),
                "{bad:?} should have been rejected"
            );
        }
    }

    #[test]
    fn blank_fields_is_the_same_as_absent() {
        let context = fixture();
        for blank in ["", "   "] {
            assert_eq!(
                FieldSelection::parse(Some(blank), &context).unwrap(),
                FieldSelection::all(),
                "{blank:?}"
            );
        }
    }

    #[test]
    fn expand_defaults_to_nothing() {
        let context = fixture();
        assert!(ExpandSpec::parse(None, &context).unwrap().is_empty());
        assert!(ExpandSpec::parse(Some("  "), &context).unwrap().is_empty());
    }

    #[test]
    fn expand_parses_relations_and_wildcards() {
        let context = fixture();

        let spec = ExpandSpec::parse(Some("author"), &context).unwrap();
        assert_eq!(spec.relations_to_expand(&context), vec!["author"]);
        assert!(spec.nested_for("author").unwrap().is_empty());

        // Deeper paths record the nested expansion: `comments` is expanded, and
        // `post` is expanded inside it.
        let spec = ExpandSpec::parse(Some("comments.post"), &context).unwrap();
        assert!(spec.nested.contains_key("comments"));
        let comments = spec.nested_for("comments").unwrap();
        assert!(!comments.all);
        assert!(comments.nested.contains_key("post"));

        // `*` expands every relation at this level.
        let spec = ExpandSpec::parse(Some("*"), &context).unwrap();
        assert_eq!(
            spec.relations_to_expand(&context),
            vec!["author", "comments"]
        );
    }

    #[test]
    fn expand_rejects_unknown_fields_and_excessive_limits() {
        let context = fixture();
        for bad in ["nope", "title", "author.name", "()", "author..name"] {
            assert!(
                ExpandSpec::parse(Some(bad), &context).is_err(),
                "{bad:?} should have been rejected"
            );
        }

        // Repeating one valid relation is still too many entries.
        let many = (0..MAX_EXPAND_ENTRIES + 1)
            .map(|_| "author".to_string())
            .collect::<Vec<_>>()
            .join(",");
        let err = ExpandSpec::parse(Some(&many), &context).unwrap_err();
        assert!(err.message().contains("too many"), "{err}");
    }

    #[test]
    fn expand_depth_is_capped_by_the_schema_shape() {
        // posts -> comments -> post -> comments ... terminates because the cycle
        // is cut at MAX_EXPAND_DEPTH.
        let context = fixture();
        let deep = (0..MAX_EXPAND_DEPTH + 2)
            .map(|i| if i % 2 == 0 { "comments" } else { "post" })
            .collect::<Vec<_>>()
            .join(".");
        let err = ExpandSpec::parse(Some(&deep), &context).unwrap_err();
        assert!(
            err.message().contains("deeper") || err.message().contains("not a relation field"),
            "{err}"
        );
    }
}
