//! Change events.

use lazypock_core::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// What happened to a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeOp {
    /// A record was created.
    Create,
    /// A record was updated.
    Update,
    /// A record was deleted.
    Delete,
}

impl ChangeOp {
    /// Wire name, matching the trigger's spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}

impl std::fmt::Display for ChangeOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One change to one record.
///
/// The field names mirror the `NOTIFY` payload the trigger builds, and are short
/// on purpose: a `NOTIFY` payload is capped at 8000 bytes, and the collection
/// name is the longest part of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeEvent {
    /// The `_rt_log` id, used as the SSE event id so a client can resume.
    #[serde(rename = "i")]
    pub log_id: i64,
    /// The collection (table) name.
    #[serde(rename = "c")]
    pub collection: String,
    /// What happened.
    #[serde(rename = "op")]
    pub op: ChangeOp,
    /// The record id.
    #[serde(rename = "id")]
    pub record_id: String,
    /// The connection that caused the change, when a request did.
    ///
    /// `None` for a change made by a hook, a job, or direct SQL — those are
    /// delivered to everybody, including the client that would otherwise have
    /// been suppressed.
    #[serde(rename = "o", default)]
    pub origin: Option<String>,
}

impl ChangeEvent {
    /// Parse a `NOTIFY` payload.
    pub fn parse(payload: &str) -> Result<Self> {
        serde_json::from_str(payload)
            .map_err(|err| Error::internal(format!("a realtime payload could not be read: {err}")))
    }

    /// Whether `connection_id` is the connection that caused this change.
    pub fn from_connection(&self, connection_id: &str) -> bool {
        !connection_id.is_empty() && self.origin.as_deref() == Some(connection_id)
    }

    /// Whether this event matches a subscription.
    pub fn matches(&self, collection: &str, record_id: Option<&str>) -> bool {
        self.collection == collection && record_id.is_none_or(|id| id == self.record_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(extra: &str) -> String {
        format!(r#"{{"i":7,"c":"posts","op":"update","id":"p1"{extra}}}"#)
    }

    #[test]
    fn payloads_round_trip() {
        let event = ChangeEvent::parse(&payload(r#","o":"conn1""#)).expect("parse");
        assert_eq!(event.log_id, 7);
        assert_eq!(event.collection, "posts");
        assert_eq!(event.op, ChangeOp::Update);
        assert_eq!(event.record_id, "p1");
        assert_eq!(event.origin.as_deref(), Some("conn1"));
    }

    #[test]
    fn a_missing_origin_means_everybody_sees_it() {
        let event = ChangeEvent::parse(&payload("")).expect("parse");
        assert_eq!(event.origin, None);
        assert!(!event.from_connection("conn1"));
        assert!(!event.from_connection(""));
    }

    #[test]
    fn echo_suppression_only_matches_the_originating_connection() {
        let event = ChangeEvent::parse(&payload(r#","o":"conn1""#)).expect("parse");
        assert!(event.from_connection("conn1"));
        assert!(
            !event.from_connection("conn2"),
            "another device of the same user must still receive it"
        );
    }

    #[test]
    fn subscriptions_match_the_collection_and_an_optional_record() {
        let event = ChangeEvent::parse(&payload("")).expect("parse");
        assert!(event.matches("posts", None));
        assert!(event.matches("posts", Some("p1")));
        assert!(!event.matches("posts", Some("p2")));
        assert!(!event.matches("comments", None));
    }

    #[test]
    fn every_operation_round_trips() {
        for op in [ChangeOp::Create, ChangeOp::Update, ChangeOp::Delete] {
            let raw = format!(r#"{{"i":1,"c":"posts","op":"{}","id":"p1"}}"#, op.as_str());
            assert_eq!(ChangeEvent::parse(&raw).expect("parse").op, op);
        }
    }

    #[test]
    fn a_malformed_payload_is_an_error_rather_than_a_panic() {
        for bad in ["", "not json", "{}", r#"{"i":1}"#] {
            assert!(ChangeEvent::parse(bad).is_err(), "{bad:?}");
        }
    }
}
