//! Applying a collection's rules as row-level security policies.

use serde_json::Value;

use lazypock_core::error::{Error, Result};

use crate::collections::Collection;
use crate::ddl::quote_ident;
use crate::query::compile::FilterContext;
use crate::query::filter;
use crate::runner::QueryRunner;

use super::policy;

/// The roles every policy is scoped to.
///
/// Scoping to these two leaves the connecting role — the schema owner — untouched,
/// which is what lets internal work and superusers run unrestricted while every
/// user-facing request is bound by the policies.
const ROLES: &str = "lazypock_anon, lazypock_authenticated";

/// The rule keys a collection may define, with the policy name, the action each
/// maps to, and — for the two that become a `SELECT` policy — the
/// `request.action` value that selects it. Reads and writes must be kept
/// apart: Postgres OR-combines permissive `SELECT` policies, so without the
/// guard a public `viewRule` would widen a filtered `listRule` (and the other
/// way round).
const RULES: &[(&str, &str, &str, Option<&str>)] = &[
    ("listRule", "lazypock_list", "select", Some("list")),
    ("viewRule", "lazypock_view", "select", Some("view")),
    ("createRule", "lazypock_create", "insert", None),
    ("updateRule", "lazypock_update", "update", None),
    ("deleteRule", "lazypock_delete", "delete", None),
];

/// The [`SELECT`](RULES) mirrors an update or delete needs.
///
/// Postgres applies the `SELECT` policies to the rows an `UPDATE`/`DELETE`
/// reads to locate its target, so a write that had only its write policy would
/// silently match nothing. Each mirror carries the rule's own predicate,
/// guarded by the write action, so a row is visible to a write exactly when
/// that write's rule allows it — and never to a list or view.
const SELECT_MIRRORS: &[(&str, &str, &str)] = &[
    ("updateRule", "lazypock_update_select", "update"),
    ("deleteRule", "lazypock_delete_select", "delete"),
];

/// Every policy name this module creates and drops.
fn policy_names() -> impl Iterator<Item = &'static str> {
    RULES
        .iter()
        .map(|(_, name, _, _)| *name)
        .chain(SELECT_MIRRORS.iter().map(|(_, name, _)| *name))
        .chain(std::iter::once(MANAGE_POLICY))
}

/// The policy a `manageRule` becomes.
const MANAGE_POLICY: &str = "lazypock_manage";

/// The three states a rule can be in (PLAN.md section 6.2).
enum Rule {
    /// Absent or null: nobody but the service may do this.
    Absent,
    /// The empty string: anyone may.
    Public,
    /// A filter expression.
    Filter(Box<filter::Expr>),
}

/// Enable row-level security on a collection and rebuild its policies.
///
/// Returns the statements it ran, which is what the tests assert against.
///
/// The policies are rebuilt from scratch rather than patched, so a rule that was
/// removed actually stops applying. Nothing is created for an absent rule, which
/// is how the default-deny comes about: with RLS on and no matching policy, the
/// restricted roles see nothing.
pub async fn apply(
    runner: &dyn QueryRunner,
    collection: &Collection,
    context: &FilterContext,
) -> Result<Vec<String>> {
    let statements = statements(collection, context)?;

    runner
        .run_batch_execute(&statements.join(";\n"))
        .await
        .map_err(|err| {
            Error::invalid(format!(
                "cannot apply the rules of {:?}: {err}",
                collection.name
            ))
        })?;

    tracing::debug!(
        collection = %collection.name,
        rules = statements.len() - 1,
        "row-level security applied"
    );

    Ok(statements)
}

/// Build the statements that (re)create a collection's policies.
///
/// Split out from [`apply`] so the tests can assert the generated SQL without a
/// database, and so the two can never drift.
pub fn statements(collection: &Collection, context: &FilterContext) -> Result<Vec<String>> {
    let table = quote_ident(&collection.name);
    let mut statements = vec![format!("alter table {table} enable row level security")];

    for name in policy_names() {
        statements.push(format!(
            "drop policy if exists {} on {table}",
            quote_ident(name)
        ));
    }

    // `manageRule` grants every action, so it becomes one `for all` policy. It
    // is deliberately not action-guarded: matching it lets both reads and
    // writes through, which is what "manage" means.
    if let Some(predicate) = predicate_of(rule_of(&collection.rules, "manageRule")?, context)? {
        statements.push(policy_statement(
            MANAGE_POLICY,
            &table,
            "all",
            &predicate,
            None,
        ));
    }

    for (key, name, action, guard) in RULES {
        if let Some(predicate) = predicate_of(rule_of(&collection.rules, key)?, context)? {
            statements.push(policy_statement(name, &table, action, &predicate, *guard));
        }
    }

    // The `SELECT` a write needs to find its row: the same predicate, keyed to
    // the write action.
    for (key, name, action) in SELECT_MIRRORS {
        if let Some(predicate) = predicate_of(rule_of(&collection.rules, key)?, context)? {
            statements.push(policy_statement(
                name,
                &table,
                "select",
                &predicate,
                Some(action),
            ));
        }
    }

    Ok(statements)
}

