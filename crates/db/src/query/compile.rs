//! Compiling filter ASTs into fully parameterized SQL.
//!
//! Every user-supplied value becomes a bind parameter — never string
//! interpolation — and every field name is resolved against a whitelist derived
//! from the schema, so an unknown field is an error rather than a raw column
//! reference. Only *identifiers* are quoted into the statement, and those come
//! from the schema, not from the request.
//!
//! Parameters are sent as text with explicit `::type` casts. That keeps the
//! parameter list homogeneous (`Vec<String>`) while still giving Postgres the
//! type information it needs for comparisons against typed columns.

use std::collections::HashMap;

use lazypock_core::error::{Error, Result};

use super::filter::{AuthRef, CompareOp, Expr, Operand};
use crate::collections::Collection;
use crate::ddl::quote_ident;
use crate::types::{self, CollectionType};

/// Maximum depth of a dotted relation path inside a filter.
pub const MAX_PATH_DEPTH: usize = 4;

/// Values substituted for `@request.auth.*` references.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthContext {
    /// The authenticated record id.
    pub id: Option<String>,
    /// The authenticated record's email.
    pub email: Option<String>,
    /// The authenticated record's collection id.
    pub collection_id: Option<String>,
    /// The authenticated record's collection name.
    pub collection_name: Option<String>,
    /// Extra claims (from OAuth/OIDC or a custom token).
    pub claims: HashMap<String, String>,
}

/// A relation hop reachable from a collection.
#[derive(Debug, Clone)]
pub struct RelationTarget {
    /// The relation column on the parent table.
    pub column: String,
    /// Whether the column is a `text[]` (multi relation).
    pub multiple: bool,
    /// The target table name.
    pub table: String,
    /// The target's own columns and relations.
    pub context: Box<FilterContext>,
}

/// What the compiler needs to know about the collection being filtered.
#[derive(Debug, Clone)]
pub struct FilterContext {
    /// Table name (unquoted).
    pub table: String,
    /// Whitelisted column name to Postgres type.
    pub columns: HashMap<String, String>,
    /// Relation fields reachable by a dotted path.
    pub relations: HashMap<String, RelationTarget>,
}

impl FilterContext {
    /// Build the context for a collection, resolving relation targets from
    /// `all`.
    pub fn for_collection(collection: &Collection, all: &[Collection]) -> Result<Self> {
        Self::build(collection, all, 0)
    }

    fn build(collection: &Collection, all: &[Collection], depth: usize) -> Result<Self> {
        let mut columns: HashMap<String, String> = HashMap::new();
        columns.insert("id".into(), "text".into());
        columns.insert("created".into(), "timestamptz".into());
        columns.insert("updated".into(), "timestamptz".into());
        if collection.collection_type == CollectionType::Auth {
            columns.insert("email".into(), "text".into());
            columns.insert("password_hash".into(), "text".into());
            columns.insert("verified".into(), "boolean".into());
            columns.insert("token_key".into(), "text".into());
        }

        let mut relations = HashMap::new();

        for field in &collection.fields {
            let column_type = field.column_type()?;
            columns.insert(field.name.clone(), column_type.to_string());

            if field.field_type != types::FieldType::Relation {
                continue;
            }

            if depth >= MAX_PATH_DEPTH {
                // Deeper relations simply are not reachable, rather than being a
                // hard error: a collection legitimately related in a cycle must
                // still be filterable at shallow depth.
                continue;
            }

            let target_id = field
                .options
                .get("collectionId")
                .and_then(|value| value.as_str())
                .ok_or_else(|| {
                    Error::invalid(format!(
                        "relation field {:?} has no collectionId",
                        field.name
                    ))
                })?;
            let target = all.iter().find(|candidate| candidate.id == target_id);
            let Some(target) = target else {
                // A dangling relation means the schema is inconsistent; the
                // field itself is still usable as a plain column.
                tracing::warn!(
                    collection = %collection.name,
                    field = %field.name,
                    target = target_id,
                    "relation target is missing from the schema; dotted filters through it are unavailable"
                );
                continue;
            };

            relations.insert(
                field.name.clone(),
                RelationTarget {
                    column: field.name.clone(),
                    multiple: field.is_multiple(),
                    table: target.name.clone(),
                    context: Box::new(Self::build(target, all, depth + 1)?),
                },
            );
        }

        Ok(Self {
            table: collection.name.clone(),
            columns,
            relations,
        })
    }

