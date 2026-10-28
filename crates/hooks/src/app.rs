//! `$app`: the database and mailer a hook may reach, under the caller's rules.
//!
//! A hook runs on the hook worker's own thread, so it cannot use the triggering
//! request's transaction: that transaction lives on the request's connection, which
//! belongs to a different thread. `$app` therefore opens its own connection — and
//! that is exactly why the rules have to travel with it.
//!
//! Every operation runs under the [request context](lazypock_db::hooks::HookContext)
//! the triggering request built: anonymous for an anonymous write, that record's
//! identity and claims for a logged-in one, and the owning role only when a superuser
//! made the request. A hook cannot use `$app` to see or write what its caller could
//! not, and an anonymous write cannot be laundered into a privileged one.
//!
//! Two further rules, both fail-closed:
//!
//! * **A write to the collection whose hook is running is refused.** The outer write
//!   has not committed yet, so a second write to the same collection would act on a
//!   row the transaction is still holding — a re-entrancy the hook cannot reason
//!   about, and a deadlock waiting to happen.
//! * **A `$app` write does not itself trigger hooks.** Nested dispatch would need a
//!   depth limit, a deadline shared across threads and an error story for the outer
//!   record; leaving it out makes recursion structurally impossible rather than
//!   bounded.
//!
//! The implementation lives in the API crate, behind [`AppHost`]: this crate owns the
//! JavaScript side and the rules that can be checked without a database, and the API
//! layer owns the queries.

use std::sync::Arc;

use lazypock_core::error::{Error, Result};
use lazypock_db::hooks::HookContext;
use serde_json::Value;

/// One `$app` operation, as a hook asked for it.
#[derive(Debug, Clone, PartialEq)]
pub enum AppOp {
    /// Read one record by id.
    FindRecordById { collection: String, id: String },
    /// Create or update a record. An `id` on the record means update.
    Save { collection: String, record: Value },
    /// Delete one record by id.
    Delete { collection: String, id: String },
    /// Queue an email. The message is the same shape `POST /api/mail` takes.
    MailerSend { message: Value },
}

impl AppOp {
    /// Read an operation out of the JSON the binding passes in.
    pub fn from_json(payload: &Value) -> Result<Self> {
        let text = |key: &str| -> Result<String> {
            payload
                .get(key)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .ok_or_else(|| Error::invalid(format!("$app: `{key}` is required")))
        };
        let op = payload
            .get("op")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::invalid("$app: `op` is required"))?;

        match op {
            "findRecordById" => Ok(Self::FindRecordById {
                collection: text("collection")?,
                id: text("id")?,
            }),
            "save" => Ok(Self::Save {
                collection: text("collection")?,
                record: payload
                    .get("record")
                    .cloned()
                    .ok_or_else(|| Error::invalid("$app.save: a record is required"))?,
            }),
            "delete" => Ok(Self::Delete {
                collection: text("collection")?,
                id: text("id")?,
            }),
            "mailerSend" => Ok(Self::MailerSend {
                message: payload
                    .get("message")
                    .cloned()
                    .ok_or_else(|| Error::invalid("$app.mailer.send: a message is required"))?,
            }),
            other => Err(Error::invalid(format!(
                "$app: {other:?} is not an operation (findRecordById, save, delete, mailerSend)"
            ))),
        }
    }

    /// The name a refusal or a log line uses.
    pub fn name(&self) -> &'static str {
        match self {
            Self::FindRecordById { .. } => "findRecordById",
            Self::Save { .. } => "save",
            Self::Delete { .. } => "delete",
            Self::MailerSend { .. } => "mailerSend",
        }
    }

    /// The collection the operation touches, when it names one.
    pub fn collection(&self) -> Option<&str> {
        match self {
            Self::FindRecordById { collection, .. }
            | Self::Save { collection, .. }
            | Self::Delete { collection, .. } => Some(collection),
            Self::MailerSend { .. } => None,
        }
    }

    /// Whether this operation writes, and so must not target the collection whose
    /// hook is running.
    fn is_write_to(&self, collection: &str) -> bool {
        match self {
            Self::Save {
                collection: target, ..
            }
            | Self::Delete {
                collection: target, ..
            } => target == collection,
            Self::FindRecordById { .. } | Self::MailerSend { .. } => false,
        }
    }
}

/// What the API layer must provide for `$app` to exist.
///
/// Implemented in `lazypock-api`, which owns the pool, the schema cache and the job
/// queue. The hooks crate stays free of `sqlx` and of the record layer, so the
/// JavaScript runtime cannot grow a second, subtly different write path.
pub trait AppHost: Send + Sync {
    /// Run `op` under `context`, answering with the operation's value.
    fn call(&self, context: &HookContext, op: AppOp) -> Result<Value>;
}

/// The `$app` capability, or the absence of it.
#[derive(Clone)]
pub struct HookApp {
    host: Option<Arc<dyn AppHost>>,
}

impl HookApp {
    /// `$app` backed by `host`.
    pub fn new(host: Arc<dyn AppHost>) -> Self {
        Self { host: Some(host) }
    }

    /// No `$app`: every call is refused.
    ///
    /// A deployment that has not wired a host gets an error naming the reason rather
    /// than an `undefined` a hook would discover at runtime.
    pub fn disabled() -> Self {
        Self { host: None }
    }