/// Remove every policy this module manages from a collection's table.
pub async fn clear(runner: &dyn QueryRunner, collection: &Collection) -> Result<Vec<String>> {
    let table = quote_ident(&collection.name);
    let mut statements: Vec<String> = policy_names()
        .map(|name| format!("drop policy if exists {} on {table}", quote_ident(name)))
        .collect();
    statements.push(format!("alter table {table} disable row level security"));

    runner
        .run_batch_execute(&statements.join(";\n"))
        .await
        .map_err(|err| {
            Error::invalid(format!(
                "cannot clear the rules of {:?}: {err}",
                collection.name
            ))
        })?;

    Ok(statements)
}

fn rule_of(rules: &Value, key: &str) -> Result<Rule> {
    match rules.get(key) {
        None | Some(Value::Null) => Ok(Rule::Absent),
        Some(Value::String(text)) if text.trim().is_empty() => Ok(Rule::Public),
        Some(Value::String(text)) => Ok(Rule::Filter(Box::new(filter::parse(text)?))),
        Some(other) => Err(Error::invalid(format!(
            "the {key} rule must be a string or null, got {other}"
        ))),
    }
}

fn predicate_of(rule: Rule, context: &FilterContext) -> Result<Option<String>> {
    Ok(match rule {
        Rule::Absent => None,
        Rule::Public => Some("true".to_string()),
        Rule::Filter(expression) => Some(policy::predicate(&expression, context)?),
    })
}

