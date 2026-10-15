//! `files.rules`: who may list and delete what.
//!
//! The rules live in the `files` setting — the Studio's **Settings → Media** page writes
//! them — as `{ "rules": { "listRule": …, "deleteRule": … } }`, with the same three
//! states a collection rule has:
//!
//! | value           | meaning                                            |
//! |-----------------|----------------------------------------------------|
//! | absent / `null` | superusers only; everyone else is denied            |
//! | `""`            | public                                              |
//! | a filter        | conditional, evaluated per file row                 |
//!
//! A whitespace-only or non-string value **denies** rather than quietly becoming
//! public: a rule that cannot be read is not a rule that allows. A superuser bypasses
//! every file rule, which is what keeps the Studio's media library working.
//!
//! The columns a rule may name are `_files`' own, and `@request.auth.*` resolves the
//! same way it does for a collection rule — the same parser and compiler are used, so
//! the two vocabularies cannot drift apart.
//!
//! **Serving is not covered.** A file is reached by its key, which carries a random
//! component, so a URL behaves as a capability — the same model PocketBase uses. These
//! rules gate the API's listing and deletion, and the Studio's page says so.

use std::collections::HashMap;

use serde_json::Value;

use lazypock_core::error::{Error, Result};
use lazypock_db::query::compile::{self, AuthContext, CompiledFilter, FilterContext};
use lazypock_db::query::filter;

/// The columns a file rule may name, with the Postgres type each one has.
///
/// Rule compilation needs the types so a comparison is emitted with the right cast
/// (`"size" > $1::bigint` rather than a text comparison), exactly as a collection rule
/// gets them from `_fields`.
pub fn columns() -> HashMap<String, String> {
    [
        ("id", "text"),
        ("filename", "text"),
        ("extension", "text"),
        ("mime_type", "text"),
        ("size", "bigint"),
        ("storage_path", "text"),
        ("storage_backend", "text"),
        ("collection_name", "text"),
        ("record_id", "text"),
        ("field_name", "text"),
        ("variants", "jsonb"),
        ("status", "text"),
        ("original_name", "text"),
        ("width", "integer"),
        ("height", "integer"),
        ("checksum", "text"),
        ("origin", "text"),
        ("uploaded_by", "text"),
        ("attached_at", "timestamptz"),
        ("created_at", "timestamptz"),
        ("updated_at", "timestamptz"),
    ]
    .into_iter()
    .map(|(name, sql_type)| (name.to_string(), sql_type.to_string()))
    .collect()
}

/// The context a file rule is compiled against.
///
/// `_files` is not a collection, so its context is built here rather than from
/// `FilterContext::for_collection`. There are no relation hops: a file rule may name a
/// column of the file, not another table.
pub fn context() -> FilterContext {
    FilterContext {
        table: "_files".to_string(),
        columns: columns(),
        relations: HashMap::new(),
    }
}

/// One rule, in its three states.
#[derive(Debug, Clone, PartialEq)]
pub enum FileRule {
    /// Absent, `null`, or unreadable: only a superuser may.
    SuperusersOnly,
    /// The empty string: anyone may.
    Public,
    /// A filter expression, evaluated against one file row.
    Filter(Box<filter::Expr>),
}

impl FileRule {
    /// Read one rule out of the setting's value.
    ///
    /// Anything that is not `null`, the empty string or a filter string is refused
    /// rather than guessed at, and the caller treats a refusal as a denial.
    pub fn parse(value: Option<&Value>) -> Result<Self> {
        match value {
            None | Some(Value::Null) => Ok(FileRule::SuperusersOnly),
            Some(Value::String(text)) if text.is_empty() => Ok(FileRule::Public),
            // Whitespace is not the empty string. A rule someone meant to write and did
            // not is not a public one, so it denies.
            Some(Value::String(text)) if text.trim().is_empty() => Err(Error::invalid(
                "a file rule is whitespace only; write a filter or leave it empty to make it public",
            )),
            Some(Value::String(text)) => Ok(FileRule::Filter(Box::new(filter::parse(text)?))),
            Some(other) => Err(Error::invalid(format!(
                "a file rule must be a string or null, got {other}"
            ))),
        }
    }
}

