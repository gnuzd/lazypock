//! AST → SQL, checked against the collection's schema.
//!
//! One compiler, two output modes:
//! - `Mode::Bind`: values become `$n` parameters (API reads).
//! - `Mode::Inline`: values are rendered as escaped literals (RLS policies;
//!   `CREATE POLICY` is DDL and cannot take parameters). Literals only ever come
//!   from the parsed AST, and strings go through `pg_string_literal`.
//!
//! Field names are never trusted: every one must exist in the collection.

use super::{AuthRef, Expr, Lit, Macro, Op, Operand};
use crate::error::{self};
use crate::schema::Collection;
use crate::types::Kind;
use lazypock_core::Error;
use sea_query::{Alias, BinOper, Expr as E, ExprTrait, PostgresQueryBuilder, Query, SimpleExpr};

use lazypock_core::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Bind,
    Inline,
}

fn err(msg: impl Into<String>) -> Error {
    error::invalid_filter(msg.into())
}

/// What one side of a comparison is, once resolved against the schema.
enum Side {
    Col(String, Kind),
    Auth(AuthRef),
    Now,
    Lit(Lit),
}

pub fn compile(c: &Collection, e: &Expr, mode: Mode) -> Result<SimpleExpr> {
    match e {
        Expr::And(parts) => fold(c, parts, mode, SimpleExpr::and),
        Expr::Or(parts) => fold(c, parts, mode, SimpleExpr::or),
        Expr::Cmp(l, op, r) => compare(c, l, *op, r, mode),
    }
}

fn fold(
    c: &Collection,
    parts: &[Expr],
    mode: Mode,
    join: fn(SimpleExpr, SimpleExpr) -> SimpleExpr,
) -> Result<SimpleExpr> {
    let mut it = parts.iter();
    let first = it.next().ok_or_else(|| err("empty group"))?;
    let mut acc = compile(c, first, mode)?;
    for p in it {
        acc = join(acc, compile(c, p, mode)?);
    }
    Ok(acc)
}

fn resolve(c: &Collection, o: &Operand) -> Result<Side> {
    Ok(match o {
        Operand::Field(name) => {
            let ty = c
                .column_kind(name)
                .ok_or_else(|| err(format!("unknown field {name:?}")))?;
            Side::Col(name.clone(), ty)
        }
        Operand::Macro(Macro::Auth(r)) => Side::Auth(r.clone()),
        Operand::Macro(Macro::Now) => Side::Now,
        Operand::Lit(l) => Side::Lit(l.clone()),
    })
}

fn side_type(s: &Side) -> Option<Kind> {
    match s {
        Side::Col(_, t) => Some(*t),
        Side::Auth(_) => Some(Kind::Text),
        Side::Now => Some(Kind::Date),
        Side::Lit(_) => None,
    }
}

fn compare(c: &Collection, l: &Operand, op: Op, r: &Operand, mode: Mode) -> Result<SimpleExpr> {
    let (ls, rs) = (resolve(c, l)?, resolve(c, r)?);

    // `x = null` / `x != null`
    match (&ls, &rs) {
        (Side::Lit(Lit::Null), Side::Lit(_)) | (Side::Lit(_), Side::Lit(Lit::Null)) => {
            return Err(err("a comparison needs a field"));
        }
        (Side::Lit(Lit::Null), other) | (other, Side::Lit(Lit::Null)) => {
            let e = value(other, None, mode)?;
            return match op {
                Op::Eq => Ok(e.is_null()),
                Op::Ne => Ok(e.is_not_null()),
                _ => Err(err("null can only be used with = and !=")),
            };
        }
        _ => {}
    }

    if matches!(ls, Side::Lit(_)) && matches!(rs, Side::Lit(_)) {
        return Err(err("a comparison needs a field"));
    }
    if matches!(ls, Side::Lit(_)) && matches!(op, Op::Like | Op::NotLike) {
        return Err(err("the left side of ~ must be a field"));
    }

    // The side with a known type decides how a literal on the other side is read.
    let (lt, rt) = (side_type(&ls), side_type(&rs));
    let target = lt.or(rt);
    if let (Some(a), Some(b)) = (lt, rt) {
        let numeric = |t| matches!(t, Kind::Int | Kind::Number);
        if a != b && !(numeric(a) && numeric(b)) {
            return Err(err("cannot compare fields of different types"));
        }
    }
    let ty = target.ok_or_else(|| err("a comparison needs a field"))?;

    match (ty, op) {
        (Kind::Json, _) => return Err(err("json fields can only be compared with null")),
        (Kind::Bool, o) if !matches!(o, Op::Eq | Op::Ne) => {
            return Err(err("bool fields only support = and !="));
        }
        (t, Op::Like | Op::NotLike) if t != Kind::Text => {
            return Err(err("~ and !~ only work on text fields"));
        }
        _ => {}
    }

    let left = value(&ls, Some(ty), mode)?;
    let right = if matches!(op, Op::Like | Op::NotLike) {
        let Side::Lit(Lit::Str(s)) = &rs else {
            return Err(err("the right side of ~ must be a quoted string"));
        };
        text_value(&like_pattern(s), mode)?
    } else {
        value(&rs, Some(ty), mode)?
    };

    Ok(match op {
        Op::Eq => left.eq(right),
        Op::Ne => left.ne(right),
        Op::Gt => left.gt(right),
        Op::Ge => left.gte(right),
        Op::Lt => left.lt(right),
        Op::Le => left.lte(right),
        Op::Like => left.binary(BinOper::Custom("ILIKE"), right),
        Op::NotLike => left.binary(BinOper::Custom("NOT ILIKE"), right),
    })
}

