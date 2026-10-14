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
