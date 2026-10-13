//! Query plane: PocketBase-compatible filter and sort expressions compiled into
//! fully parameterized SQL.
//!
//! Split so each concern is testable on its own:
//!
//! * [`filter`] — lexer, AST and parser for filter expressions.
//! * [`compile`] — schema-derived whitelisting and AST → SQL + parameters.
//! * [`sort`] — the `sort` parameter.
//! * [`pagination`] — offset pages and keyset cursors.
//! * [`projection`] — the `fields` and `expand` trees.
//! * [`read`] — assembles a complete record `SELECT` from all of the above.

pub mod compile;
pub mod filter;
pub mod pagination;
pub mod projection;
pub mod read;
pub mod sort;
