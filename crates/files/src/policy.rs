//! `files.rules` as row-level security policies.
//!
//! The rules are data — the `files` setting the Studio's **Settings → Media**
//! page writes — so their policies cannot live in a migration. They are rebuilt
//! from the setting at boot and whenever that setting changes. Every policy is
//! dropped first, so a rule that was removed actually stops applying.
//!
//! What each rule becomes:
//!
//! * `listRule` → a guarded `SELECT` policy. The `request.action` guard keeps a
//!   public list from widening a filtered one, exactly as a collection rule does.
//! * `deleteRule` → a `DELETE` policy plus the `SELECT` mirror the delete needs:
//!   Postgres applies the `SELECT` policies to the rows a `DELETE` reads to find
//!   its target, so without the mirror a permitted delete would match nothing.
//! * Serving is **not** rule-gated: a file id carries a random component, so the
//!   URL is the capability (roadmap F10). That is expressed as a `view` policy
//!   that allows the row, so a serve path can run under a role without needing an
//!   exemption.
//! * Attaching is an `UPDATE`, and its policy is `uploaded_by = auth.uid()` for a
//!   restricted caller. It does not need a subquery into the target record: the
//!   attach happens inside that record's write transaction, which its own rules
//!   already authorised. What the policy adds is the invariant the Rust filter
//!   used to be alone in enforcing — and, because `auth.uid()` is null for an
//!   anonymous request, it also closes the case where a caller with no account
//!   could claim a file. The "not already another record's" guard stays in
//!   `store::attach`'s `WHERE`: a policy cannot compare the old row with the new
//!   one.
//!
//! `_file_refs` gets RLS with no policies: internal bookkeeping is owner-only.
//! `_file_deletions` is left alone — the `_files_deleted` trigger inserts into it
//! while a restricted `DELETE` runs, and a policy there would break every delete.
//!
//! A malformed rule is default-deny: [`FileRules::from_setting`] carries it as
//! `SuperusersOnly`, so no policy is created for it.

use lazypock_core::error::{Error, Result};
use lazypock_db::rls::policy;
use lazypock_db::runner::QueryRunner;

use crate::rules::{context, FileRule, FileRules};

/// The roles every policy is scoped to.
///
/// The connecting role owns the tables and is left untouched: internal work and
/// superusers run as it and bypass the policies.
const ROLES: &str = "lazypock_anon, lazypock_authenticated";

/// Every policy this module creates and drops.
const POLICIES: &[&str] = &[
    "lazypock_files_list",
    "lazypock_files_view",
    "lazypock_files_delete",
    "lazypock_files_delete_select",
    "lazypock_files_attach",
    "lazypock_files_attach_select",
];

/// Enable RLS on the file tables and rebuild the policies from `rules`.
///
/// Returns the statements it ran, which is what the tests assert against.
pub async fn apply(runner: &dyn QueryRunner, rules: &FileRules) -> Result<Vec<String>> {
    let statements = statements(rules)?;

    runner
        .run_batch_execute(&statements.join(";\n"))
        .await
        .map_err(|err| Error::invalid(format!("cannot apply the file rules: {err}")))?;

    tracing::debug!(
        policies = statements.len(),
        "file row-level security applied"
    );

    Ok(statements)
}

/// Build the statements that (re)create the file policies.
///
/// Split out from [`apply`] so the generated SQL can be asserted without a
/// database.
pub fn statements(rules: &FileRules) -> Result<Vec<String>> {
    let mut statements = vec![
        "alter table _files enable row level security".to_string(),
        "alter table _file_refs enable row level security".to_string(),
    ];

    for name in POLICIES {
        statements.push(format!("drop policy if exists {name} on _files"));
    }

    // Serving is a capability URL, not a rule (see the module note).
    statements.push(format!(
        "create policy lazypock_files_view on _files for select to {ROLES} \
         using (current_setting('request.action', true) = 'view')"
    ));

    if let Some(predicate) = predicate_of(&rules.list)? {
        statements.push(format!(
            "create policy lazypock_files_list on _files for select to {ROLES} \
             using (current_setting('request.action', true) = 'list' and ({predicate}))"
        ));
    }

    if let Some(predicate) = predicate_of(&rules.delete)? {
        statements.push(format!(
            "create policy lazypock_files_delete on _files for delete to {ROLES} \
             using ({predicate})"
        ));
        statements.push(format!(
            "create policy lazypock_files_delete_select on _files for select to {ROLES} \
             using (current_setting('request.action', true) = 'delete' and ({predicate}))"
        ));
    }

    // Attaching is an update of an upload the caller owns. `auth.uid()` is null for an
    // anonymous request, so both halves deny it, which is the point: a file cannot be
    // claimed by an account that never uploaded it.
    statements.push(format!(
        "create policy lazypock_files_attach on _files for update to {ROLES} \
         using (uploaded_by = auth.uid()) with check (uploaded_by = auth.uid())"
    ));
    // The `SELECT` a write needs to find its row, keyed to the update action so it
    // cannot widen a list or a view.
    statements.push(format!(
        "create policy lazypock_files_attach_select on _files for select to {ROLES} \
         using (current_setting('request.action', true) = 'update' and uploaded_by = auth.uid())"
    ));

    Ok(statements)
}