    /// Whether a column name is whitelisted.
    pub fn has_column(&self, column: &str) -> bool {
        self.columns.contains_key(column)
    }
}

/// A filter compiled to SQL plus its bind parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledFilter {
    /// SQL boolean expression referencing `$1..$n`.
    pub sql: String,
    /// Positional parameters, matching the placeholders in `sql`.
    pub params: Vec<String>,
}

/// Compile a filter expression.
pub fn compile(expr: &Expr, context: &FilterContext, auth: &AuthContext) -> Result<CompiledFilter> {
    let mut params = Vec::new();
    let sql = compile_expr(expr, context, auth, &mut params)?;
    Ok(CompiledFilter { sql, params })
}

fn compile_expr(
    expr: &Expr,
    context: &FilterContext,
    auth: &AuthContext,
    params: &mut Vec<String>,
) -> Result<String> {
    match expr {
        Expr::And(left, right) => Ok(format!(
            "({} and {})",
            compile_expr(left, context, auth, params)?,
            compile_expr(right, context, auth, params)?
        )),
        Expr::Or(left, right) => Ok(format!(
            "({} or {})",
            compile_expr(left, context, auth, params)?,
            compile_expr(right, context, auth, params)?
        )),
        Expr::Compare { path, op, operand } => {
            if path.len() > MAX_PATH_DEPTH {
                return Err(Error::invalid(format!(
                    "filter path `{}` is deeper than the supported {MAX_PATH_DEPTH} levels",
                    path.join(".")
                )));
            }
            let (column, hops) = path.split_last().expect("a comparison always has a path");
            let root = quote_ident(&context.table).to_string();
            let mut alias_counter = 0;
            compile_at(
                context,
                &root,
                hops,
                column,
                *op,
                operand,
                auth,
                params,
                &mut alias_counter,
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn compile_at(
    context: &FilterContext,
    parent: &str,
    hops: &[String],
    column: &str,
    op: CompareOp,
    operand: &Operand,
    auth: &AuthContext,
    params: &mut Vec<String>,
    alias_counter: &mut usize,
) -> Result<String> {
    if hops.is_empty() {
        let column_type = context.columns.get(column).ok_or_else(|| {
            Error::invalid(format!(
                "unknown field {column:?} in collection {:?}",
                context.table
            ))
        })?;
        return compile_operator(
            &format!("{parent}.{}", quote_ident(column)),
            column_type,
            op,
            operand,
            auth,
            params,
        );
    }

    let (relation_name, rest) = hops.split_first().expect("non-empty hops");
    let relation = context.relations.get(relation_name).ok_or_else(|| {
        Error::invalid(format!(
            "{relation_name:?} is not a relation field of collection {:?}",
            context.table
        ))
    })?;

    *alias_counter += 1;
    let alias = format!("rt{alias_counter}");
    let join = if relation.multiple {
        format!(
            "{alias}.\"id\" = any({parent}.{})",
            quote_ident(&relation.column)
        )
    } else {
        format!(
            "{alias}.\"id\" = {parent}.{}",
            quote_ident(&relation.column)
        )
    };

    let inner = compile_at(
        &relation.context,
        &alias,
        rest,
        column,
        op,
        operand,
        auth,
        params,
        alias_counter,
    )?;

    Ok(format!(
        "exists (select 1 from {} {alias} where {join} and {inner})",
        quote_ident(&relation.table)
    ))
}

fn compile_operator(
    target: &str,
    column_type: &str,
    op: CompareOp,
    operand: &Operand,
    auth: &AuthContext,
    params: &mut Vec<String>,
) -> Result<String> {
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

    check_operator_supported(op, column_type, target)?;

    match op {
        CompareOp::Any | CompareOp::NotAny => {
            let value = literal_value(operand, auth)?;
            let placeholder = push_param(value, "text", params);
            let predicate = format!("{placeholder} = any({target})");
            Ok(if op == CompareOp::Any {
                predicate
            } else {
                format!("not ({predicate})")
            })
        }
        CompareOp::Like | CompareOp::NotLike => {
            let value = literal_value(operand, auth)?;
            let escaped = escape_like(&value);
            let placeholder = push_param(escaped, "text", params);
            let keyword = if op == CompareOp::Like {
                "ilike"
            } else {
                "not ilike"
            };
            // Concatenation keeps the wildcards out of the parameter, so a value
            // containing `%` or `_` stays a literal.
            Ok(format!(
                "{target} {keyword} '%' || {placeholder} || '%' escape '\\'"
            ))
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
                // `now()` is a timestamptz; comparing it with anything else would
                // be a runtime type error in Postgres, so refuse it here.
                if column_type != "timestamptz" {
                    return Err(Error::invalid(format!(
                        "`@now` cannot be compared with {target} ({column_type}); \
                         it is only valid for date fields"
                    )));
                }
                return Ok(format!("{target} {sql_operator} now()"));
            }

            let value = literal_value(operand, auth)?;
            let placeholder = push_param(value, cast_for(column_type), params);
            Ok(format!("{target} {sql_operator} {placeholder}"))
        }
    }
}

/// Reject operator/type combinations that would produce a confusing runtime
/// error (or silently wrong results) instead of a clear message.
fn check_operator_supported(op: CompareOp, column_type: &str, target: &str) -> Result<()> {
    let is_array = column_type == "text[]";

    match op {
        CompareOp::Any | CompareOp::NotAny => {
            if !is_array {
                return Err(Error::invalid(format!(
                    "`{}` needs a multiple-value field, but {target} is {column_type}",
                    op.as_str()
                )));
            }
        }
        CompareOp::Like | CompareOp::NotLike => {
            if column_type != "text" {
                return Err(Error::invalid(format!(
                    "`{}` needs a text field, but {target} is {column_type}",
                    op.as_str()
                )));
            }
        }
        CompareOp::Eq | CompareOp::NotEq => {
            if is_array {
                return Err(Error::invalid(format!(
                    "cannot compare {target} ({column_type}) with `{}`; use `?=` or `?!=`",
                    op.as_str()
                )));
            }
        }
        CompareOp::Gt | CompareOp::Gte | CompareOp::Lt | CompareOp::Lte => {
            if is_array || column_type == "jsonb" || column_type == "boolean" {
                return Err(Error::invalid(format!(
                    "`{}` is not supported for {target} ({column_type})",
                    op.as_str()
                )));
            }
        }
    }
    Ok(())
}

/// The value for a bind parameter, as text.
///
/// A missing auth value becomes the empty string: a request that is not
/// authenticated must match nothing rather than everything.
fn literal_value(operand: &Operand, auth: &AuthContext) -> Result<String> {
    Ok(match operand {
        Operand::Text(value) => value.clone(),
        Operand::Number(value) => value.to_string(),
        Operand::Bool(value) => value.to_string(),
        Operand::Auth(reference) => match reference {
            AuthRef::Id => auth.id.clone().unwrap_or_default(),
            AuthRef::Email => auth.email.clone().unwrap_or_default(),
            AuthRef::CollectionId => auth.collection_id.clone().unwrap_or_default(),
            AuthRef::CollectionName => auth.collection_name.clone().unwrap_or_default(),
            AuthRef::Claim(name) => auth.claims.get(name).cloned().unwrap_or_default(),
        },
        Operand::Null => {
            return Err(Error::internal(
                "null reached the literal encoder; it should have been handled earlier",
            ))
        }
        Operand::Now => {
            return Err(Error::internal(
                "`@now` reached the literal encoder; it should have been handled earlier",
            ))
        }
    })
}

fn push_param(value: String, cast: &str, params: &mut Vec<String>) -> String {
    params.push(value);
    let index = params.len();
    if cast == "text" {
        return format!("${index}::text");
    }
    // Parameters are always sent as text. `$n::bigint` would make Postgres infer
    // the *parameter* type as bigint, which the text value cannot serialize as;
    // chaining `$n::text::bigint` pins the parameter to text and converts after,
    // keeping one homogeneous parameter type while the comparison still gets the
    // type it needs.
    format!("${index}::text::{cast}")
}

fn cast_for(column_type: &str) -> &'static str {
    match column_type {
        "bigint" => "bigint",
        "double precision" => "double precision",
        "boolean" => "boolean",
        "timestamptz" => "timestamptz",
        "jsonb" => "jsonb",
        "text[]" => "text[]",
        _ => "text",
    }
}

/// Escape the `LIKE` metacharacters so a user value is matched literally.
fn escape_like(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        if matches!(character, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(character);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::Field;
    use crate::query::filter;
    use crate::types::FieldType;
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

    fn collection(id: &str, name: &str, fields: Vec<Field>) -> Collection {
        Collection {
            id: id.into(),
            name: name.into(),
            collection_type: CollectionType::Base,
            options: json!({}),
            rules: json!({}),
            fields,
            created: SystemTime::now(),
            updated: SystemTime::now(),
        }
    }

    fn fixture() -> (Collection, Vec<Collection>) {
        let users = collection(
            "c_users",
            "users",
            vec![
                field("name", FieldType::Text, json!({})),
                field("age", FieldType::Number, json!({"onlyInt": true})),
            ],
        );
        let posts = collection(
            "c_posts",
            "posts",
            vec![
                field("title", FieldType::Text, json!({})),
                field("views", FieldType::Number, json!({"onlyInt": true})),
                field("ratio", FieldType::Number, json!({})),
                field("published", FieldType::Bool, json!({})),
                field(
                    "tags",
                    FieldType::Select,
                    json!({"values": ["a", "b"], "maxSelect": 5}),
                ),
                field(
                    "author",
                    FieldType::Relation,
                    json!({"collectionId": "c_users"}),
                ),
                field(
                    "reviewers",
                    FieldType::Relation,
                    json!({"collectionId": "c_users", "maxSelect": 3}),
                ),
                field("meta", FieldType::Json, json!({})),
            ],
        );
        let all = vec![users, posts.clone()];
        (posts, all)
    }

    fn compiled(input: &str) -> CompiledFilter {
        let (posts, all) = fixture();
        let context = FilterContext::for_collection(&posts, &all).unwrap();
        let expr = filter::parse(input).unwrap();
        compile(&expr, &context, &AuthContext::default()).unwrap()
    }

    #[test]
    fn values_are_always_parameters_never_inlined() {
        let result = compiled("title = \"abc\"");
        assert_eq!(result.sql, "\"posts\".\"title\" = $1::text");
        assert_eq!(result.params, vec!["abc"]);
    }

    #[test]
    fn quoted_looking_values_stay_data() {
        // The classic injection attempt must end up as a bound parameter.
        let result = compiled("title = \"' or 1=1 --\"");
        assert_eq!(result.params, vec!["' or 1=1 --"]);
        assert!(!result.sql.contains("or 1=1"));
        assert_eq!(result.sql, "\"posts\".\"title\" = $1::text");
    }

    #[test]
    fn the_field_name_is_whitelisted() {
        let (posts, all) = fixture();
        let context = FilterContext::for_collection(&posts, &all).unwrap();

        for bad in ["nope = 1", "title; drop table \"posts\" = 1", "1 = 1"] {
            let expr = filter::parse(bad);
            if let Ok(expr) = expr {
                let err = compile(&expr, &context, &AuthContext::default()).unwrap_err();
                assert!(
                    err.message().contains("unknown field")
                        || err.message().contains("not a relation field")
                        || err.message().contains("expected a field"),
                    "{bad:?} produced {err}"
                );
            }
        }

        // A path through a non-relation field is rejected too.
        let expr = filter::parse("title.name = \"x\"").unwrap();
        let err = compile(&expr, &context, &AuthContext::default()).unwrap_err();
        assert!(err.message().contains("not a relation field"), "{err}");
    }

    #[test]
    fn numbers_and_booleans_get_typed_casts() {
        // Parameters go over the wire as text and are converted by the second
        // cast, so a numeric or boolean column still compares correctly.
        let result = compiled("views >= 10");
        assert_eq!(result.sql, "\"posts\".\"views\" >= $1::text::bigint");
        assert_eq!(result.params, vec!["10"]);

        let result = compiled("ratio < 1.5");
        assert_eq!(
            result.sql,
            "\"posts\".\"ratio\" < $1::text::double precision"
        );
        assert_eq!(result.params, vec!["1.5"]);

        let result = compiled("published = true");
        assert_eq!(result.sql, "\"posts\".\"published\" = $1::text::boolean");
        assert_eq!(result.params, vec!["true"]);
    }

    #[test]
    fn null_uses_is_null_and_needs_no_parameter() {
        assert_eq!(compiled("title = null").sql, "\"posts\".\"title\" is null");
        assert_eq!(
            compiled("title != null").sql,
            "\"posts\".\"title\" is not null"
        );
        assert!(compiled("title = null").params.is_empty());

        let (posts, all) = fixture();
        let context = FilterContext::for_collection(&posts, &all).unwrap();
        let expr = filter::parse("title > null").unwrap();
        let err = compile(&expr, &context, &AuthContext::default()).unwrap_err();
        assert!(
            err.message().contains("cannot be combined with null"),
            "{err}"
        );
    }

    #[test]
    fn contains_is_case_insensitive_and_escapes_wildcards() {
        let result = compiled("title ~ \"Hello\"");
        assert_eq!(
            result.sql,
            "\"posts\".\"title\" ilike '%' || $1::text || '%' escape '\\'"
        );
        assert_eq!(result.params, vec!["Hello"]);

        // `%` and `_` are literal, not wildcards.
        let result = compiled("title ~ \"50%_off\"");
        assert_eq!(result.params, vec!["50\\%\\_off"]);
        assert_eq!(
            result.sql,
            "\"posts\".\"title\" ilike '%' || $1::text || '%' escape '\\'"
        );

        let result = compiled("title !~ \"x\"");
        assert!(result.sql.contains("not ilike"), "{}", result.sql);
    }

    #[test]
    fn any_of_operators_target_array_columns() {
        let result = compiled("tags ?= \"a\"");
        assert_eq!(result.sql, "$1::text = any(\"posts\".\"tags\")");
        let result = compiled("tags ?!= \"a\"");
        assert_eq!(result.sql, "not ($1::text = any(\"posts\".\"tags\"))");

        // ... and are refused on scalar columns.
        let (posts, all) = fixture();
        let context = FilterContext::for_collection(&posts, &all).unwrap();
        let expr = filter::parse("title ?= \"a\"").unwrap();
        let err = compile(&expr, &context, &AuthContext::default()).unwrap_err();
        assert!(err.message().contains("multiple-value field"), "{err}");

        // Comparing an array with `=` is refused with a pointer to `?=`.
        let expr = filter::parse("tags = \"a\"").unwrap();
        let err = compile(&expr, &context, &AuthContext::default()).unwrap_err();
        assert!(err.message().contains("?="), "{err}");
    }

    #[test]
    fn auth_references_become_parameters() {
        let (posts, all) = fixture();
        let context = FilterContext::for_collection(&posts, &all).unwrap();
        let auth = AuthContext {
            id: Some("user1".into()),
            claims: HashMap::from([("realm".to_string(), "acme".to_string())]),
            ..Default::default()
        };

        let expr = filter::parse("author = @request.auth.id").unwrap();
        let result = compile(&expr, &context, &auth).unwrap();
        assert_eq!(result.sql, "\"posts\".\"author\" = $1::text");
        assert_eq!(result.params, vec!["user1"]);

        // A missing auth value matches nothing rather than everything.
        let unauth = compile(&expr, &context, &AuthContext::default()).unwrap();
        assert_eq!(unauth.params, vec![""]);
    }

    #[test]
    fn now_compiles_to_the_database_clock() {
        assert_eq!(
            compiled("updated > @now").sql,
            "\"posts\".\"updated\" > now()"
        );
        // `@now` against a non-temporal column is refused.
        let (posts, all) = fixture();
        let context = FilterContext::for_collection(&posts, &all).unwrap();
        let expr = filter::parse("views > @now").unwrap();
        let err = compile(&expr, &context, &AuthContext::default()).unwrap_err();
        assert!(
            err.message().contains("cannot be compared") || err.message().contains("timestamptz"),
            "{err}"
        );
    }

    #[test]
    fn dotted_relations_become_exists_subqueries() {
        let result = compiled("author.name = \"x\"");
        assert_eq!(result.sql, "exists (select 1 from \"users\" rt1 where rt1.\"id\" = \"posts\".\"author\" and rt1.\"name\" = $1::text)");
        assert_eq!(result.params, vec!["x"]);
    }

    #[test]
    fn multi_relations_join_with_any() {
        let result = compiled("reviewers.name = \"x\"");
        assert!(
            result
                .sql
                .contains("rt1.\"id\" = any(\"posts\".\"reviewers\")"),
            "{}",
            result.sql
        );
    }

    #[test]
    fn logical_operators_nest_with_parentheses() {
        let result = compiled("title = \"a\" || views > 1 && published = true");
        assert_eq!(
            result.sql,
            "(\"posts\".\"title\" = $1::text or (\"posts\".\"views\" > $2::text::bigint and \"posts\".\"published\" = $3::text::boolean))"
        );
        assert_eq!(result.params, vec!["a", "1", "true"]);
    }

    #[test]
    fn placeholders_are_numbered_in_order() {
        let result = compiled("title = \"a\" && views = 1 && ratio = 2.5 && title != \"b\"");
        assert!(result.sql.contains("$1::text"));
        assert!(result.sql.contains("$2::text::bigint"));
        assert!(result.sql.contains("$3::text::double precision"));
        assert!(result.sql.contains("$4::text"));
        assert_eq!(result.params.len(), 4);
    }

    #[test]
    fn unreasonable_operator_type_combinations_are_refused() {
        let (posts, all) = fixture();
        let context = FilterContext::for_collection(&posts, &all).unwrap();
        let cases = [
            ("meta > 1", "jsonb"),
            ("published > true", "boolean"),
            ("views > @now", "date fields"),
        ];
        for (input, expected) in cases {
            let expr = filter::parse(input).unwrap();
            let err = compile(&expr, &context, &AuthContext::default()).unwrap_err();
            assert!(err.message().contains(expected), "{input} produced {err}");
        }

        // Ordering on text is allowed: Postgres orders text meaningfully.
        let expr = filter::parse("title > 1").unwrap();
        assert!(compile(&expr, &context, &AuthContext::default()).is_ok());
    }

    #[test]
    fn a_dangling_relation_target_only_disables_the_dotted_form() {
        let posts = collection(
            "c_posts",
            "posts",
            vec![field(
                "author",
                FieldType::Relation,
                json!({"collectionId": "c_missing"}),
            )],
        );
        let context = FilterContext::for_collection(&posts, std::slice::from_ref(&posts)).unwrap();

        // The plain column still works.
        let expr = filter::parse("author = \"x\"").unwrap();
        assert!(compile(&expr, &context, &AuthContext::default()).is_ok());

        // The dotted form is refused because the target is unknown.
        let expr = filter::parse("author.name = \"x\"").unwrap();
        let err = compile(&expr, &context, &AuthContext::default()).unwrap_err();
        assert!(err.message().contains("not a relation field"), "{err}");
    }

    #[test]
    fn escaping_like_metacharacters_is_exhaustive() {
        assert_eq!(escape_like("plain"), "plain");
        assert_eq!(escape_like("100%"), "100\\%");
        assert_eq!(escape_like("a_b"), "a\\_b");
        assert_eq!(escape_like("back\\slash"), "back\\\\slash");
    }
}
