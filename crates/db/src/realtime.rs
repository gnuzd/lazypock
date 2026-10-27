//! Realtime publishing: the per-collection switch, the instance-wide switch, and
//! the change log's retention.
//!
//! Both switches are enforced by adding or **dropping the change-event trigger**,
//! never by filtering on the publishing side. That is not an optimisation detail:
//! the trigger is what makes a write cost anything at all, and a write that comes
//! from `psql`, a migration, a cron or another service never passes through the
//! server. A switch that only suppressed delivery would leave every one of those
//! writes paying for an `_rt_log` row and a `NOTIFY`.
//!
//! The two switches compose: a collection publishes when the instance-wide switch
//! is on **and** the collection did not opt out.

use serde_json::Value;

use lazypock_core::error::{Error, Result};

use crate::collections::Collection;
use crate::ddl;
use crate::pool::db_error;
use crate::runner::QueryRunner;
use crate::types::CollectionType;

/// The `_settings` key the instance-wide switch is stored under.
pub const SETTING: &str = "realtime";

/// The per-collection option, stored in a collection's `options`.
pub const OPTION: &str = "realtime";

/// Whether one collection publishes, given the instance-wide switch.
pub fn is_published(collection: &Collection, globally_enabled: bool) -> bool {
    globally_enabled && collection.realtime_enabled()
}

/// Whether the instance-wide switch is on.
///
/// Absent means on. An installation that never touched the setting publishes, which
/// is exactly what it did before the switch existed — turning realtime off must be
/// something an operator asks for, never something they discover.
pub async fn global_enabled(runner: &dyn QueryRunner) -> Result<bool> {
    let rows = runner
        .run_query("select value from _settings where key = $1", &[&SETTING])
        .await
        .map_err(db_error)?;
    Ok(rows
        .first()
        .map(|row| enabled_from_setting(&row.get::<_, Value>(0)))
        .unwrap_or(true))
}

/// Read the switch out of the stored setting.
///
/// A boolean is the documented shape; `{"enabled": …}` is accepted too, because a
/// settings form that groups instance-wide toggles writes objects. Anything else
/// means "on": a malformed setting must not silently stop every collection from
/// publishing.
pub fn enabled_from_setting(value: &Value) -> bool {
    match value {
        Value::Bool(enabled) => *enabled,
        Value::Object(object) => object
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        _ => true,
    }
}

/// The statements that bring one collection's trigger in line with both switches.
///
/// Pure, so the decision is testable without a database.
pub fn statements_for(collection: &Collection, globally_enabled: bool) -> Result<Vec<String>> {
    // A view is not a table, so it has nothing to attach a trigger to.
    if collection.collection_type == CollectionType::View {
        return Ok(Vec::new());
    }
    ddl::realtime_trigger_statements(&collection.name, is_published(collection, globally_enabled))
}

/// Bring one collection's trigger in line with both switches.
///
/// Returns the statements it ran, which is what the tests assert against.
pub async fn apply(
    runner: &dyn QueryRunner,
    collection: &Collection,
    globally_enabled: bool,
) -> Result<Vec<String>> {
    let statements = statements_for(collection, globally_enabled)?;
    if statements.is_empty() {
        return Ok(statements);
    }

    runner
        .run_batch_execute(&statements.join(";\n"))
        .await
        .map_err(|err| {
            Error::invalid(format!(
                "cannot set the realtime trigger of {:?}: {err}",
                collection.name
            ))
        })?;

    tracing::debug!(
        collection = %collection.name,
        published = is_published(collection, globally_enabled),
        "realtime trigger set"
    );

    Ok(statements)
}