/// `None` is default-deny (a superusers-only rule): no policy is created.
fn predicate_of(rule: &FileRule) -> Result<Option<String>> {
    Ok(match rule {
        FileRule::SuperusersOnly => None,
        FileRule::Public => Some("true".to_string()),
        FileRule::Filter(expression) => Some(policy::predicate(expression, &context())?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::FileRule;
    use lazypock_db::query::filter;

    fn rules(list: FileRule, delete: FileRule) -> FileRules {
        FileRules { list, delete }
    }

    #[test]
    fn a_superusers_only_rule_creates_no_policy() {
        // RLS is on and there is no matching policy, which is the default deny.
        let statements = statements(&rules(FileRule::SuperusersOnly, FileRule::SuperusersOnly))
            .expect("statements");

        assert!(statements
            .iter()
            .any(|sql| sql.contains("enable row level security")));
        assert!(!statements
            .iter()
            .any(|sql| sql.contains("create policy lazypock_files_list")));
        assert!(!statements
            .iter()
            .any(|sql| sql.contains("create policy lazypock_files_delete")));
        // Serving is still allowed: the URL is the capability.
        assert!(statements
            .iter()
            .any(|sql| sql.contains("create policy lazypock_files_view")));
    }

    #[test]
    fn a_public_rule_creates_a_policy_without_a_predicate() {
        let statements =
            statements(&rules(FileRule::Public, FileRule::Public)).expect("statements");

        assert!(statements
            .iter()
            .any(|sql| sql.contains("create policy lazypock_files_list")
                && sql.contains("and (true)")));
        assert!(statements
            .iter()
            .any(|sql| sql.contains("create policy lazypock_files_delete")));
        assert!(statements
            .iter()
            .any(|sql| sql.contains("create policy lazypock_files_delete_select")));
    }

    #[test]
    fn a_filter_rule_is_inlined_and_action_guarded() {
        let filter = FileRule::Filter(Box::new(
            filter::parse("uploaded_by = @request.auth.id").expect("filter"),
        ));
        let statements = statements(&rules(filter.clone(), filter)).expect("statements");

        let list = statements
            .iter()
            .find(|sql| sql.contains("create policy lazypock_files_list"))
            .expect("list policy");
        assert!(list.contains("current_setting('request.action', true) = 'list'"));
        assert!(list.contains("auth.uid()"));

        let delete = statements
            .iter()
            .find(|sql| sql.contains("create policy lazypock_files_delete "))
            .expect("delete policy");
        assert!(delete.contains("auth.uid()"));
    }

    #[test]
    fn attaching_is_owner_only_and_closes_the_anonymous_case() {
        let statements =
            statements(&rules(FileRule::Public, FileRule::Public)).expect("statements");

        let attach = statements
            .iter()
            .find(|sql| sql.contains("create policy lazypock_files_attach "))
            .expect("attach policy");
        assert!(attach.contains("for update"));
        assert!(attach.contains("using (uploaded_by = auth.uid())"));
        assert!(attach.contains("with check (uploaded_by = auth.uid())"));

        let mirror = statements
            .iter()
            .find(|sql| sql.contains("create policy lazypock_files_attach_select"))
            .expect("attach select mirror");
        assert!(mirror.contains("current_setting('request.action', true) = 'update'"));
        assert!(mirror.contains("uploaded_by = auth.uid()"));
    }

    #[test]
    fn every_policy_is_dropped_before_it_is_recreated() {
        // A removed rule must stop applying, so the rebuild is unconditional.
        let statements =
            statements(&rules(FileRule::Public, FileRule::Public)).expect("statements");
        for name in POLICIES {
            assert!(
                statements
                    .iter()
                    .any(|sql| sql == &format!("drop policy if exists {name} on _files")),
                "{name} is dropped"
            );
        }
    }
}
