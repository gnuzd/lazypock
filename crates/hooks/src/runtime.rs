//! The QuickJS runtime and its limits.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use lazypock_core::error::{Error, Result};
use rquickjs::{Context, Function, Runtime};
use serde_json::Value;

use crate::http::{HookHttp, HttpCall};

/// The bootstrap that defines the registration helpers, kept as a real `.js` file.
const BOOTSTRAP: &str = include_str!("bootstrap.js");

/// The limits a hook runs under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HookLimits {
    /// Heap a context may allocate.
    pub memory_bytes: usize,
    /// Stack a context may use.
    pub stack_bytes: usize,
    /// How long one dispatch may take before it is interrupted.
    pub deadline: Duration,
}

impl Default for HookLimits {
    fn default() -> Self {
        // `PLAN.md` section 11.1: 16-32 MB per context.
        Self {
            memory_bytes: 32 * 1024 * 1024,
            stack_bytes: 512 * 1024,
            deadline: Duration::from_secs(2),
        }
    }
}

/// Everything a hook runtime is built with: its limits, and the outbound HTTP it is
/// allowed to make.
///
/// `From<HookLimits>` means a caller that does not care about `$http` keeps writing
/// `HookRuntime::new(HookLimits::default())` — and gets a runtime where every `$http`
/// call is refused, which is the right default rather than no `$http` at all.
#[derive(Debug, Clone)]
pub struct HookSetup {
    pub limits: HookLimits,
    pub http: Arc<HookHttp>,
}

impl Default for HookSetup {
    fn default() -> Self {
        let limits = HookLimits::default();
        Self {
            limits,
            http: Arc::new(HookHttp::disabled(limits.deadline)),
        }
    }
}

impl HookSetup {
    /// A setup whose `$http` may reach `http`, within `limits`.
    pub fn new(limits: HookLimits, http: HookHttp) -> Self {
        Self {
            limits,
            http: Arc::new(http),
        }
    }
}

impl From<HookLimits> for HookSetup {
    fn from(limits: HookLimits) -> Self {
        Self {
            limits,
            http: Arc::new(HookHttp::disabled(limits.deadline)),
        }
    }
}

/// A record event a hook can be registered for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordEvent {
    /// Before a record is created, inside the transaction.
    BeforeCreate,
    /// After a record was created.
    AfterCreate,
    /// Before a record is updated.
    BeforeUpdate,
    /// After a record was updated.
    AfterUpdate,
    /// Before a record is deleted.
    BeforeDelete,
    /// After a record was deleted.
    AfterDelete,
}

impl RecordEvent {
    /// The name a hook file registers against.
    pub fn key(self) -> &'static str {
        match self {
            RecordEvent::BeforeCreate => "recordBeforeCreate",
            RecordEvent::AfterCreate => "recordAfterCreate",
            RecordEvent::BeforeUpdate => "recordBeforeUpdate",
            RecordEvent::AfterUpdate => "recordAfterUpdate",
            RecordEvent::BeforeDelete => "recordBeforeDelete",
            RecordEvent::AfterDelete => "recordAfterDelete",
        }
    }

    /// Whether this event runs inside the database transaction.
    pub fn is_before(self) -> bool {
        matches!(
            self,
            RecordEvent::BeforeCreate | RecordEvent::BeforeUpdate | RecordEvent::BeforeDelete
        )
    }

    /// Every event, which is what a caller iterating needs.
    pub fn all() -> [RecordEvent; 6] {
        [
            RecordEvent::BeforeCreate,
            RecordEvent::AfterCreate,
            RecordEvent::BeforeUpdate,
            RecordEvent::AfterUpdate,
            RecordEvent::BeforeDelete,
            RecordEvent::AfterDelete,
        ]
    }
}

/// A QuickJS context holding the registered hooks.
pub struct HookRuntime {
    runtime: Runtime,
    context: Context,
    limits: HookLimits,
    /// The outbound HTTP this runtime may make, with its rules.
    http: Arc<HookHttp>,
    /// When the current dispatch must give up. Shared with the interrupt handler,
    /// which QuickJS calls during execution.
    deadline: Arc<Mutex<Option<Instant>>>,
}

impl std::fmt::Debug for HookRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HookRuntime")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

