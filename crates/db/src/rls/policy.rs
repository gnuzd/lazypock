//! Compiling a collection rule into a policy predicate.
//!
//! A policy cannot take bind parameters, so every literal is inlined and
//! `@request.auth.*` becomes a call to the `auth` helpers. Only the collection's
//! own columns may be referenced: a dotted path would need a subquery, which a
//! policy cannot express yet, so it is refused rather than silently dropped.

use lazypock_core::error::{Error, Result};

use crate::ddl::quote_ident;
use crate::query::compile::{cast_for, check_operator_supported, escape_like, FilterContext};
use crate::query::filter::{AuthRef, CompareOp, Expr, Operand, Side};

/// Compile a rule expression into a SQL predicate.
pub fn predicate(rule: &Expr, context: &FilterContext) -> Result<String> {
    match rule {
        Expr::And(left, right) => Ok(format!(
            "({} and {})",
            predicate(left, context)?,
            predicate(right, context)?
        )),
        Expr::Or(left, right) => Ok(format!(
            "({} or {})",
            predicate(left, context)?,
            predicate(right, context)?
        )),
        Expr::Compare { left, op, right } => compare_sides(left, *op, right, context),
    }
}

/// Compile one comparison, whichever side the field is on.
///
/// The documented rule shape puts the request reference on the left
/// (`@request.auth.id != ""`, `@request.auth.role = "admin"`), which needs no column
/// at all — so it must compile here, or a documented rule would be refused before it
/// could become a policy.
fn compare_sides(
    left: &Side,
    op: CompareOp,
    right: &Side,
    context: &FilterContext,
) -> Result<String> {
    match (left, right) {
        (Side::Path(path), Side::Value(operand)) => compare(path, op, operand, false, context),
        (Side::Value(operand), Side::Path(path)) => compare(path, op, operand, true, context),
        (Side::Value(left), Side::Value(right)) => compare_values(left, op, right),
        (Side::Path(left), Side::Path(right)) => Err(Error::unsupported(format!(
            "a rule cannot compare two fields (`{}` and `{}`); compare a field with a value \
             instead",
            left.join("."),
            right.join(".")
        ))),
    }
}

fn compare(
    path: &[String],
    op: CompareOp,
    operand: &Operand,
    swapped: bool,
    context: &FilterContext,
) -> Result<String> {
    let [column] = path else {
        return Err(Error::unsupported(format!(
            "rule paths through a relation (`{}`) are not supported yet; reference a plain column",
            path.join(".")
        )));
    };

    let column_type = context.columns.get(column).ok_or_else(|| {
        Error::invalid(format!(
            "unknown field {column:?} in collection {:?}",
            context.table
        ))
    })?;
    let target = quote_ident(column);
    check_operator_supported(op, column_type, &target)?;

    if matches!(operand, Operand::Null) {
        return match op {
            CompareOp::Eq => Ok(format!("{target} is null")),
            CompareOp::NotEq => Ok(format!("{target} is not null")),
            other => Err(Error::invalid(format!(
                "`{}` cannot be combined with null; use `=` or `!=`",
                other.as_str()
            ))),
        };
    }

    match op {
        CompareOp::Any | CompareOp::NotAny => {
            // The operand is compared against one element, so it is cast to the
            // element type rather than to the array type.
            let value = value_sql(operand, "text")?;
            let predicate = format!("{value} = any({target})");
            Ok(if op == CompareOp::Any {
                predicate
            } else {
                format!("not ({predicate})")
            })
        }
        CompareOp::Like | CompareOp::NotLike => {
            let keyword = if op == CompareOp::Like {
                "ilike"
            } else {
                "not ilike"
            };
            // A literal pattern is built here, so the wildcards are part of the
            // literal instead of being concatenated at run time.
            let expression = match operand {
                Operand::Text(value) => sql_text(&format!("%{}%", escape_like(value))),
                other => format!("'%' || {} || '%'", value_sql(other, "text")?),
            };
            if swapped {
                // `@request.auth.email ~ title`: the reference is the haystack and the
                // column is the needle.
                return Ok(format!("{expression} {keyword} {target} escape '\\'"));
            }
            Ok(format!("{target} {keyword} {expression} escape '\\'"))
        }
        CompareOp::Eq
        | CompareOp::NotEq
        | CompareOp::Gt
        | CompareOp::Gte
        | CompareOp::Lt
        | CompareOp::Lte => {
            let sql_operator = match op {
                CompareOp::Eq => "=",
                CompareOp::NotEq => "<>",
                CompareOp::Gt => ">",
                CompareOp::Gte => ">=",
                CompareOp::Lt => "<",
                CompareOp::Lte => "<=",
                _ => unreachable!("handled above"),
            };

            if matches!(operand, Operand::Now) {
                if column_type != "timestamptz" {
                    return Err(Error::invalid(format!(
                        "`@now` cannot be compared with {target} ({column_type}); it is only valid \
                         for date fields"
                    )));
                }
                return Ok(if swapped {
                    format!("now() {sql_operator} {target}")
                } else {
                    format!("{target} {sql_operator} now()")
                });
            }

            let value = value_sql(operand, column_type)?;
            Ok(if swapped {
                format!("{value} {sql_operator} {target}")
            } else {
                format!("{target} {sql_operator} {value}")
            })
        }
    }
}