fn policy_statement(
    name: &str,
    table: &str,
    action: &str,
    predicate: &str,
    guard: Option<&str>,
) -> String {
    let name = quote_ident(name);
    // Only the two `SELECT` rules carry a guard, and its value comes from a
    // closed set, never from a request.
    let guarded = match guard {
        Some(action) => {
            format!("current_setting('request.action', true) = '{action}' and ({predicate})")
        }
        None => predicate.to_string(),
    };
    match action {
        "select" => {
            format!("create policy {name} on {table} for select to {ROLES} using ({guarded})")
        }
        "insert" => {
            format!(
                "create policy {name} on {table} for insert to {ROLES} with check ({predicate})"
            )
        }
        "update" => format!(
            "create policy {name} on {table} for update to {ROLES} \
             using ({predicate}) with check ({predicate})"
        ),
        "delete" => {
            format!("create policy {name} on {table} for delete to {ROLES} using ({predicate})")
        }
        _ => format!(
            "create policy {name} on {table} for all to {ROLES} \
             using ({guarded}) with check ({guarded})"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::{Collection, Field};
    use crate::query::compile::FilterContext;
    use crate::types::{CollectionType, FieldType};
    use serde_json::json;
    use std::time::SystemTime;

    fn context() -> FilterContext {
        let posts = Collection {
            id: "c1".into(),
            name: "posts".into(),
            collection_type: CollectionType::Base,
            options: json!({}),
            rules: json!({}),
            fields: vec![Field {
                id: "f1".into(),
                collection_id: "c1".into(),
                name: "author".into(),
                field_type: FieldType::Text,
                required: false,
                is_unique: false,
                options: json!({}),
                position: 0,
            }],
            created: SystemTime::now(),
            updated: SystemTime::now(),
        };
        FilterContext::for_collection(&posts, std::slice::from_ref(&posts)).unwrap()
    }

    fn statements(rules: Value) -> Vec<String> {
        let context = context();
        let collection = Collection {
            id: "c1".into(),
            name: "posts".into(),
            collection_type: CollectionType::Base,
            options: json!({}),
            rules,
            fields: Vec::new(),
            created: SystemTime::now(),
            updated: SystemTime::now(),
        };
        // The same builder `apply` runs, so the tests cannot drift from it.
        super::statements(&collection, &context).unwrap()
    }

    #[test]
    fn row_level_security_is_enabled_and_nothing_is_forced() {
        let statements = statements(json!({}));
        assert_eq!(
            statements[0],
            "alter table \"posts\" enable row level security"
        );
        // Forcing it would subject the schema owner to policies and break the
        // server's own internal reads.
        assert!(
            !statements.iter().any(|sql| sql.contains("force")),
            "{statements:?}"
        );
    }

    #[test]
    fn an_absent_rule_creates_no_policy() {
        // Default deny: with RLS on and no policy, the restricted roles see
        // nothing at all.
        let statements = statements(json!({}));
        assert_eq!(
            statements.len(),
            9,
            "one enable plus eight drops: {statements:?}"
        );
        assert!(!statements.iter().any(|sql| sql.contains("create policy")));
    }

    #[test]
    fn a_public_list_rule_allows_everyone_only_for_a_list() {
        let statements = statements(json!({"listRule": ""}));
        let policy = statements
            .iter()
            .find(|sql| sql.contains("create policy"))
            .expect("a policy");
        assert!(policy.contains("for select"), "{policy}");
        assert!(
            policy.contains("current_setting('request.action', true) = 'list'"),
            "{policy}"
        );
        assert!(policy.contains("(true)"), "{policy}");
        assert!(
            policy.contains("to lazypock_anon, lazypock_authenticated"),
            "{policy}"
        );
    }

    #[test]
    fn a_public_view_rule_does_not_widen_a_filtered_list_rule() {
        // The bug this guards: both rules become `FOR SELECT` policies and
        // Postgres OR-combines permissive policies, so a public `viewRule`
        // used to make a filtered `listRule` readable by everyone.
        let statements = statements(json!({
            "listRule": "author = @request.auth.id",
            "viewRule": "",
        }));
        let created: Vec<&String> = statements
            .iter()
            .filter(|sql| sql.contains("create policy"))
            .collect();
        assert_eq!(created.len(), 2, "{statements:?}");

        let list = created
            .iter()
            .find(|sql| sql.contains("lazypock_list"))
            .expect("the list policy");
        let view = created
            .iter()
            .find(|sql| sql.contains("lazypock_view"))
            .expect("the view policy");

        assert!(
            list.contains("current_setting('request.action', true) = 'list'"),
            "{list}"
        );
        assert!(list.contains("(\"author\" = auth.uid()::text)"), "{list}");
        assert!(
            !list.contains("(true)"),
            "the public view rule must not leak into the list policy: {list}"
        );
        assert!(
            view.contains("current_setting('request.action', true) = 'view'"),
            "{view}"
        );
        assert!(view.contains("(true)"), "{view}");
    }

    #[test]
    fn a_filter_rule_becomes_the_predicate() {
        let statements = statements(json!({"listRule": "author = @request.auth.id"}));
        let policy = statements
            .iter()
            .find(|sql| sql.contains("create policy"))
            .expect("a policy");
        assert!(
            policy.contains("current_setting('request.action', true) = 'list'"),
            "{policy}"
        );
        assert!(
            policy.contains("(\"author\" = auth.uid()::text)"),
            "{policy}"
        );
    }

    #[test]
    fn every_action_maps_to_the_right_policy_clause() {
        let statements = statements(json!({
            "createRule": "author = @request.auth.id",
            "updateRule": "author = @request.auth.id",
            "deleteRule": "author = @request.auth.id",
        }));
        let created: Vec<&String> = statements
            .iter()
            .filter(|sql| sql.contains("create policy"))
            .collect();
        assert_eq!(created.len(), 5, "{statements:?}");

        let find = |needle: &str| {
            created
                .iter()
                .find(|sql| sql.contains(needle))
                .unwrap_or_else(|| panic!("no {needle} policy: {statements:?}"))
        };

        let insert = find("lazypock_create");
        assert!(insert.contains("for insert") && insert.contains("with check"));

        // The trailing quote keeps `lazypock_update"` from matching the
        // `lazypock_update_select` mirror.
        let update = find("lazypock_update\"");
        assert!(update.contains("for update") && update.contains("using"));
        assert!(
            update.contains("with check"),
            "an update needs both clauses"
        );

        let delete = find("lazypock_delete\"");
        assert!(delete.contains("for delete") && delete.contains("using"));

        // The `SELECT` a write needs to find its row must carry the write's own
        // rule, keyed to the write action — never the list or view rule.
        let update_select = find("lazypock_update_select");
        assert!(
            update_select.contains("for select")
                && update_select.contains("current_setting('request.action', true) = 'update'")
                && update_select.contains("(\"author\" = auth.uid()::text)"),
            "{update_select}"
        );
        let delete_select = find("lazypock_delete_select");
        assert!(
            delete_select.contains("for select")
                && delete_select.contains("current_setting('request.action', true) = 'delete'")
                && delete_select.contains("(\"author\" = auth.uid()::text)"),
            "{delete_select}"
        );
    }

    #[test]
    fn manage_rule_grants_every_action() {
        let statements = statements(json!({"manageRule": "author = @request.auth.id"}));
        let policy = statements
            .iter()
            .find(|sql| sql.contains("create policy"))
            .expect("a policy");
        assert!(policy.contains("lazypock_manage"), "{policy}");
        assert!(policy.contains("for all"), "{policy}");
        assert!(
            policy.contains("using (") && policy.contains("with check ("),
            "{policy}"
        );
    }

    #[test]
    fn a_malformed_rule_is_refused() {
        let context = context();
        assert!(rule_of(&json!({"listRule": 5}), "listRule").is_err());
        assert!(rule_of(&json!({"listRule": "nope = 1"}), "listRule").is_ok());
        // ... and a rule naming an unknown column fails when compiled.
        let rule = rule_of(&json!({"listRule": "nope = 1"}), "listRule").unwrap();
        assert!(predicate_of(rule, &context).is_err());
    }
}
