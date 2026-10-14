//! Row-level security (PLAN.md section 6).
//!
//! This lives in the database crate rather than in a crate of its own on purpose:
//! creating a collection is what applies its policies, and a separate crate would
//! have to depend on this one — so policies would be applied by *callers*, and a
//! new call site could forget. Keeping it here means RLS is part of the DDL
//! transaction instead of something remembered afterwards.

pub mod context;
pub mod policies;
pub mod policy;

pub use context::{RequestContext, Role};