/// Apply the instance-wide switch to every collection that already exists.
///
/// Called when the setting changes: the setting alone would only affect
/// collections created afterwards, which would make the switch look broken on the
/// installation it was meant to fix. Every collection's statements run in the
/// caller's transaction, so a failure leaves the setting unsaved rather than half
/// applied — and the error names the collection that refused, because "the switch
/// did not save" is not actionable on its own.
pub async fn apply_global(runner: &dyn QueryRunner, enabled: bool) -> Result<Vec<String>> {
    let rows = runner
        .run_query(
            "select name, type, options from _collections order by name",
            &[],
        )
        .await
        .map_err(db_error)?;

    let mut statements = Vec::new();
    for row in &rows {
        let name: String = row.get("name");
        let kind = CollectionType::parse(&row.get::<_, String>("type"))?;
        if kind == CollectionType::View {
            continue;
        }
        let options: Value = row.get("options");
        let collection_enabled = options.get(OPTION).and_then(Value::as_bool).unwrap_or(true);

        let collection_statements =
            ddl::realtime_trigger_statements(&name, enabled && collection_enabled)?;
        runner
            .run_batch_execute(&collection_statements.join(";\n"))
            .await
            .map_err(|err| {
                Error::invalid(format!(
                    "cannot set the realtime trigger of {name:?} while applying the \
                     instance-wide switch: {err}"
                ))
            })?;
        statements.extend(collection_statements);
    }

    Ok(statements)
}

/// Delete change-log rows older than `days`.
///
/// The log is what makes `Last-Event-ID` resume possible, and it is written by the
/// trigger, so it grows with **every** write — including writes that never went
/// through the server. Without a bound a bulk `UPDATE` adds a row per row, for
/// ever. `days <= 0` keeps everything, which is the pre-existing behaviour and
/// stays available as an explicit choice.
pub async fn prune_log(runner: &dyn QueryRunner, days: i64) -> Result<u64> {
    if days <= 0 {
        return Ok(0);
    }
    let days = i32::try_from(days).unwrap_or(i32::MAX);
    runner
        .run_execute(
            "delete from _rt_log where created < now() - make_interval(days => $1::int)",
            &[&days],
        )
        .await
        .map_err(db_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::Field;
    use serde_json::json;
    use std::time::SystemTime;

    fn collection(options: Value) -> Collection {
        Collection {
            id: "c1".into(),
            name: "posts".into(),
            collection_type: CollectionType::Base,
            options,
            rules: json!({}),
            fields: Vec::<Field>::new(),
            created: SystemTime::now(),
            updated: SystemTime::now(),
        }
    }

    #[test]
    fn a_collection_publishes_unless_it_opts_out() {
        assert!(collection(json!({})).realtime_enabled());
        assert!(collection(json!({ "realtime": true })).realtime_enabled());
        assert!(!collection(json!({ "realtime": false })).realtime_enabled());
    }

    #[test]
    fn the_instance_switch_is_on_when_it_was_never_set() {
        assert!(enabled_from_setting(&Value::Null));
        assert!(enabled_from_setting(&json!({})));
        assert!(enabled_from_setting(&json!(true)));
        assert!(!enabled_from_setting(&json!(false)));
        assert!(!enabled_from_setting(&json!({ "enabled": false })));
    }

    #[test]
    fn the_two_switches_compose() {
        let on = collection(json!({}));
        let off = collection(json!({ "realtime": false }));
        assert!(is_published(&on, true));
        assert!(!is_published(&on, false));
        assert!(!is_published(&off, true));
        assert!(!is_published(&off, false));
    }

    #[test]
    fn a_view_has_no_trigger_to_set() {
        let mut view = collection(json!({}));
        view.collection_type = CollectionType::View;
        // Nothing to attach to, and saying so is better than a DDL error.
        assert_eq!(statements_for(&view, true).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn opting_out_drops_the_trigger() {
        let statements = statements_for(&collection(json!({ "realtime": false })), true).unwrap();
        assert_eq!(statements.len(), 1, "{statements:?}");
        assert!(statements[0].starts_with("drop trigger if exists"));
        assert!(
            !statements.iter().any(|sql| sql.contains("create trigger")),
            "a collection that opted out must not get a trigger back: {statements:?}"
        );
    }

    #[test]
    fn opting_in_recreates_the_trigger_idempotently() {
        let statements = statements_for(&collection(json!({})), true).unwrap();
        assert_eq!(statements.len(), 2, "{statements:?}");
        assert!(statements[0].starts_with("drop trigger if exists"));
        assert!(statements[1].contains("create trigger lazypock_change"));
        assert!(statements[1].contains("execute function lazypock_change_event()"));
    }

    #[test]
    fn the_instance_switch_wins_over_a_collection_that_did_not_opt_out() {
        let statements = statements_for(&collection(json!({})), false).unwrap();
        assert_eq!(statements.len(), 1, "{statements:?}");
        assert!(statements[0].starts_with("drop trigger if exists"));
    }
}
