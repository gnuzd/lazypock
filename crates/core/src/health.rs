//! Process-wide record of subsystems that cannot do their job.
//!
//! Some parts of the server fail *partially*. The schema watcher and the
//! realtime bus each need a dedicated `LISTEN` connection, and if that cannot be
//! established, the rest of the node keeps serving: the API answers, records are
//! written, `/api/health` says OK and the container healthcheck goes green —
//! while realtime is dead and the schema cache is frozen at its boot snapshot.
//! That is by design (a dropped `LISTEN` must not take the node down) but it was
//! invisible, which is how a node ends up serving a collection set that stopped
//! being true hours ago.
//!
//! So a subsystem that is failing records itself here and clears the record once
//! it works. `/api/health` reports what is set; nothing else reads it.
//!
//! A process-wide registry rather than a value threaded through every
//! constructor: this is diagnostics about *the process*, and every caller is
//! describing the same process. Threading it would touch every constructor and
//! every test that builds one, to carry a diagnostic nobody reads except one
//! endpoint.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

/// Names used as keys. Constants so a typo cannot silently register a subsystem
/// that nothing ever reports.
pub const SCHEMA_WATCH: &str = "schema_watch";
pub const REALTIME: &str = "realtime";

fn registry() -> &'static Mutex<BTreeMap<&'static str, String>> {
    static REGISTRY: OnceLock<Mutex<BTreeMap<&'static str, String>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Record that `subsystem` cannot do its job, and why.
///
/// Called again on every retry, so the reason stays current rather than the
/// first failure being reported forever.
pub fn set_unavailable(subsystem: &'static str, reason: impl Into<String>) {
    if let Ok(mut map) = registry().lock() {
        map.insert(subsystem, reason.into());
    }
}

/// Record that `subsystem` is working again.
pub fn set_available(subsystem: &'static str) {
    if let Ok(mut map) = registry().lock() {
        map.remove(subsystem);
    }
}

/// Everything currently known to be unavailable, keyed by subsystem.
///
/// Empty is the healthy case.
pub fn unavailable() -> BTreeMap<&'static str, String> {
    registry().lock().map(|map| map.clone()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_subsystem_is_reported_only_while_it_is_failing() {
        // One test for the whole registry: the state is process-wide, so two
        // tests touching it would race each other.
        assert!(unavailable().is_empty(), "expected a clean start");

        set_unavailable(SCHEMA_WATCH, "cannot connect");
        let reported = unavailable();
        assert_eq!(
            reported.get(SCHEMA_WATCH).map(String::as_str),
            Some("cannot connect")
        );

        // A later failure replaces the earlier reason rather than stacking.
        set_unavailable(SCHEMA_WATCH, "still cannot connect");
        assert_eq!(
            unavailable().get(SCHEMA_WATCH).map(String::as_str),
            Some("still cannot connect")
        );

        set_available(SCHEMA_WATCH);
        assert!(
            unavailable().is_empty(),
            "expected the record to be cleared"
        );
    }
}