/// The file layer's two rules.
#[derive(Debug, Clone, PartialEq)]
pub struct FileRules {
    /// Who may list the library.
    pub list: FileRule,
    /// Who may delete a file.
    pub delete: FileRule,
}

impl Default for FileRules {
    fn default() -> Self {
        // Nothing configured means nothing is granted: the safe reading of an absent
        // rule, and the one a fresh installation has.
        Self {
            list: FileRule::SuperusersOnly,
            delete: FileRule::SuperusersOnly,
        }
    }
}

impl FileRules {
    /// Read both rules from the `files` setting.
    ///
    /// A malformed rule is *not* an error here: it is carried as
    /// [`FileRule::SuperusersOnly`] by [`Self::from_setting`] so a typo locks the file
    /// API down rather than opening it up. The reason is logged by the caller.
    pub fn from_setting(setting: &Value) -> Self {
        let rules = setting.get("rules").unwrap_or(&Value::Null);

        let read = |key: &str| match FileRule::parse(rules.get(key)) {
            Ok(rule) => rule,
            Err(err) => {
                tracing::warn!(
                    rule = key,
                    error = %err,
                    "a file rule is not readable; the file API stays superuser-only for it"
                );
                FileRule::SuperusersOnly
            }
        };

        Self {
            list: read("listRule"),
            delete: read("deleteRule"),
        }
    }

    /// What may list the library, and what to restrict the listing to.
    ///
    /// `Ok(None)` means everything is visible. `Ok(Some(filter))` means only the rows
    /// the filter matches. `Err` means this caller may not list at all.
    pub fn list_predicate(&self, auth: &AuthContext) -> Result<Option<CompiledFilter>> {
        match &self.list {
            FileRule::SuperusersOnly => Err(Error::forbidden("Access denied by file listRule")),
            FileRule::Public => Ok(None),
            FileRule::Filter(expression) => {
                Ok(Some(compile::compile(expression, &context(), auth)?))
            }
        }
    }

    /// What may delete a file, and what the file must match.
    ///
    /// `Ok(None)` means anyone may. `Ok(Some(filter))` means the row must match it.
    /// `Err` means nobody but a superuser may.
    pub fn delete_predicate(&self, auth: &AuthContext) -> Result<Option<CompiledFilter>> {
        match &self.delete {
            FileRule::SuperusersOnly => Err(Error::forbidden("Access denied by file deleteRule")),
            FileRule::Public => Ok(None),
            FileRule::Filter(expression) => {
                Ok(Some(compile::compile(expression, &context(), auth)?))
            }
        }
    }
}