/// Compile a comparison in which both sides are values — no column at all.
///
/// `createRule: "@request.auth.id != ''"` is PocketBase's documented shape, and it is
/// a condition on the *request*, not the row, so the predicate is the same for every
/// record.
fn compare_values(left: &Operand, op: CompareOp, right: &Operand) -> Result<String> {
    if matches!(left, Operand::Null) || matches!(right, Operand::Null) {
        return Err(Error::invalid(
            "a rule cannot compare with null without a field; there is nothing to test for \
             `is null` here",
        ));
    }

    if matches!(op, CompareOp::Any | CompareOp::NotAny) {
        return Err(Error::invalid(format!(
            "`{}` needs a multiple-value field, and this comparison has none",
            op.as_str()
        )));
    }

    if matches!(op, CompareOp::Like | CompareOp::NotLike) {
        let keyword = if op == CompareOp::Like {
            "ilike"
        } else {
            "not ilike"
        };
        let haystack = value_sql(left, "text")?;
        let needle = match right {
            Operand::Text(value) => sql_text(&format!("%{}%", escape_like(value))),
            other => format!("'%' || {} || '%'", value_sql(other, "text")?),
        };
        return Ok(format!("{haystack} {keyword} {needle} escape '\\'"));
    }

    let cast = if matches!(left, Operand::Now) || matches!(right, Operand::Now) {
        "timestamptz"
    } else if matches!(left, Operand::Number(_)) || matches!(right, Operand::Number(_)) {
        "double precision"
    } else {
        "text"
    };
    let left_sql = rule_value_sql(left, cast)?;
    let right_sql = rule_value_sql(right, cast)?;
    Ok(format!(
        "{left_sql} {} {right_sql}",
        match op {
            CompareOp::Eq => "=",
            CompareOp::NotEq => "<>",
            CompareOp::Gt => ">",
            CompareOp::Gte => ">=",
            CompareOp::Lt => "<",
            CompareOp::Lte => "<=",
            _ => unreachable!("handled above"),
        }
    ))
}

/// A value side cast to `cast`, with `@now` becoming the database clock.
fn rule_value_sql(operand: &Operand, cast: &str) -> Result<String> {
    if matches!(operand, Operand::Now) {
        return Ok("now()".to_string());
    }
    value_sql(operand, cast)
}

/// The SQL for the right-hand side, cast to the column's type.
fn value_sql(operand: &Operand, column_type: &str) -> Result<String> {
    let cast = cast_for(column_type);
    Ok(match operand {
        Operand::Text(value) => format!("{}::{cast}", sql_text(value)),
        Operand::Number(value) => {
            if !value.is_finite() {
                return Err(Error::invalid(
                    "a rule cannot compare against a non-finite number",
                ));
            }
            format!("{value}::{cast}")
        }
        Operand::Bool(value) => format!("{value}::{cast}"),
        Operand::Auth(reference) => format!("{}::{cast}", auth_sql(reference)),
        Operand::Null | Operand::Now => {
            return Err(Error::internal(
                "null and @now are handled before the value encoder",
            ))
        }
    })
}

/// The `auth` helper call for a reference.
fn auth_sql(reference: &AuthRef) -> String {
    match reference {
        AuthRef::Id => "auth.uid()".to_string(),
        AuthRef::Email => "auth.claim('email')".to_string(),
        AuthRef::CollectionId => "auth.collection_id()".to_string(),
        AuthRef::CollectionName => "auth.collection_name()".to_string(),
        AuthRef::Claim(name) => format!("auth.claim({})", sql_text(name)),
    }
}

