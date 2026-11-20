//! A collection's rules as Postgres policies.
//!
//! Same semantics as the rest of lazypock (PLAN.md 6.2): a rule is absent
//! (nobody but the owner), the empty string (anyone) or a filter. Policies are
//! rebuilt from scratch on every change, so a removed rule actually stops
//! applying; with RLS on and no matching policy the restricted roles see nothing.
//!
//! `listRule`/`viewRule` both become `SELECT` policies, kept apart with a
//! `request.action` guard because Postgres OR-combines permissive policies.
//! Updates and deletes get a guarded `SELECT` mirror of their own rule, since
//! Postgres applies `SELECT` policies to the rows a write has to find first.

use crate::error::{self, PgExt};
use crate::filter::{self, Mode};
use crate::schema::{Collection, Rules};
use tokio_postgres::GenericClient;

use lazypock_core::Result;

const ROLES: &str = "lazypock_anon, lazypock_authenticated";
const MANAGE_POLICY: &str = "lazypock_manage";

/// (rule key, policy name, action, request.action guard)
const RULES: &[(&str, &str, &str, Option<&str>)] = &[
    ("listRule", "lazypock_list", "select", Some("list")),
    ("viewRule", "lazypock_view", "select", Some("view")),
    ("createRule", "lazypock_create", "insert", None),
    ("updateRule", "lazypock_update", "update", None),
    ("deleteRule", "lazypock_delete", "delete", None),
];

const SELECT_MIRRORS: &[(&str, &str, &str)] = &[
    ("updateRule", "lazypock_update_select", "update"),
    ("deleteRule", "lazypock_delete_select", "delete"),
];

fn policy_names() -> impl Iterator<Item = &'static str> {
    RULES
        .iter()
        .map(|r| r.1)
        .chain(SELECT_MIRRORS.iter().map(|m| m.1))
        .chain(std::iter::once(MANAGE_POLICY))
}

fn rule_text<'a>(rules: &'a Rules, key: &str) -> Option<&'a str> {
    match key {
        "listRule" => rules.list_rule.as_deref(),
        "viewRule" => rules.view_rule.as_deref(),
        "createRule" => rules.create_rule.as_deref(),
        "updateRule" => rules.update_rule.as_deref(),
        "deleteRule" => rules.delete_rule.as_deref(),
        "manageRule" => rules.manage_rule.as_deref(),
        _ => None,
    }
}

/// Predicate SQL for one rule, `None` when the rule is absent. `""` = everyone.
pub fn rule_predicate(c: &Collection, rule: Option<&str>) -> Result<Option<String>> {
    let Some(text) = rule else { return Ok(None) };
    if text.trim().is_empty() {
        return Ok(Some("true".into()));
    }
    let ast = filter::parse(text)?;
    Ok(Some(filter::render_inline(filter::compile(
        c,
        &ast,
        Mode::Inline,
    )?)))
}

/// Dry-run every rule (used when a collection or its rules are saved).
pub fn validate_rules(c: &Collection) -> Result<()> {
    statements(c).map(|_| ())
}

/// The statements that (re)create a collection's policies.
pub fn statements(c: &Collection) -> Result<Vec<String>> {
    let table = c.name.quoted();
    let mut out = vec![format!("alter table {table} enable row level security")];
    for name in policy_names() {
        out.push(format!("drop policy if exists \"{name}\" on {table}"));
    }
    if let Some(p) = rule_predicate(c, rule_text(&c.rules, "manageRule"))? {
        out.push(policy(MANAGE_POLICY, &table, "all", &p, None));
    }
    for (key, name, action, guard) in RULES {
        if let Some(p) = rule_predicate(c, rule_text(&c.rules, key))? {
            out.push(policy(name, &table, action, &p, *guard));
        }
    }
    for (key, name, action) in SELECT_MIRRORS {
        if let Some(p) = rule_predicate(c, rule_text(&c.rules, key))? {
            out.push(policy(name, &table, "select", &p, Some(action)));
        }
    }
    Ok(out)
}

