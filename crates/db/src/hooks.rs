//! The seam the records layer uses to run hooks around a write.
//!
//! This crate deliberately does **not** depend on the hook runtime. The records
//! layer needs one thing — hand a record to the hooks and get one back — and the
//! API layer supplies the implementation. That keeps the JavaScript runtime out of
//! the data plane's dependency graph and lets the write path be tested with a
//! plain Rust double.
//!
//! Note that the hook runtime is **not `Send`**: QuickJS holds an `Rc` internally,
//! so a runtime must be owned by one thread. The trait therefore describes *what*
//! to do without saying *where* the implementation keeps its state; the adapter is
//! free to cross a channel to a dedicated thread.

use serde_json::Value;

use crate::rls::context::RequestContext;
use crate::runner::BoxFuture;
use lazypock_core::error::Result;

/// A write a hook can be attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookEvent {
    /// A record is being created.
    Create,
    /// A record is being updated.
    Update,
    /// A record is being deleted.
    Delete,
}

impl HookEvent {
    /// The name used in hook files and in log lines.
    pub fn as_str(self) -> &'static str {
        match self {
            HookEvent::Create => "create",
            HookEvent::Update => "update",
            HookEvent::Delete => "delete",
        }
    }
}

/// Who triggered the write a hook is running for.
///
/// Carried alongside the record rather than looked up, because a hook runs on the
/// hook worker's own thread: the triggering request's transaction, its identity and
/// its socket are all on a different thread by then. What travels is the database
/// context that request ran under, so a hook's own database access is bounded by the
/// same rules the caller was — never by the connecting role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookContext {
    /// The collection whose hook is running, for the re-entrancy refusal.
    pub collection: String,
    /// The request's row-level-security context, as the triggering request built it.
    pub request: RequestContext,
}

impl HookContext {
    /// A context for a hook triggered by `request` on `collection`.
    pub fn new(collection: impl Into<String>, request: RequestContext) -> Self {
        Self {
            collection: collection.into(),
            request,
        }
    }

    /// The context a write with no request behind it runs under: anonymous, so a
    /// hook that reaches the database can only do what the public rules allow.
    pub fn anonymous(collection: impl Into<String>) -> Self {
        Self::new(collection, RequestContext::anonymous())
    }

    /// The same context, for one record action.
    ///
    /// `$app.findRecordById` must be matched against `viewRule` rather than the
    /// action of whatever write happens to be in flight, so each operation sets its
    /// own.
    pub fn with_action(mut self, action: crate::rls::context::Action) -> Self {
        self.request = self.request.with_action(action);
        self
    }
}

impl Default for HookContext {
    fn default() -> Self {
        Self::anonymous(String::new())
    }
}

/// Runs hooks around a record write.
///
/// Object-safe, with boxed `Send` futures and the supertraits the handler futures
/// need, matching the shape used elsewhere in this workspace.
pub trait RecordHooks: Send + Sync {
    /// Run the hooks that precede a write, returning the record to write.
    ///
    /// Runs inside the caller's transaction, so it stays short.
    fn before<'a>(
        &'a self,
        event: HookEvent,
        collection: &'a str,
        record: Value,
    ) -> BoxFuture<'a, Result<Value>>;

    /// Run the before-hooks for a write that has a triggering request behind it.
    ///
    /// The context is what lets a hook reach the database as the caller rather than
    /// as the server. A caller that does not know it wraps this trait with
    /// [`ScopedHooks`] instead of implementing this — the default drops the context,
    /// and a hook that needs it then refuses rather than guessing.
    fn before_ctx<'a>(
        &'a self,
        _context: &'a HookContext,
        event: HookEvent,
        collection: &'a str,
        record: Value,
    ) -> BoxFuture<'a, Result<Value>> {
        self.before(event, collection, record)
    }

    /// The after-hooks for a write that has a triggering request behind it.
    fn after_ctx<'a>(
        &'a self,
        _context: &'a HookContext,
        event: HookEvent,
        collection: &'a str,
        record: Value,
    ) -> BoxFuture<'a, Result<()>> {
        self.after(event, collection, record)
    }

    /// Run the hooks that follow a write.
    ///
    /// Called once the write has committed, so a failure here cannot undo it.
    fn after<'a>(
        &'a self,
        event: HookEvent,
        collection: &'a str,
        record: Value,
    ) -> BoxFuture<'a, Result<()>>;
}

/// Hooks bound to the request that triggered them.
///
/// The write path passes `&dyn RecordHooks` around, and most of it has no business
/// knowing about requests. Wrapping the real implementation here keeps that: the
/// context is attached once, at the entry point that has a principal, and the layers
/// below hand the trait object along as before.
pub struct ScopedHooks<'a> {
    inner: &'a dyn RecordHooks,
    context: HookContext,
}

impl<'a> ScopedHooks<'a> {
    /// Bind `inner` to the request `context` describes.
    pub fn new(inner: &'a dyn RecordHooks, context: HookContext) -> Self {
        Self { inner, context }
    }
}

impl std::fmt::Debug for ScopedHooks<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ScopedHooks")
            .field("collection", &self.context.collection)
            .finish_non_exhaustive()
    }
}

impl RecordHooks for ScopedHooks<'_> {
    fn before<'a>(
        &'a self,
        event: HookEvent,
        collection: &'a str,
        record: Value,
    ) -> BoxFuture<'a, Result<Value>> {
        self.inner
            .before_ctx(&self.context, event, collection, record)
    }

    fn after<'a>(
        &'a self,
        event: HookEvent,
        collection: &'a str,
        record: Value,
    ) -> BoxFuture<'a, Result<()>> {
        self.inner
            .after_ctx(&self.context, event, collection, record)
    }
}

/// An installation with no hooks.
///
/// The default everywhere, so the write path pays nothing when nothing is
/// configured.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoHooks;

impl RecordHooks for NoHooks {
    fn before<'a>(
        &'a self,
        _event: HookEvent,
        _collection: &'a str,
        record: Value,
    ) -> BoxFuture<'a, Result<Value>> {
        Box::pin(async move { Ok(record) })
    }

    fn after<'a>(
        &'a self,
        _event: HookEvent,
        _collection: &'a str,
        _record: Value,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move { Ok(()) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn no_hooks_passes_the_record_through_untouched() {
        let record = serde_json::json!({ "title": "unchanged" });
        let produced = NoHooks
            .before(HookEvent::Create, "posts", record.clone())
            .await
            .expect("no hooks");
        assert_eq!(produced, record);
        NoHooks
            .after(HookEvent::Create, "posts", record)
            .await
            .expect("no hooks");
    }

    #[test]
    fn an_event_names_itself_the_way_a_hook_file_does() {
        assert_eq!(HookEvent::Create.as_str(), "create");
        assert_eq!(HookEvent::Update.as_str(), "update");
        assert_eq!(HookEvent::Delete.as_str(), "delete");
    }
}
