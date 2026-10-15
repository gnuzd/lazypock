//! Realtime: change events, fan-out and subscriptions.
//!
//! Postgres is the message bus (`PLAN.md` section 8). A trigger on each
//! collection appends to `_rt_log` and `NOTIFY`s a small payload; every node
//! `LISTEN`s on its own connection and fans the event out to its own clients,
//! filtered by the same row-level security that governs ordinary reads.

pub mod bus;
pub mod event;
pub mod hub;

pub use bus::{EventBus, PgBus};
pub use event::{ChangeEvent, ChangeOp};
pub use hub::{Delivered, Hub, Subscription, REGISTRY_TOPIC};
