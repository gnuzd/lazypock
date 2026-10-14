//! TypeScript hooks (`PLAN.md` section 11).
//!
//! A hook file registers handlers for record events; the server calls them around
//! the database work. The runtime is a QuickJS context with hard limits, and it is
//! **only created when there is something to run** — an installation with no hook
//! files pays nothing for the capability.
//!
//! Two decisions worth stating up front:
//!
//! - **Async handlers are refused, not skipped.** This context has no job queue,
//!   so a promise cannot be awaited. A handler that returns one gets an error
//!   naming the reason, because the alternative is a hook that silently does
//!   nothing.
//! - **A hook error is contained.** QuickJS state is poisoned by an exception, so
//!   the runtime is rebuilt rather than reused; the error is reported once and the
//!   request or the server survives.

pub mod loader;
pub mod runtime;
pub mod service;
pub mod strip;

pub use loader::{HookLoader, LoadReport};
pub use runtime::{HookLimits, HookRuntime, RecordEvent};
pub use service::HookService;
pub use strip::strip_types;