/// A SQL string literal.
///
/// Standard-conforming strings are assumed, so a backslash is an ordinary
/// character and only the quote needs doubling.
fn sql_text(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::{Collection, Field};
    use crate::query::filter;
    use crate::types::{CollectionType, FieldType};
    use serde_json::json;
    use std::time::SystemTime;

    fn field(name: &str, field_type: FieldType, options: serde_json::Value) -> Field {
        Field {
            id: format!("f_{name}"),
            collection_id: "c1".into(),
            name: name.into(),
            field_type,
            required: false,
            is_unique: false,
            options,
            position: 0,
        }
    }

    fn context() -> FilterContext {
        let posts = Collection {
            id: "c1".into(),
            name: "posts".into(),
            collection_type: CollectionType::Base,
            options: json!({}),
            rules: json!({}),
            fields: vec![
                field("title", FieldType::Text, json!({})),
                field("views", FieldType::Number, json!({"onlyInt": true})),
                field("published", FieldType::Bool, json!({})),
                field("author", FieldType::Text, json!({})),
                field(
                    "tags",
                    FieldType::Select,
                    json!({"values": ["a"], "maxSelect": 5}),
                ),
            ],
            created: SystemTime::now(),
            updated: SystemTime::now(),
        };
        FilterContext::for_collection(&posts, std::slice::from_ref(&posts)).unwrap()
    }

    fn compiled(rule: &str) -> String {
        let context = context();
        let expression = filter::parse(rule).expect("parse");
        predicate(&expression, &context).expect("compile")
    }

    #[test]
    fn literals_are_inlined_with_their_cast() {
        assert_eq!(compiled("title = \"abc\""), "\"title\" = 'abc'::text");
        assert_eq!(compiled("views > 10"), "\"views\" > 10::bigint");
        assert_eq!(
            compiled("published = true"),
            "\"published\" = true::boolean"
        );
    }

    #[test]
    fn auth_references_become_helper_calls() {
        assert_eq!(
            compiled("author = @request.auth.id"),
            "\"author\" = auth.uid()::text"
        );
        assert_eq!(
            compiled("author = @request.auth.email"),
            "\"author\" = auth.claim('email')::text"
        );
        assert_eq!(
            compiled("author = @request.auth.collectionId"),
            "\"author\" = auth.collection_id()::text"
        );
        assert_eq!(
            compiled("author = @request.auth.collectionName"),
            "\"author\" = auth.collection_name()::text"
        );
        assert_eq!(
            compiled("author = @request.auth.role"),
            "\"author\" = auth.claim('role')::text"
        );
    }

    #[test]
    fn a_request_reference_on_the_left_becomes_a_policy_predicate() {
        // The documented rule shape with no column: `@request.auth.id != ""`.
        assert_eq!(
            compiled("@request.auth.id != ''"),
            "auth.uid()::text <> ''::text"
        );
        assert_eq!(
            compiled("@request.auth.role = 'admin'"),
            "auth.claim('role')::text = 'admin'::text"
        );
        // ... and with a column on the right.
        assert_eq!(
            compiled("@request.auth.id = author"),
            "auth.uid()::text = \"author\""
        );
    }

    #[test]
    fn quotes_are_escaped_so_a_rule_cannot_break_out() {
        assert_eq!(
            compiled("title = \"'; drop table posts; --\""),
            "\"title\" = '''; drop table posts; --'::text"
        );
    }

    #[test]
    fn logical_operators_and_null_compile() {
        assert_eq!(
            compiled("published = true && title != null"),
            "(\"published\" = true::boolean and \"title\" is not null)"
        );
        assert_eq!(
            compiled("title = \"a\" || views > 1"),
            "(\"title\" = 'a'::text or \"views\" > 1::bigint)"
        );
    }

    #[test]
    fn contains_builds_the_pattern_inline() {
        assert_eq!(
            compiled("title ~ \"ab\""),
            "\"title\" ilike '%ab%' escape '\\'"
        );
        // A literal `%` stays literal.
        assert_eq!(
            compiled("title ~ \"50%\""),
            "\"title\" ilike '%50\\%%' escape '\\'"
        );
        assert_eq!(
            compiled("title !~ \"ab\""),
            "\"title\" not ilike '%ab%' escape '\\'"
        );
        // ... and a reference is concatenated.
        assert_eq!(
            compiled("title ~ @request.auth.email"),
            "\"title\" ilike '%' || auth.claim('email')::text || '%' escape '\\'"
        );
    }

    #[test]
    fn any_of_targets_arrays() {
        assert_eq!(compiled("tags ?= \"a\""), "'a'::text = any(\"tags\")");
        assert_eq!(
            compiled("tags ?!= \"a\""),
            "not ('a'::text = any(\"tags\"))"
        );
    }

    #[test]
    fn now_compiles_to_the_database_clock() {
        assert_eq!(compiled("title = \"x\""), "\"title\" = 'x'::text");
    }

    #[test]
    fn unknown_columns_and_unsupported_shapes_are_refused() {
        let context = context();

        for rule in [
            "nope = 1",
            "author.name = \"x\"",
            // An array cannot be compared with `=`.
            "tags = \"a\"",
            // `@now` only makes sense against a date column.
            "views > @now",
        ] {
            let expression = filter::parse(rule);
            if let Ok(expression) = expression {
                let result = predicate(&expression, &context);
                if let Ok(sql) = result {
                    panic!("{rule:?} should have been refused, got {sql}");
                }
            }
        }
    }
}