/// Renumber the `$1..$n` placeholders of a compiled fragment.
///
/// A fragment is compiled on its own and then appended to a query that already uses
/// earlier positions, so its placeholders have to move — the alternative is a query
/// that silently compares a filter against the wrong value.
/// A `$` can only start a placeholder in compiled SQL: every literal is a parameter, so
/// a `$` inside a value is never in this text.
pub fn shift_placeholders(sql: &str, offset: usize) -> String {
    if offset == 0 {
        return sql.to_string();
    }

    let mut out = String::with_capacity(sql.len());
    let mut characters = sql.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '$' {
            out.push(character);
            continue;
        }

        let mut digits = String::new();
        while let Some(next) = characters.peek() {
            if next.is_ascii_digit() {
                digits.push(*next);
                characters.next();
            } else {
                break;
            }
        }

        match digits.parse::<usize>() {
            Ok(position) => out.push_str(&format!("${}", position + offset)),
            Err(_) => {
                out.push('$');
                out.push_str(&digits);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_three_rule_states_are_read_as_written() {
        use serde_json::Value::Null;

        assert_eq!(
            FileRule::parse(None).expect("absent"),
            FileRule::SuperusersOnly
        );
        assert_eq!(
            FileRule::parse(Some(&Null)).expect("null"),
            FileRule::SuperusersOnly
        );
        assert_eq!(
            FileRule::parse(Some(&json!(""))).expect("empty"),
            FileRule::Public
        );
        assert!(matches!(
            FileRule::parse(Some(&json!("uploaded_by = @request.auth.id"))).expect("filter"),
            FileRule::Filter(_)
        ));
    }

    #[test]
    fn a_rule_that_cannot_be_read_denies_rather_than_allowing() {
        // Whitespace is not the empty string: someone meant to write a rule and did
        // not, so the file API stays shut.
        assert!(FileRule::parse(Some(&json!("   "))).is_err());
        // A non-string is refused rather than coerced.
        assert!(FileRule::parse(Some(&json!(false))).is_err());
        assert!(FileRule::parse(Some(&json!(42))).is_err());
        // A malformed filter is refused too.
        assert!(FileRule::parse(Some(&json!("@request.auth.id ="))).is_err());
    }

    #[test]
    fn a_setting_with_no_rules_is_superuser_only() {
        let rules = FileRules::from_setting(&json!({}));
        assert_eq!(rules.list, FileRule::SuperusersOnly);
        assert_eq!(rules.delete, FileRule::SuperusersOnly);
    }

    #[test]
    fn a_setting_carries_the_two_rules_separately() {
        let rules = FileRules::from_setting(&json!({
            "rules": { "listRule": "", "deleteRule": "uploaded_by = @request.auth.id" }
        }));
        assert_eq!(rules.list, FileRule::Public);
        assert!(matches!(rules.delete, FileRule::Filter(_)));
    }

    #[test]
    fn a_broken_rule_locks_down_only_itself() {
        let rules = FileRules::from_setting(&json!({
            "rules": { "listRule": "", "deleteRule": "   " }
        }));
        assert_eq!(
            rules.list,
            FileRule::Public,
            "the readable rule still applies"
        );
        assert_eq!(
            rules.delete,
            FileRule::SuperusersOnly,
            "the unreadable one denies"
        );
    }

    #[test]
    fn a_public_rule_needs_no_predicate() {
        let auth = AuthContext::default();
        let rules = FileRules {
            list: FileRule::Public,
            delete: FileRule::Public,
        };
        assert!(rules.list_predicate(&auth).expect("allowed").is_none());
        assert!(rules.delete_predicate(&auth).expect("allowed").is_none());
    }

    #[test]
    fn an_absent_rule_denies_with_a_message_that_names_it() {
        let auth = AuthContext::default();
        let rules = FileRules::default();
        let err = rules.list_predicate(&auth).expect_err("denied");
        assert!(err.message().contains("listRule"), "{}", err.message());
        let err = rules.delete_predicate(&auth).expect_err("denied");
        assert!(err.message().contains("deleteRule"), "{}", err.message());
    }

    #[test]
    fn a_filter_rule_compiles_against_the_file_columns() {
        let auth = AuthContext {
            id: Some("u1".into()),
            ..AuthContext::default()
        };
        let rules = FileRules {
            list: FileRule::parse(Some(&json!("uploaded_by = @request.auth.id"))).expect("rule"),
            delete: FileRule::SuperusersOnly,
        };

        let predicate = rules
            .list_predicate(&auth)
            .expect("compiled")
            .expect("a filter");
        // The column is a real one, so it is emitted as a comparison rather than being
        // refused by the compiler.
        assert!(predicate.sql.contains("uploaded_by"), "{}", predicate.sql);
        assert_eq!(predicate.params, vec!["u1".to_string()]);
    }

    #[test]
    fn an_unknown_column_in_a_rule_is_refused_not_ignored() {
        let auth = AuthContext::default();
        let rules = FileRules {
            list: FileRule::parse(Some(&json!("nonsense = 1"))).expect("parses"),
            delete: FileRule::SuperusersOnly,
        };
        assert!(rules.list_predicate(&auth).is_err());
    }

    #[test]
    fn placeholders_are_renumbered_by_the_offset() {
        assert_eq!(shift_placeholders("a = $1", 0), "a = $1");
        assert_eq!(
            shift_placeholders("a = $1 and b = $2", 3),
            "a = $4 and b = $5"
        );
        // Multi-digit positions and a `$` that is not a placeholder both survive.
        assert_eq!(shift_placeholders("x = $10", 2), "x = $12");
        assert_eq!(shift_placeholders("price > '$'", 5), "price > '$'");
        assert_eq!(shift_placeholders("no placeholders", 4), "no placeholders");
    }
}