/// `~` semantics: with no `%` the pattern is wrapped (`%x%`); `_` and `\` are
/// always literal.
fn like_pattern(s: &str) -> String {
    let has_wildcard = s.contains('%');
    let mut out = String::with_capacity(s.len() + 2);
    if !has_wildcard {
        out.push('%');
    }
    for ch in s.chars() {
        if matches!(ch, '\\' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    if !has_wildcard {
        out.push('%');
    }
    out
}

fn value(s: &Side, ty: Option<Kind>, mode: Mode) -> Result<SimpleExpr> {
    match s {
        Side::Col(name, _) => Ok(E::col(Alias::new(name.clone())).into()),
        Side::Auth(r) => Ok(E::cust(auth_sql(r))),
        Side::Now => Ok(E::cust("now()")),
        Side::Lit(l) => literal(
            l,
            ty.ok_or_else(|| err("a comparison needs a field"))?,
            mode,
        ),
    }
}

/// The SQL for `@request.auth.*`. The claim name is an identifier by grammar, so
/// quoting it as a string literal needs no escaping.
fn auth_sql(r: &AuthRef) -> String {
    match r {
        AuthRef::Id => "auth.uid()::text".into(),
        AuthRef::Email => "auth.claim('email')".into(),
        AuthRef::CollectionId => "auth.collection_id()::text".into(),
        AuthRef::CollectionName => "auth.collection_name()::text".into(),
        AuthRef::Claim(name) => format!("auth.claim('{name}')"),
    }
}

fn text_value(s: &str, mode: Mode) -> Result<SimpleExpr> {
    Ok(match mode {
        Mode::Bind => E::val(s.to_owned()).into(),
        Mode::Inline => E::cust(pg_string_literal(s)?),
    })
}

fn literal(l: &Lit, ty: Kind, mode: Mode) -> Result<SimpleExpr> {
    let mismatch = || err(format!("value does not fit a {} field", ty.name()));
    Ok(match (ty, l, mode) {
        (Kind::Text, Lit::Str(s), m) => text_value(s, m)?,
        (Kind::Int, Lit::Int(i), Mode::Bind) => E::val(*i).into(),
        (Kind::Int, Lit::Int(i), Mode::Inline) => E::cust(i.to_string()),
        (Kind::Number, Lit::Int(i), Mode::Bind) => E::val(*i as f64).into(),
        (Kind::Number, Lit::Num(f), Mode::Bind) => E::val(*f).into(),
        (Kind::Number, Lit::Int(i), Mode::Inline) => E::cust(format!("{i}::float8")),
        (Kind::Number, Lit::Num(f), Mode::Inline) if f.is_finite() => {
            E::cust(format!("{f:?}::float8"))
        }
        (Kind::Bool, Lit::Bool(b), Mode::Bind) => E::val(*b).into(),
        (Kind::Bool, Lit::Bool(b), Mode::Inline) => E::cust(if *b { "true" } else { "false" }),
        (Kind::Date, Lit::Str(s), Mode::Bind) => {
            E::val(s.clone()).cast_as(Alias::new("timestamptz"))
        }
        (Kind::Date, Lit::Str(s), Mode::Inline) => {
            E::cust(format!("CAST({} AS timestamptz)", pg_string_literal(s)?))
        }
        _ => return Err(mismatch()),
    })
}

/// `E'…'` with `\` and `'` doubled: correct whatever `standard_conforming_strings` says.
pub(crate) fn pg_string_literal(s: &str) -> Result<String> {
    if s.contains('\0') {
        return Err(err("NUL characters are not allowed"));
    }
    let mut out = String::with_capacity(s.len() + 3);
    out.push_str("E'");
    for ch in s.chars() {
        match ch {
            '\'' => out.push_str("''"),
            '\\' => out.push_str("\\\\"),
            c => out.push(c),
        }
    }
    out.push('\'');
    Ok(out)
}

/// Render an inline-mode expression as SQL text (for `CREATE POLICY`).
pub fn render_inline(e: SimpleExpr) -> String {
    let sql = Query::select().expr(e).to_string(PostgresQueryBuilder);
    sql.strip_prefix("SELECT ").unwrap_or(&sql).to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::parse;
    use crate::schema::Field;
    use crate::types::{CollectionType, FieldType};
    use serde_json::json;

    fn coll() -> Collection {
        let mut c = Collection::new("posts", CollectionType::Base).unwrap();
        let mut views = Field::new("views", FieldType::Number).unwrap();
        views.options = json!({"onlyInt": true});
        c.fields = vec![
            Field::new("title", FieldType::Text).unwrap(),
            views,
            Field::new("score", FieldType::Number).unwrap(),
            Field::new("featured", FieldType::Bool).unwrap(),
            Field::new("meta", FieldType::Json).unwrap(),
            Field::new("owner", FieldType::Text).unwrap(),
            Field::new("firstName", FieldType::Text).unwrap(),
        ];
        c
    }

    fn inline(src: &str) -> Result<String> {
        compile(&coll(), &parse(src)?, Mode::Inline).map(render_inline)
    }

    fn bound(src: &str) -> Result<(String, usize)> {
        let e = compile(&coll(), &parse(src)?, Mode::Bind)?;
        let (sql, values) = Query::select().expr(e).build(PostgresQueryBuilder);
        Ok((sql.trim_start_matches("SELECT ").to_owned(), values.0.len()))
    }

    #[test]
    fn inline_renders_expected_sql() {
        assert_eq!(inline("views > 10").unwrap(), "\"views\" > (10)");
        assert_eq!(inline("featured = true").unwrap(), "\"featured\" = (true)");
        assert_eq!(inline("meta = null").unwrap(), "\"meta\" IS NULL");
        assert_eq!(inline("firstName = 'a'").unwrap(), "\"firstName\" = (E'a')");
    }

    #[test]
    fn auth_macros_use_the_auth_helpers() {
        assert_eq!(
            inline("owner = @request.auth.id").unwrap(),
            "\"owner\" = (auth.uid()::text)"
        );
        assert_eq!(
            inline("@request.auth.id != ''").unwrap(),
            "(auth.uid()::text) <> (E'')"
        );
        assert_eq!(
            inline("title = @request.auth.email").unwrap(),
            "\"title\" = (auth.claim('email'))"
        );
        assert_eq!(
            inline("title = @request.auth.role").unwrap(),
            "\"title\" = (auth.claim('role'))"
        );
        assert_eq!(
            inline("owner = @request.auth.collectionId").unwrap(),
            "\"owner\" = (auth.collection_id()::text)"
        );
        assert_eq!(
            inline("@request.auth.role = 'admin'").unwrap(),
            "(auth.claim('role')) = (E'admin')"
        );
    }

    #[test]
    fn bind_mode_uses_parameters_not_text() {
        let (sql, n) = bound("title = 'x' && views > 3").unwrap();
        assert_eq!(n, 2);
        assert!(!sql.contains("'x'"));
        assert!(sql.contains("$1") && sql.contains("$2"));
    }

    #[test]
    fn like_wraps_and_escapes() {
        assert_eq!(like_pattern("ab"), "%ab%");
        assert_eq!(like_pattern("a_b"), "%a\\_b%");
        assert_eq!(like_pattern("ab%"), "ab%");
        assert_eq!(like_pattern("a\\b"), "%a\\\\b%");
    }

    #[test]
    fn precedence_is_parenthesised() {
        let sql = inline("(views > 1 || views < 0) && featured = true").unwrap();
        assert!(sql.starts_with('('), "{sql}");
    }

    #[test]
    fn unknown_hidden_and_mismatched_are_rejected() {
        for bad in [
            "nope = 1",
            "views = 'a'",
            "title = 1",
            "featured > true",
            "meta = 'x'",
            "views ~ 'a'",
            "1 = 1",
            "'a' = 'b'",
            "title = views",
            "views = null && 1 > 2",
            "@request.auth.id = 1",
            "views = @request.auth.id",
        ] {
            assert!(inline(bad).is_err(), "{bad:?} should fail");
        }
        assert!(inline("views < score").is_ok());

        let mut auth = Collection::new("users", CollectionType::Auth).unwrap();
        auth.fields = crate::schema::auth_system_fields();
        let e = |s: &str| compile(&auth, &parse(s).unwrap(), Mode::Inline);
        assert!(e("email = 'a'").is_ok());
        assert!(
            e("password_hash = 'x'").is_err(),
            "credentials are not filterable"
        );
        assert!(e("token_key = 'x'").is_err());
    }

    // The RLS path inlines literals, so these are the tests that matter most.
    #[test]
    fn quotes_cannot_break_out_of_a_literal() {
        let sql = inline(r#"title = 'a\' OR true --'"#).unwrap();
        assert_eq!(sql, r#""title" = (E'a'' OR true --')"#);
        let sql = inline(r#"title = "it's""#).unwrap();
        assert_eq!(sql, r#""title" = (E'it''s')"#);
        let sql = inline(r#"title = 'back\\slash'"#).unwrap();
        assert_eq!(sql, r#""title" = (E'back\\slash')"#);
    }

    #[test]
    fn nul_bytes_are_rejected() {
        assert!(pg_string_literal("a\0b").is_err());
    }

    #[test]
    fn dates_are_cast() {
        assert_eq!(
            inline("created < '2026-01-01'").unwrap(),
            "\"created\" < (CAST(E'2026-01-01' AS timestamptz))"
        );
        assert_eq!(inline("created < @now").unwrap(), "\"created\" < (now())");
    }
}