impl HookRuntime {
    /// Create a runtime with limits applied and the bootstrap loaded.
    pub fn new(setup: impl Into<HookSetup>) -> Result<Self> {
        let HookSetup { limits, http } = setup.into();
        let runtime = Runtime::new()
            .map_err(|err| Error::internal(format!("cannot create the hook runtime: {err}")))?;
        runtime.set_memory_limit(limits.memory_bytes);
        runtime.set_max_stack_size(limits.stack_bytes);

        let deadline: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
        let watched = Arc::clone(&deadline);
        runtime.set_interrupt_handler(Some(Box::new(move || {
            // Returning true stops execution. A hook that loops forever costs one
            // request, not the server.
            let guard = watched
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.is_some_and(|deadline| Instant::now() > deadline)
        })));

        let context = Context::full(&runtime)
            .map_err(|err| Error::internal(format!("cannot create the hook context: {err}")))?;

        let runtime = Self {
            runtime,
            context,
            limits,
            http,
            deadline,
        };
        runtime.eval(BOOTSTRAP)?;
        runtime.install_http()?;
        Ok(runtime)
    }

    /// The limits in force.
    pub fn limits(&self) -> HookLimits {
        self.limits
    }

    /// Expose the one Rust binding the bootstrap builds `$http` on.
    ///
    /// The binding answers with an envelope rather than throwing, so a refusal is
    /// the same shape as a failure and the JavaScript side turns both into an
    /// `Error` with the message intact. Nothing a hook can pass reaches the network
    /// without going through [`HookHttp::call`].
    fn install_http(&self) -> Result<()> {
        let http = Arc::clone(&self.http);
        self.context.with(|ctx| {
            let binding = Function::new(ctx.clone(), move |payload: String| -> String {
                respond(&http, &payload)
            })
            .map_err(|err| Error::internal(format!("cannot register $http: {err}")))?;
            ctx.globals()
                .set("__lazypock_http", binding)
                .map_err(|err| Error::internal(format!("cannot register $http: {err}")))
        })
    }

    /// Heap currently held by the runtime, in bytes.
    ///
    /// Exposed for metrics: a hook that creeps towards its memory limit is worth
    /// seeing before it starts failing.
    pub fn memory_used(&self) -> usize {
        // QuickJS reports this as a signed count; a negative value would be a
        // bug in the runtime, so clamp rather than wrap.
        self.runtime.memory_usage().memory_used_size.max(0) as usize
    }

    /// Evaluate a hook file.
    ///
    /// A file that fails to compile leaves the runtime usable, so one bad hook
    /// does not take the others with it.
    pub fn load(&self, source: &str) -> Result<()> {
        self.eval(source)
    }

    /// How many handlers are registered for an event and collection.
    pub fn registered(&self, event: RecordEvent, collection: &str) -> Result<usize> {
        let source = format!(
            "__lazypock_count({}, {})",
            literal(event.key()),
            literal(collection)
        );
        self.deadline();
        self.context.with(|ctx| {
            let count: i64 = ctx
                .eval(source)
                .map_err(|err| js_error(&ctx, "counting hooks", err))?;
            Ok(count.max(0) as usize)
        })
    }

    /// Run the handlers for an event, returning the event they produced.
    ///
    /// The record a "before" hook returns is what the caller should then write.
    pub fn dispatch(&self, event: RecordEvent, collection: &str, payload: &Value) -> Result<Value> {
        let source = format!(
            "__lazypock_dispatch({}, {}, {})",
            literal(event.key()),
            literal(collection),
            literal(&payload.to_string()),
        );

        self.deadline();
        let rendered: String = self.context.with(|ctx| {
            ctx.eval(source)
                .map_err(|err| js_error(&ctx, "running hooks", err))
        })?;

        serde_json::from_str(&rendered)
            .map_err(|err| Error::internal(format!("a hook returned unreadable JSON: {err}")))
    }

    /// Evaluate one script and discard the value.
    fn eval(&self, source: &str) -> Result<()> {
        self.deadline();
        self.context.with(|ctx| {
            ctx.eval::<rquickjs::Value, _>(source)
                .map(|_| ())
                .map_err(|err| js_error(&ctx, "loading hooks", err))
        })
    }

    /// Arm the deadline for the call about to run.
    fn deadline(&self) {
        let deadline = Instant::now() + self.limits.deadline;
        let mut guard = self
            .deadline
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = Some(deadline);
    }
}

/// Run one `$http` call and answer with the envelope the bootstrap reads.
fn respond(http: &HookHttp, payload: &str) -> String {
    let outcome = serde_json::from_str::<Value>(payload)
        .map_err(|err| Error::invalid(format!("$http: unreadable request: {err}")))
        .and_then(|payload| HttpCall::from_json(&payload))
        .and_then(|call| http.call(&call));

    let envelope = match outcome {
        Ok(response) => {
            let mut value = response.to_json();
            value["ok"] = Value::Bool(true);
            value
        }
        Err(err) => serde_json::json!({ "ok": false, "error": err.to_string() }),
    };
    envelope.to_string()
}