fn policy(name: &str, table: &str, action: &str, predicate: &str, guard: Option<&str>) -> String {
    // The guard value comes from a closed set, never from a request.
    let guarded = match guard {
        Some(a) => format!("current_setting('request.action', true) = '{a}' and ({predicate})"),
        None => predicate.to_string(),
    };
    match action {
        "select" => {
            format!("create policy \"{name}\" on {table} for select to {ROLES} using ({guarded})")
        }
        "insert" => format!(
            "create policy \"{name}\" on {table} for insert to {ROLES} with check ({predicate})"
        ),
        "update" => format!(
            "create policy \"{name}\" on {table} for update to {ROLES} using ({predicate}) with check ({predicate})"
        ),
        "delete" => {
            format!("create policy \"{name}\" on {table} for delete to {ROLES} using ({predicate})")
        }
        _ => format!(
            "create policy \"{name}\" on {table} for all to {ROLES} using ({guarded}) with check ({guarded})"
        ),
    }
}

/// Apply a collection's rules.
pub async fn apply<C: GenericClient>(client: &C, c: &Collection) -> Result<()> {
    let sql = statements(c)?.join(";\n");
    client.batch_execute(&sql).await.map_err(|e| {
        tracing::warn!(collection = %c.name, error = ?e, "applying row-level security failed");
        error::invalid_schema(format!("cannot apply the rules of {}", c.name))
    })
}

/// Remove every managed policy and switch RLS off (the owner stays unrestricted).
pub async fn clear<C: GenericClient>(client: &C, c: &Collection) -> Result<()> {
    let table = c.name.quoted();
    let mut sql: Vec<String> = policy_names()
        .map(|n| format!("drop policy if exists \"{n}\" on {table}"))
        .collect();
    sql.push(format!("alter table {table} disable row level security"));
    client.batch_execute(&sql.join(";\n")).await.pg()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Field;
    use crate::types::{CollectionType, FieldType};

    fn coll(rules: Rules) -> Collection {
        let mut c = Collection::new("posts", CollectionType::Base).unwrap();
        c.fields = vec![Field::new("author", FieldType::Text).unwrap()];
        c.rules = rules;
        c
    }

    fn policies(c: &Collection) -> Vec<String> {
        statements(c)
            .unwrap()
            .into_iter()
            .filter(|s| s.starts_with("create policy"))
            .collect()
    }

    #[test]
    fn no_rules_means_rls_on_and_no_policies() {
        let s = statements(&coll(Rules::default())).unwrap();
        assert_eq!(s[0], "alter table \"posts\" enable row level security");
        assert!(!s.iter().any(|x| x.starts_with("create policy")));
        assert_eq!(s.iter().filter(|x| x.starts_with("drop policy")).count(), 8);
    }

    #[test]
    fn public_rule_and_guards() {
        let c = coll(Rules {
            list_rule: Some(String::new()),
            view_rule: Some("author = @request.auth.id".into()),
            ..Rules::default()
        });
        let p = policies(&c);
        assert_eq!(p.len(), 2);
        assert!(
            p[0].contains("\"lazypock_list\"")
                && p[0].contains("request.action', true) = 'list' and (true)"),
            "{}",
            p[0]
        );
        assert!(
            p[1].contains("\"lazypock_view\"")
                && p[1].contains("= 'view' and (\"author\" = (auth.uid()::text))"),
            "{}",
            p[1]
        );
    }

    #[test]
    fn update_and_delete_get_guarded_select_mirrors() {
        let rule = Some("author = @request.auth.id".to_string());
        let c = coll(Rules {
            update_rule: rule.clone(),
            delete_rule: rule,
            ..Rules::default()
        });
        let p = policies(&c);
        assert_eq!(p.len(), 4);
        assert!(p.iter().any(|x| x.contains("\"lazypock_update\"")
            && x.contains("for update")
            && x.contains("with check")));
        assert!(p.iter().any(|x| x.contains("\"lazypock_update_select\"")
            && x.contains("for select")
            && x.contains("= 'update' and")));
        assert!(
            p.iter()
                .any(|x| x.contains("\"lazypock_delete_select\"") && x.contains("= 'delete' and"))
        );
    }

    #[test]
    fn manage_rule_is_one_unguarded_for_all_policy() {
        let c = coll(Rules {
            manage_rule: Some("@request.auth.id = 'admin'".into()),
            ..Rules::default()
        });
        let p = policies(&c);
        assert_eq!(p.len(), 1);
        assert!(
            p[0].contains("\"lazypock_manage\"")
                && p[0].contains("for all")
                && !p[0].contains("request.action")
        );
    }

    #[test]
    fn bad_rules_fail_at_save_time() {
        for bad in ["nope = 1", "author = 'x'; DROP TABLE posts", "author ="] {
            let c = coll(Rules {
                list_rule: Some(bad.into()),
                ..Rules::default()
            });
            assert!(validate_rules(&c).is_err(), "{bad}");
        }
    }
}
