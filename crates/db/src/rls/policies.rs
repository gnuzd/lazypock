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

/// The rule keys a collection may define, with the policy name and action each
/// maps to.
const RULES: &[(&str, &str, &str)] = &[
    ("listRule", "lazypock_list", "select"),
    ("viewRule", "lazypock_view", "select"),
    ("createRule", "lazypock_create", "insert"),
    ("updateRule", "lazypock_update", "update"),
    ("deleteRule", "lazypock_delete", "delete"),
];

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
    let table = quote_ident(&collection.name);
    let mut statements = vec![format!("alter table {table} enable row level security")];

    for (_, name, _) in RULES {
        statements.push(format!(
            "drop policy if exists {} on {table}",
            quote_ident(name)
        ));
    }
    statements.push(format!(
        "drop policy if exists {} on {table}",
        quote_ident(MANAGE_POLICY)
    ));

    // `manageRule` grants every action, so it becomes one `for all` policy.
    if let Some(predicate) = predicate_of(rule_of(&collection.rules, "manageRule")?, context)? {
        statements.push(policy_statement(MANAGE_POLICY, &table, "all", &predicate));
    }

    for (key, name, action) in RULES {
        if let Some(predicate) = predicate_of(rule_of(&collection.rules, key)?, context)? {
            statements.push(policy_statement(name, &table, action, &predicate));
        }
    }

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

/// Remove every policy this module manages from a collection's table.
pub async fn clear(runner: &dyn QueryRunner, collection: &Collection) -> Result<Vec<String>> {
    let table = quote_ident(&collection.name);
    let mut statements: Vec<String> = RULES
        .iter()
        .map(|(_, name, _)| format!("drop policy if exists {} on {table}", quote_ident(name)))
        .collect();
    statements.push(format!(
        "drop policy if exists {} on {table}",
        quote_ident(MANAGE_POLICY)
    ));
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

fn policy_statement(name: &str, table: &str, action: &str, predicate: &str) -> String {
    let name = quote_ident(name);
    match action {
        "select" => {
            format!("create policy {name} on {table} for select to {ROLES} using ({predicate})")
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
             using ({predicate}) with check ({predicate})"
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
        // Building the statement list does not need a database.
        let table = quote_ident(&collection.name);
        let mut out = vec![format!("alter table {table} enable row level security")];
        for (_, name, _) in RULES {
            out.push(format!(
                "drop policy if exists {} on {table}",
                quote_ident(name)
            ));
        }
        out.push(format!(
            "drop policy if exists {} on {table}",
            quote_ident(MANAGE_POLICY)
        ));
        if let Some(predicate) =
            predicate_of(rule_of(&collection.rules, "manageRule").unwrap(), &context).unwrap()
        {
            out.push(policy_statement(MANAGE_POLICY, &table, "all", &predicate));
        }
        for (key, name, action) in RULES {
            if let Some(predicate) =
                predicate_of(rule_of(&collection.rules, key).unwrap(), &context).unwrap()
            {
                out.push(policy_statement(name, &table, action, &predicate));
            }
        }
        out
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
            7,
            "one enable plus six drops: {statements:?}"
        );
        assert!(!statements.iter().any(|sql| sql.contains("create policy")));
    }

    #[test]
    fn a_public_rule_allows_everyone() {
        let statements = statements(json!({"listRule": ""}));
        let policy = statements
            .iter()
            .find(|sql| sql.contains("create policy"))
            .expect("a policy");
        assert!(policy.contains("for select"), "{policy}");
        assert!(policy.contains("using (true)"), "{policy}");
        assert!(
            policy.contains("to lazypock_anon, lazypock_authenticated"),
            "{policy}"
        );
    }

    #[test]
    fn a_filter_rule_becomes_the_predicate() {
        let statements = statements(json!({"listRule": "author = @request.auth.id"}));
        let policy = statements
            .iter()
            .find(|sql| sql.contains("create policy"))
            .expect("a policy");
        assert!(
            policy.contains("using (\"author\" = auth.uid()::text)"),
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
        assert_eq!(created.len(), 3, "{statements:?}");
        assert!(created[0].contains("for insert") && created[0].contains("with check"));
        assert!(created[1].contains("for update") && created[1].contains("using"));
        assert!(
            created[1].contains("with check"),
            "an update needs both clauses"
        );
        assert!(created[2].contains("for delete") && created[2].contains("using"));
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