/// Render a Rust string as a JavaScript string literal.
///
/// JSON string escaping is a subset of JavaScript's, so this is exact rather than
/// approximate — which matters because a hook file may contain quotes, newlines or
/// a `</script>`.
fn literal(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

/// Turn a failure into an error that reports what the hook did wrong.
///
/// The thrown value carries the message the hook author needs — "a handler
/// function is required", "a hook returned a promise" — so it is read out rather
/// than replaced with a generic "the script threw", which tells nobody anything.
fn js_error(ctx: &rquickjs::Ctx<'_>, stage: &str, error: rquickjs::Error) -> Error {
    let detail = match error {
        rquickjs::Error::Exception => thrown_message(ctx),
        other => other.to_string(),
    };
    Error::invalid(format!("{stage}: {detail}"))
}

/// The message carried by whatever the script threw.
fn thrown_message(ctx: &rquickjs::Ctx<'_>) -> String {
    let thrown = ctx.catch();
    if let Some(exception) = thrown.as_exception() {
        if let Some(message) = exception.message() {
            return message;
        }
    }
    // A script may `throw "a string"` rather than an Error.
    if let Some(text) = thrown.as_string() {
        if let Ok(text) = text.to_string() {
            return text;
        }
    }
    "a hook threw, without a message".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn runtime() -> HookRuntime {
        HookRuntime::new(HookLimits::default()).expect("runtime")
    }

    /// A resolver that always answers with one address.
    struct OneAddress(std::net::IpAddr);

    impl crate::http::Resolver for OneAddress {
        fn resolve(&self, _host: &str, _port: u16) -> Result<Vec<std::net::IpAddr>> {
            Ok(vec![self.0])
        }
    }

    /// A transport that echoes the request body's `id`, so a test can see that what
    /// the hook sent is what arrived.
    struct EchoTransport;

    impl crate::http::Transport for EchoTransport {
        fn send(&self, request: &crate::http::Prepared) -> Result<crate::http::HttpResponse> {
            let id = request
                .body
                .as_deref()
                .and_then(|body| serde_json::from_str::<Value>(body).ok())
                .and_then(|body| body.get("id").cloned())
                .unwrap_or(Value::Null);
            Ok(crate::http::HttpResponse {
                status: 200,
                headers: Vec::new(),
                body: serde_json::json!({ "echo": id }).to_string(),
            })
        }
    }

    fn http_runtime(allow: &str) -> HookRuntime {
        let limits = HookLimits::default();
        let gate = crate::http::HookHttp::with_parts(
            lazypock_core::net::HostAllowlist::parse(allow).expect("allowlist"),
            limits.deadline,
            Arc::new(OneAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::new(
                93, 184, 216, 34,
            )))),
            Arc::new(EchoTransport),
        );
        HookRuntime::new(HookSetup::new(limits, gate)).expect("runtime")
    }

    #[test]
    fn a_hook_can_call_an_allowed_host_and_read_the_answer() {
        let runtime = http_runtime("api.example.com");
        runtime
            .load(
                r#"
                onRecordAfterCreate("posts", (e) => {
                    const res = $http.post("https://api.example.com/hook", { id: e.record.id });
                    e.record.status = res.status + ":" + res.json().echo;
                });
                "#,
            )
            .expect("load");

        let produced = runtime
            .dispatch(
                RecordEvent::AfterCreate,
                "posts",
                &json!({ "collection": "posts", "record": { "id": "p1" } }),
            )
            .expect("dispatch");
        assert_eq!(produced["record"]["status"], "200:p1");
    }

    #[test]
    fn a_hook_cannot_reach_a_host_that_was_not_allowed() {
        let runtime = http_runtime("");
        runtime
            .load(
                r#"
                onRecordAfterCreate("posts", () => {
                    $http.get("https://api.example.com/x");
                });
                "#,
            )
            .expect("load");

        let err = runtime
            .dispatch(
                RecordEvent::AfterCreate,
                "posts",
                &json!({ "collection": "posts", "record": {} }),
            )
            .expect_err("refused");
        let message = format!("{err}");
        assert!(
            message.contains("LAZYPOCK_HOOK_HTTP_ALLOWLIST"),
            "the hook author is told how to allow the host: {message}"
        );
    }

    fn event(record: Value) -> Value {
        json!({ "collection": "posts", "record": record })
    }

    #[test]
    fn hooks_are_registered_per_event_and_collection() {
        let runtime = runtime();
        runtime
            .load(
                r#"
                onRecordBeforeCreate("posts", (e) => { e.record.slug = "hello"; });
                onRecordBeforeCreate("posts", () => {});
                onRecordBeforeCreate("orders", () => {});
                "#,
            )
            .expect("load");

        assert_eq!(
            runtime
                .registered(RecordEvent::BeforeCreate, "posts")
                .unwrap(),
            2
        );
        assert_eq!(
            runtime
                .registered(RecordEvent::BeforeCreate, "orders")
                .unwrap(),
            1
        );
        // Nothing registered for another event, or another collection.
        assert_eq!(
            runtime
                .registered(RecordEvent::AfterCreate, "posts")
                .unwrap(),
            0
        );
        assert_eq!(
            runtime
                .registered(RecordEvent::BeforeCreate, "other")
                .unwrap(),
            0
        );
    }

    #[test]
    fn a_before_hook_can_mutate_the_record_it_is_given() {
        let runtime = runtime();
        runtime
            .load(
                r#"
                onRecordBeforeCreate("posts", (e) => {
                    e.record.slug = e.record.title.toLowerCase().replace(/ /g, "-");
                });
                "#,
            )
            .expect("load");

        let produced = runtime
            .dispatch(
                RecordEvent::BeforeCreate,
                "posts",
                &event(json!({ "title": "Hello World" })),
            )
            .expect("dispatch");

        assert_eq!(produced["record"]["slug"], json!("hello-world"));
    }

    #[test]
    fn a_hook_that_returns_a_record_replaces_it() {
        let runtime = runtime();
        runtime
            .load(
                r#"
                onRecordBeforeUpdate("posts", (e) => ({ ...e.record, touched: true }));
                "#,
            )
            .expect("load");

        let produced = runtime
            .dispatch(
                RecordEvent::BeforeUpdate,
                "posts",
                &event(json!({ "id": "p1" })),
            )
            .expect("dispatch");

        assert_eq!(produced["record"], json!({ "id": "p1", "touched": true }));
    }

    #[test]
    fn handlers_run_in_registration_order() {
        let runtime = runtime();
        runtime
            .load(
                r#"
                onRecordBeforeCreate("posts", (e) => { e.record.order = "first"; });
                onRecordBeforeCreate("posts", (e) => { e.record.order += "+second"; });
                "#,
            )
            .expect("load");

        let produced = runtime
            .dispatch(RecordEvent::BeforeCreate, "posts", &event(json!({})))
            .expect("dispatch");

        assert_eq!(produced["record"]["order"], json!("first+second"));
    }

    #[test]
    fn a_throwing_hook_is_reported_rather_than_swallowed() {
        let runtime = runtime();
        runtime
            .load(r#"onRecordBeforeCreate("posts", () => { throw new Error("nope"); });"#)
            .expect("load");

        let error = runtime
            .dispatch(RecordEvent::BeforeCreate, "posts", &event(json!({})))
            .expect_err("a throwing hook should be an error");
        assert!(
            error.message().contains("running hooks"),
            "{}",
            error.message()
        );
    }

    #[test]
    fn an_async_hook_is_refused_rather_than_silently_skipped() {
        // The context has no job queue, so a promise cannot be awaited. Dropping
        // the handler's work silently is the outcome to avoid.
        let runtime = runtime();
        runtime
            .load(r#"onRecordBeforeCreate("posts", async (e) => { e.record.slug = "x"; });"#)
            .expect("load");

        let error = runtime
            .dispatch(RecordEvent::BeforeCreate, "posts", &event(json!({})))
            .expect_err("an async hook should be refused");
        assert!(error.message().contains("promise"), "{}", error.message());
    }

    #[test]
    fn a_registration_without_a_handler_is_refused() {
        let runtime = runtime();
        let error = runtime
            .load(r#"onRecordBeforeCreate("posts", "not a function");"#)
            .expect_err("a bad registration should be refused");
        assert!(error.message().contains("handler"), "{}", error.message());
    }

    #[test]
    fn a_syntax_error_is_reported_and_leaves_the_runtime_usable() {
        let runtime = runtime();
        let error = runtime
            .load("onRecordBeforeCreate(")
            .expect_err("a syntax error should be refused");
        assert!(
            error.message().contains("loading hooks"),
            "{}",
            error.message()
        );

        // The runtime still works, so one bad file does not take the rest down.
        runtime
            .load(r#"onRecordAfterCreate("posts", () => {});"#)
            .expect("a good file still loads");
        assert_eq!(
            runtime
                .registered(RecordEvent::AfterCreate, "posts")
                .unwrap(),
            1
        );
    }

    #[test]
    fn a_runaway_hook_is_interrupted_by_the_deadline() {
        let runtime = HookRuntime::new(HookLimits {
            deadline: Duration::from_millis(200),
            ..HookLimits::default()
        })
        .expect("runtime");
        runtime
            .load(r#"onRecordBeforeCreate("posts", () => { while (true) {} });"#)
            .expect("load");

        let started = Instant::now();
        let error = runtime
            .dispatch(RecordEvent::BeforeCreate, "posts", &event(json!({})))
            .expect_err("an endless hook should be interrupted");
        assert!(
            error.message().contains("running hooks"),
            "{}",
            error.message()
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the interrupt took {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn an_allocation_past_the_limit_is_refused() {
        let runtime = HookRuntime::new(HookLimits {
            memory_bytes: 1024 * 1024,
            ..HookLimits::default()
        })
        .expect("runtime");
        runtime
            .load(r#"onRecordBeforeCreate("posts", () => { new Array(50_000_000).fill(1); });"#)
            .expect("load");

        let error = runtime
            .dispatch(RecordEvent::BeforeCreate, "posts", &event(json!({})))
            .expect_err("an over-allocating hook should be refused");
        assert!(
            error.message().contains("running hooks"),
            "{}",
            error.message()
        );
    }

    #[test]
    fn a_collection_name_cannot_escape_its_literal() {
        // The name reaches the runtime inside a string literal, so a quote in it
        // must not become code.
        let runtime = runtime();
        let error = runtime
            .load(r#"onRecordBeforeCreate("a"); globalThis.escaped = true; //", () => {});"#)
            .expect_err("a bad registration should be refused");
        assert!(
            error.message().contains("loading hooks"),
            "{}",
            error.message()
        );

        // Nothing ran beyond the failing statement: a quote in a collection name is
        // an ordinary character, not an escape.
        let clean = HookRuntime::new(HookLimits::default()).expect("runtime");
        clean
            .load(r#"onRecordBeforeCreate("a\"b", () => {});"#)
            .expect("load");
        assert_eq!(
            clean.registered(RecordEvent::BeforeCreate, "a\"b").unwrap(),
            1
        );
    }

    #[test]
    fn every_event_a_hook_can_register_for_dispatches() {
        let runtime = runtime();
        runtime
            .load(
                r#"
                onRecordBeforeCreate("posts", (e) => { e.record.seen = "beforeCreate"; });
                onRecordAfterCreate("posts", (e) => { e.record.seen = "afterCreate"; });
                onRecordBeforeUpdate("posts", (e) => { e.record.seen = "beforeUpdate"; });
                onRecordAfterUpdate("posts", (e) => { e.record.seen = "afterUpdate"; });
                onRecordBeforeDelete("posts", (e) => { e.record.seen = "beforeDelete"; });
                onRecordAfterDelete("posts", (e) => { e.record.seen = "afterDelete"; });
                "#,
            )
            .expect("load");

        for event in RecordEvent::all() {
            let produced = runtime
                .dispatch(event, "posts", &event_payload(event))
                .unwrap_or_else(|err| panic!("{event:?} failed: {err}"));
            assert!(
                produced["record"]["seen"].is_string(),
                "{event:?} did not run its handler: {produced}"
            );
        }
    }

    /// A payload shaped like the one the records layer will send.
    fn event_payload(event: RecordEvent) -> Value {
        let action = match event {
            RecordEvent::BeforeCreate | RecordEvent::AfterCreate => "create",
            RecordEvent::BeforeUpdate | RecordEvent::AfterUpdate => "update",
            RecordEvent::BeforeDelete | RecordEvent::AfterDelete => "delete",
        };
        json!({ "collection": "posts", "action": action, "record": { "id": "p1" } })
    }

    #[test]
    fn memory_use_is_reported_and_stays_inside_the_limit() {
        let runtime = HookRuntime::new(HookLimits {
            memory_bytes: 4 * 1024 * 1024,
            ..HookLimits::default()
        })
        .expect("runtime");

        let before = runtime.memory_used();
        runtime
            .load(r#"globalThis.held = new Array(20_000).fill("x");"#)
            .expect("load");
        let after = runtime.memory_used();

        assert!(after >= before, "allocation should not shrink the heap");
        assert!(
            after < 4 * 1024 * 1024,
            "the heap should stay inside the limit, was {after}"
        );
    }

    #[test]
    fn only_the_before_events_run_inside_a_transaction() {
        for event in RecordEvent::all() {
            let expected = matches!(
                event,
                RecordEvent::BeforeCreate | RecordEvent::BeforeUpdate | RecordEvent::BeforeDelete
            );
            assert_eq!(event.is_before(), expected, "{event:?}");
        }
    }
}
