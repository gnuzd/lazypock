//! Filter expressions: text → AST → SQL.
//!
//! `parse` turns the text into an [`Expr`]; `compile` (see `compile.rs`) checks
//! every field against the collection and renders either a bound query
//! condition (API reads) or an inlined predicate (RLS policies).

mod compile;

pub use compile::{Mode, compile, render_inline};

use crate::error::{self};
use lazypock_core::Error;
use pest::Parser;
use pest::iterators::Pair;

use lazypock_core::Result;

const MAX_LEN: usize = 1024;
const MAX_PARENS: usize = 32;

#[derive(pest_derive::Parser)]
#[grammar = "filter/filter.pest"]
struct FilterParser;

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Cmp(Operand, Op, Operand),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Eq,
    Ne,
    Gt,
    Ge,
    Lt,
    Le,
    Like,
    NotLike,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Operand {
    Field(String),
    Macro(Macro),
    Lit(Lit),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Macro {
    Now,
    Auth(AuthRef),
}

/// `@request.auth.*`: resolved by the `auth` helper functions (migrations 0003
/// and 0017) from the per-request settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthRef {
    Id,
    Email,
    CollectionId,
    CollectionName,
    /// Any other `@request.auth.<name>`: a column of the signed-in record.
    Claim(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Lit {
    Str(String),
    Int(i64),
    Num(f64),
    Bool(bool),
    Null,
}

fn err(msg: impl Into<String>) -> Error {
    error::invalid_filter(msg.into())
}

pub fn parse(input: &str) -> Result<Expr> {
    if input.len() > MAX_LEN {
        return Err(err(format!("too long (max {MAX_LEN} characters)")));
    }
    // Cheap guard against pathological nesting, before the recursive parser runs.
    if input.matches('(').count() > MAX_PARENS {
        return Err(err("too many parentheses"));
    }
    let mut pairs = FilterParser::parse(Rule::filter, input).map_err(|e| {
        use pest::error::LineColLocation::*;
        let (l, c) = match e.line_col {
            Pos(p) => p,
            Span(p, _) => p,
        };
        err(format!("syntax error at line {l}, column {c}"))
    })?;
    let filter = pairs.next().ok_or_else(|| err("empty filter"))?;
    let expr = filter
        .into_inner()
        .find(|p| p.as_rule() == Rule::expr)
        .ok_or_else(|| err("empty filter"))?;
    build_expr(expr)
}

/// `a && b || c && d` → Or[And[a,b], And[c,d]]; single-element groups collapse.
fn build_expr(pair: Pair<'_, Rule>) -> Result<Expr> {
    let mut or_groups: Vec<Vec<Expr>> = vec![vec![]];
    for p in pair.into_inner() {
        match p.as_rule() {
            Rule::and_op => {}
            Rule::or_op => or_groups.push(vec![]),
            Rule::group => {
                let inner = p.into_inner().next().ok_or_else(|| err("empty group"))?;
                or_groups.last_mut().unwrap().push(build_expr(inner)?);
            }
            Rule::comparison => or_groups.last_mut().unwrap().push(build_cmp(p)?),
            other => return Err(err(format!("unexpected {other:?}"))),
        }
    }
    let mut ors: Vec<Expr> = or_groups
        .into_iter()
        .map(|mut g| {
            if g.len() == 1 {
                g.remove(0)
            } else {
                Expr::And(g)
            }
        })
        .collect();
    Ok(if ors.len() == 1 {
        ors.remove(0)
    } else {
        Expr::Or(ors)
    })
}

fn build_cmp(pair: Pair<'_, Rule>) -> Result<Expr> {
    let mut it = pair.into_inner();
    let lhs = build_operand(it.next().ok_or_else(|| err("missing left side"))?)?;
    let op = match it.next().ok_or_else(|| err("missing operator"))?.as_str() {
        "=" => Op::Eq,
        "!=" => Op::Ne,
        ">" => Op::Gt,
        ">=" => Op::Ge,
        "<" => Op::Lt,
        "<=" => Op::Le,
        "~" => Op::Like,
        "!~" => Op::NotLike,
        other => return Err(err(format!("unknown operator {other:?}"))),
    };
    let rhs = build_operand(it.next().ok_or_else(|| err("missing right side"))?)?;
    Ok(Expr::Cmp(lhs, op, rhs))
}

fn build_operand(p: Pair<'_, Rule>) -> Result<Operand> {
    Ok(match p.as_rule() {
        Rule::field => Operand::Field(p.as_str().to_owned()),
        Rule::macro_ => Operand::Macro(parse_macro(p.as_str())?),
        Rule::boolean => Operand::Lit(Lit::Bool(p.as_str() == "true")),
        Rule::null => Operand::Lit(Lit::Null),
        Rule::number => {
            let s = p.as_str();
            if s.contains('.') {
                Operand::Lit(Lit::Num(s.parse().map_err(|_| err("bad number"))?))
            } else {
                Operand::Lit(Lit::Int(
                    s.parse().map_err(|_| err("integer out of range"))?,
                ))
            }
        }
        Rule::string => {
            let raw = p.into_inner().next().map_or("", |i| i.as_str());
            Operand::Lit(Lit::Str(unescape(raw)))
        }
        other => return Err(err(format!("unexpected {other:?}"))),
    })
}

fn parse_macro(s: &str) -> Result<Macro> {
    if s == "@now" {
        return Ok(Macro::Now);
    }
    // The grammar guarantees `name` is an identifier (letters, digits, `_`).
    match s.strip_prefix("@request.auth.") {
        Some("id") => Ok(Macro::Auth(AuthRef::Id)),
        Some("email") => Ok(Macro::Auth(AuthRef::Email)),
        Some("collectionId") => Ok(Macro::Auth(AuthRef::CollectionId)),
        Some("collectionName") => Ok(Macro::Auth(AuthRef::CollectionName)),
        Some(name) if !name.contains('.') => Ok(Macro::Auth(AuthRef::Claim(name.to_owned()))),
        _ => Err(err(format!("unknown macro {s}"))),
    }
}

fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other), // \' \" \\ and anything else: the char itself
            None => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(n: &str) -> Operand {
        Operand::Field(n.into())
    }
    fn s(v: &str) -> Operand {
        Operand::Lit(Lit::Str(v.into()))
    }

    #[test]
    fn parses_simple_comparison() {
        assert_eq!(
            parse("title = 'a'").unwrap(),
            Expr::Cmp(f("title"), Op::Eq, s("a"))
        );
    }

    #[test]
    fn and_binds_tighter_than_or() {
        let e = parse("a = 1 || b = 2 && c = 3").unwrap();
        let Expr::Or(parts) = e else {
            panic!("expected Or")
        };
        assert_eq!(parts.len(), 2);
        assert!(matches!(parts[1], Expr::And(ref v) if v.len() == 2));
    }

    #[test]
    fn groups_override_precedence() {
        let e = parse("(a = 1 || b = 2) && c = 3").unwrap();
        let Expr::And(parts) = e else {
            panic!("expected And")
        };
        assert!(matches!(parts[0], Expr::Or(_)));
    }

    #[test]
    fn literals_and_macros() {
        assert!(parse("n = -5").is_ok());
        assert!(parse("n >= 1.5").is_ok());
        assert!(parse("ok = true && x = null").is_ok());
        assert!(parse("owner = @request.auth.id").is_ok());
        assert!(parse("created < @now").is_ok());
        assert!(parse("t = @request.auth.a.b").is_err());
        assert!(parse("t = @request.other.id").is_err());
        assert_eq!(
            parse("t = @request.auth.role").unwrap(),
            Expr::Cmp(
                f("t"),
                Op::Eq,
                Operand::Macro(Macro::Auth(AuthRef::Claim("role".into())))
            )
        );
        // `trueish` is a field, not the boolean `true`.
        assert_eq!(
            parse("trueish = 1").unwrap(),
            Expr::Cmp(f("trueish"), Op::Eq, Operand::Lit(Lit::Int(1)))
        );
    }

    #[test]
    fn string_escapes() {
        assert_eq!(
            parse(r#"a = 'it\'s'"#).unwrap(),
            Expr::Cmp(f("a"), Op::Eq, s("it's"))
        );
        assert_eq!(
            parse(r#"a = "x\"y""#).unwrap(),
            Expr::Cmp(f("a"), Op::Eq, s("x\"y"))
        );
    }

    #[test]
    fn rejects_garbage_and_limits() {
        for bad in [
            "",
            "a",
            "a =",
            "= 1",
            "a = 1 &&",
            "a = 1; DROP TABLE x",
            "a = 'x",
            "a == 1",
            "a.b = 1",
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        assert!(parse(&format!("a = '{}'", "x".repeat(2000))).is_err());
        assert!(parse(&format!("{}a = 1{}", "(".repeat(40), ")".repeat(40))).is_err());
    }
}