    /// Whether any operation is possible.
    pub fn is_enabled(&self) -> bool {
        self.host.is_some()
    }

    /// Run one operation, applying the rules that need no database first.
    pub fn call(&self, context: &HookContext, op: AppOp) -> Result<Value> {
        let Some(host) = &self.host else {
            return Err(Error::invalid(
                "$app is not available: this deployment has not wired a hook host",
            ));
        };

        if let Some(collection) = op.collection() {
            if op.is_write_to(&context.collection) {
                return Err(Error::invalid(format!(
                    "$app.{}: a write to {:?} from inside its own hook is refused — the outer \
                     write has not committed yet; do it with the record the hook was given",
                    op.name(),
                    collection
                )));
            }
        }

        host.call(context, op)
    }
}

impl std::fmt::Debug for HookApp {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HookApp")
            .field("enabled", &self.is_enabled())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    /// A host that records what reached it.
    struct Recorder(Mutex<Vec<(HookContext, AppOp)>>);

    impl AppHost for Recorder {
        fn call(&self, context: &HookContext, op: AppOp) -> Result<Value> {
            self.0
                .lock()
                .expect("not poisoned")
                .push((context.clone(), op));
            Ok(json!({ "seen": true }))
        }
    }

    fn app() -> (HookApp, Arc<Recorder>) {
        let recorder = Arc::new(Recorder(Mutex::new(Vec::new())));
        let app = HookApp::new(Arc::clone(&recorder) as Arc<dyn AppHost>);
        (app, recorder)
    }

    #[test]
    fn an_operation_is_read_out_of_the_json_the_binding_passes() {
        let op = AppOp::from_json(&json!({
            "op": "findRecordById",
            "collection": "posts",
            "id": "p1",
        }))
        .expect("parsed");
        assert_eq!(
            op,
            AppOp::FindRecordById {
                collection: "posts".into(),
                id: "p1".into(),
            }
        );
        assert_eq!(op.collection(), Some("posts"));
        assert_eq!(op.name(), "findRecordById");

        let save = AppOp::from_json(&json!({
            "op": "save",
            "collection": "orders",
            "record": { "id": "o1" },
        }))
        .expect("parsed");
        assert_eq!(
            save,
            AppOp::Save {
                collection: "orders".into(),
                record: json!({ "id": "o1" }),
            }
        );

        // A missing field, an unknown operation, and a save without a record.
        assert!(AppOp::from_json(&json!({ "op": "save", "collection": "x" })).is_err());
        assert!(AppOp::from_json(&json!({ "op": "dropTable", "collection": "x" })).is_err());
        assert!(AppOp::from_json(&json!({ "op": "save" })).is_err());
        assert!(AppOp::from_json(&json!({ "op": "findRecordById", "collection": "x" })).is_err());
    }

    #[test]
    fn nothing_is_reachable_when_no_host_is_wired() {
        let app = HookApp::disabled();
        assert!(!app.is_enabled());
        let err = app
            .call(
                &HookContext::default(),
                AppOp::Delete {
                    collection: "posts".into(),
                    id: "p1".into(),
                },
            )
            .expect_err("refused");
        assert!(format!("{err}").contains("not available"), "{err}");
    }

    #[test]
    fn a_write_to_the_collection_whose_hook_is_running_is_refused() {
        let (app, recorder) = app();
        let context = HookContext::default();

        let err = app
            .call(
                &HookContext::new("posts", context.request.clone()),
                AppOp::Save {
                    collection: "posts".into(),
                    record: json!({ "title": "again" }),
                },
            )
            .expect_err("refused");
        assert!(format!("{err}").contains("its own hook"), "{err}");
        assert!(
            recorder.0.lock().expect("not poisoned").is_empty(),
            "a refused operation never reaches the host"
        );

        // A delete of the same collection is the same re-entrancy, and a read is not
        // a write at all.
        assert!(app
            .call(
                &HookContext::new("posts", HookContext::default().request),
                AppOp::Delete {
                    collection: "posts".into(),
                    id: "p1".into(),
                },
            )
            .is_err());
        app.call(
            &HookContext::new("posts", HookContext::default().request),
            AppOp::FindRecordById {
                collection: "posts".into(),
                id: "p1".into(),
            },
        )
        .expect("a read of the same collection is fine");
    }

    #[test]
    fn another_collection_is_written_with_the_context_intact() {
        let (app, recorder) = app();
        let context = HookContext::anonymous("orders");
        app.call(
            &context,
            AppOp::Save {
                collection: "audit".into(),
                record: json!({ "note": "order created" }),
            },
        )
        .expect("allowed");

        let seen = recorder.0.lock().expect("not poisoned");
        assert_eq!(seen.len(), 1);
        // The context that reached the host is the one the trigger carried.
        assert_eq!(seen[0].0, context);
        assert_eq!(seen[0].0.collection, "orders");
        assert_eq!(
            seen[0].0.request.role,
            lazypock_db::rls::context::Role::Anon
        );
    }

    #[test]
    fn the_rules_that_need_no_database_hold_for_mailer_send() {
        let (app, _) = app();
        app.call(
            &HookContext::anonymous("posts"),
            AppOp::MailerSend {
                message: json!({ "to": "a@b.c", "template": "receipt" }),
            },
        )
        .expect("mail is not a write to a collection");
    }
}
