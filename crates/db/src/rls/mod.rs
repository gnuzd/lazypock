//! Row-level security: rules -> Postgres policies, and the per-request context
//! that tells those policies who is asking.

mod context;
mod policies;

pub use context::{assume_owner, begin, begin_owner, Action, RequestContext, Role};
pub use policies::{apply, clear, rule_predicate, statements, validate_rules};
