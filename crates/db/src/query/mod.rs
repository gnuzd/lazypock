//! Query plane: PocketBase-compatible filter and sort expressions compiled into
//! fully parameterized SQL.
//!
//! Split so each concern is testable on its own:
//!
//! * [`filter`] — lexer, AST and parser for filter expressions.
//! * [`compile`] — schema-derived whitelisting and AST → SQL + parameters.
//! * [`sort`] — the `sort` parameter.

pub mod compile;
pub mod filter;
pub mod sort;
