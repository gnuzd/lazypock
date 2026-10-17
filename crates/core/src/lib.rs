//! Core building blocks shared by every lazypock crate.
//!
//! This crate intentionally stays dependency-light: configuration, the error
//! model, identifier generation and telemetry setup. Anything that needs
//! Postgres, HTTP or a JS runtime lives in a higher crate.

pub mod config;
pub mod error;
pub mod health;
pub mod id;
pub mod net;
pub mod secret;
pub mod telemetry;
