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

use super::filter::{AuthRef, CompareOp, Expr, Operand, Side};
use crate::collections::Collection;
use crate::ddl::quote_ident;
use crate::types::{self, CollectionType, FieldType};

/// Maximum depth of a dotted relation path inside a filter.
pub const MAX_PATH_DEPTH: usize = 4;

/// The columns a collection has before a single field is declared, as
/// `(name, field type)`.
///
/// `id`/`created`/`updated` are table columns with no `_fields` row, so a client that
/// reads only a collection's `fields` cannot see them — which is why the API serves
/// them as fields as well. They are listed here, next to the context that consumes
/// them, so what a client may name in a filter and what it sees in the definition
/// cannot drift apart. `email` and `verified` are not here: an auth collection has
/// real `_fields` rows for those.
///
/// A view is the exception. It has an `id` because the query must provide one, but
/// `created`/`updated` are table columns rather than something a query guarantees,
/// and assuming them made every view without them fail its default sort.
pub fn implicit_fields(collection: &Collection) -> Vec<(&'static str, FieldType)> {
    let mut fields = vec![("id", FieldType::Text)];
    if collection.collection_type != CollectionType::View {
        fields.push(("created", FieldType::Autodate));
        fields.push(("updated", FieldType::Autodate));
    }
    fields
}

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
    /// Table name (unquoted). For a collection this is also its name, which is what
    /// the record's `collectionName` key reports.
    pub table: String,
    /// The collection's id, reported as the record's `collectionId` key. Empty for a
    /// context that is not a collection (the `_files` rules context), which then
    /// returns no `collectionId`/`collectionName` keys.
    pub collection_id: String,
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
        // A view has only the columns its query selects; for anything else these are
        // `id`, `created` and `updated`.
        for (name, field_type) in implicit_fields(collection) {
            columns.insert(
                name.to_string(),
                types::column_type(field_type, &serde_json::json!({}))?.to_string(),
            );
        }
        // `email` and `verified` are ordinary columns. `password_hash` and
        // `token_key` are deliberately *absent*: everything in this map is both
        // filterable and, with `fields=*`, returned in the record document — and
        // a credential hash must never leave the server. Keeping them out also
        // means a filter naming them is rejected as an unknown field.
        if collection.collection_type == CollectionType::Auth {
            columns.insert("email".into(), "text".into());
            columns.insert("verified".into(), "boolean".into());
        }

        let mut relations = HashMap::new();

        for field in &collection.fields {
            // This map is what is filterable *and* what `fields=*` returns, so a
            // credential in it is a credential returned to a client. The auth fields
            // are recorded now — an account's email and password are part of its
            // definition — and without this the password hash would be back in the
            // document, which is what the credential test caught.
            if collection.collection_type.is_auth()
                && types::CREDENTIAL_COLUMNS.contains(&field.name.as_str())
            {
                continue;
            }
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

            // Through the shared accessor, not off `options` directly. This was the
            // **third** place reading the target, and the only one the request
            // reached: the resolver and the DDL both looked it up one way while this
            // looked for `collectionId` alone, so a relation naming its target by
            // *name* — which is what Studio sends — was accepted by validation, given
            // a foreign key, and then refused here.
            //
            // A single accessor would have made that impossible, which is why there is
            // now one, and why every reader goes through it.
            let target_id = types::relation_target(&field.options).ok_or_else(|| {
                Error::invalid(format!(
                    "relation field {:?} has no collectionId",
                    field.name
                ))
            })?;
            // By id **or name**: PocketBase stores the target's id, but the Studio's
            // relation editor sends `options.collection` with a name, and that is
            // what is stored. Resolving by id alone left the field out of `relations`
            // entirely, so `expand=author` was refused as "not a relation field" and
            // the Studio's record list never loaded.
            let target = all
                .iter()
                .find(|candidate| candidate.id == target_id || candidate.name == target_id);
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
            collection_id: collection.id.clone(),
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
    compile_with_root(expr, context, auth, &quote_ident(&context.table))
}

/// Compile a filter expression against an explicit root table reference.
///
/// `root` is the already-quoted SQL reference for the collection's row: either
/// the table name, or — for callers that alias the table, such as the record
/// reader — the alias. Postgres rejects the table name once an alias is in
/// effect, so the two must agree.
///
pub fn compile_with_root(
    expr: &Expr,
    context: &FilterContext,
    auth: &AuthContext,
    root: &str,
) -> Result<CompiledFilter> {
    let mut params = Vec::new();
    let sql = compile_expr(expr, context, auth, root, &mut params)?;
    Ok(CompiledFilter { sql, params })
}

fn compile_expr(
    expr: &Expr,
    context: &FilterContext,
    auth: &AuthContext,
    root: &str,
    params: &mut Vec<String>,
) -> Result<String> {
    match expr {
        Expr::And(left, right) => Ok(format!(
            "({} and {})",
            compile_expr(left, context, auth, root, params)?,
            compile_expr(right, context, auth, root, params)?
        )),
        Expr::Or(left, right) => Ok(format!(
            "({} or {})",
            compile_expr(left, context, auth, root, params)?,
            compile_expr(right, context, auth, root, params)?
        )),
        Expr::Compare { left, op, right } => {
            compile_comparison(left, *op, right, context, auth, root, params)
        }
    }
}

/// Compile one comparison, whichever side the field is on.
fn compile_comparison(
    left: &Side,
    op: CompareOp,
    right: &Side,
    context: &FilterContext,
    auth: &AuthContext,
    root: &str,
    params: &mut Vec<String>,
) -> Result<String> {
    match (left, right) {
        (Side::Path(path), Side::Value(operand)) => {
            compile_path(path, op, operand, false, context, auth, root, params)
        }
        // `@request.auth.id != ""`, `@request.auth.role = "admin"`: the documented
        // rule shape, with the request reference on the left.
        (Side::Value(operand), Side::Path(path)) => {
            compile_path(path, op, operand, true, context, auth, root, params)
        }
        (Side::Value(left), Side::Value(right)) => compile_values(left, op, right, auth, params),
        (Side::Path(left), Side::Path(right)) => Err(Error::unsupported(format!(
            "a filter cannot compare two fields (`{}` and `{}`); compare a field with a \
             value instead",
            left.join("."),
            right.join(".")
        ))),
    }
}

#[allow(clippy::too_many_arguments)]
fn compile_path(
    path: &[String],
    op: CompareOp,
    operand: &Operand,
    swapped: bool,
    context: &FilterContext,
    auth: &AuthContext,
    root: &str,
    params: &mut Vec<String>,
) -> Result<String> {
    if path.len() > MAX_PATH_DEPTH {
        return Err(Error::invalid(format!(
            "filter path `{}` is deeper than the supported {MAX_PATH_DEPTH} levels",
            path.join(".")
        )));
    }
    let (column, hops) = path.split_last().expect("a comparison always has a path");
    let mut alias_counter = 0;
    compile_at(
        context,
        root,
        hops,
        column,
        op,
        operand,
        swapped,
        auth,
        params,
        &mut alias_counter,
    )
}

#[allow(clippy::too_many_arguments)]
fn compile_at(
    context: &FilterContext,
    parent: &str,
    hops: &[String],
    column: &str,
    op: CompareOp,
    operand: &Operand,
    swapped: bool,
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
            swapped,
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
        swapped,
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
    swapped: bool,
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
            // `?=` reads "any of the field's values equals the operand", so it is the
            // same predicate whichever side the value was written on.
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
            if swapped {
                // `@request.auth.email ~ field`: the reference is the haystack and the
                // column is the needle, so the pattern cannot be escaped — a wildcard
                // in the column's own value is the data's, not the caller's, and
                // matching it as a wildcard is what the expression asks for.
                return Ok(format!(
                    "{placeholder} {keyword} '%' || {target} || '%' escape '\\'"
                ));
            }
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
            let sql_operator = sql_operator(op);

            if matches!(operand, Operand::Now) {
                // `now()` is a timestamptz; comparing it with anything else would
                // be a runtime type error in Postgres, so refuse it here.
                if column_type != "timestamptz" {
                    return Err(Error::invalid(format!(
                        "`@now` cannot be compared with {target} ({column_type}); \
                         it is only valid for date fields"
                    )));
                }
                return Ok(if swapped {
                    format!("now() {sql_operator} {target}")
                } else {
                    format!("{target} {sql_operator} now()")
                });
            }

            let value = literal_value(operand, auth)?;
            let placeholder = push_param(value, cast_for(column_type), params);
            Ok(if swapped {
                format!("{placeholder} {sql_operator} {target}")
            } else {
                format!("{target} {sql_operator} {placeholder}")
            })
        }
    }
}

/// Compile a comparison in which both sides are values: `@request.auth.id != ""`,
/// `@request.auth.role = "admin"`.
///
/// Such a predicate does not touch the row at all — it is entirely about the request —
/// which is exactly what PocketBase's documented `createRule: "@request.auth.id != ''"`
/// is. The same typing rules as a column comparison apply: `@now` is a timestamp, a
/// number compares numerically, and everything else compares as text.
fn compile_values(
    left: &Operand,
    op: CompareOp,
    right: &Operand,
    auth: &AuthContext,
    params: &mut Vec<String>,
) -> Result<String> {
    if matches!(left, Operand::Null) || matches!(right, Operand::Null) {
        return Err(Error::invalid(
            "a comparison with null needs a field on one side; there is nothing to test \
             for `is null` here",
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
        let haystack = value_sql(left, "text", auth, params)?;
        let needle = escape_like(&literal_value(right, auth)?);
        let placeholder = push_param(needle, "text", params);
        return Ok(format!(
            "{haystack} {keyword} '%' || {placeholder} || '%' escape '\\'"
        ));
    }

    let cast = if matches!(left, Operand::Now) || matches!(right, Operand::Now) {
        "timestamptz"
    } else if matches!(left, Operand::Number(_)) || matches!(right, Operand::Number(_)) {
        "double precision"
    } else {
        "text"
    };
    let left_sql = value_sql(left, cast, auth, params)?;
    let right_sql = value_sql(right, cast, auth, params)?;
    Ok(format!("{left_sql} {} {right_sql}", sql_operator(op)))
}

/// The SQL for one value side of a comparison, bound as a parameter (or `now()`).
fn value_sql(
    operand: &Operand,
    cast: &str,
    auth: &AuthContext,
    params: &mut Vec<String>,
) -> Result<String> {
    if matches!(operand, Operand::Now) {
        return Ok("now()".to_string());
    }
    let value = literal_value(operand, auth)?;
    Ok(push_param(value, cast, params))
}

fn sql_operator(op: CompareOp) -> &'static str {
    match op {
        CompareOp::Eq => "=",
        CompareOp::NotEq => "<>",
        CompareOp::Gt => ">",
        CompareOp::Gte => ">=",
        CompareOp::Lt => "<",
        CompareOp::Lte => "<=",
        CompareOp::Like => "ilike",
        CompareOp::NotLike => "not ilike",
        CompareOp::Any => "= any",
        CompareOp::NotAny => "<> all",
    }
}

/// Reject operator/type combinations that would produce a confusing runtime
/// error (or silently wrong results) instead of a clear message.
pub fn check_operator_supported(op: CompareOp, column_type: &str, target: &str) -> Result<()> {
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

pub fn cast_for(column_type: &str) -> &'static str {
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
pub fn escape_like(value: &str) -> String {
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
    fn a_request_reference_on_the_left_compiles_to_a_bound_comparison() {
        let (posts, all) = fixture();
        let context = FilterContext::for_collection(&posts, &all).unwrap();
        let auth = AuthContext {
            id: Some("user1".into()),
            claims: HashMap::from([("role".to_string(), "admin".to_string())]),
            ..Default::default()
        };

        // `@request.auth.id != ""` — no column at all, the documented rule shape.
        let expr = filter::parse("@request.auth.id != ''").unwrap();
        let result = compile(&expr, &context, &auth).unwrap();
        assert_eq!(result.sql, "$1::text <> $2::text");
        assert_eq!(result.params, vec!["user1", ""]);

        let expr = filter::parse("@request.auth.role = 'admin'").unwrap();
        let result = compile(&expr, &context, &auth).unwrap();
        assert_eq!(result.sql, "$1::text = $2::text");
        assert_eq!(result.params, vec!["admin", "admin"]);

        // ... and an unauthenticated request matches nothing, not everything.
        let unauth = compile(
            &filter::parse("@request.auth.id != ''").unwrap(),
            &context,
            &AuthContext::default(),
        )
        .unwrap();
        assert_eq!(unauth.params, vec!["", ""]);
    }

    #[test]
    fn a_request_reference_may_be_compared_with_a_field() {
        let (posts, all) = fixture();
        let context = FilterContext::for_collection(&posts, &all).unwrap();
        let auth = AuthContext {
            id: Some("user1".into()),
            ..Default::default()
        };

        let expr = filter::parse("@request.auth.id = author").unwrap();
        let result = compile(&expr, &context, &auth).unwrap();
        assert_eq!(result.sql, "$1::text = \"posts\".\"author\"");
        assert_eq!(result.params, vec!["user1"]);

        assert_eq!(
            compiled("@now < updated").sql,
            "now() < \"posts\".\"updated\""
        );
    }

    #[test]
    fn comparing_two_fields_is_refused() {
        let (posts, all) = fixture();
        let context = FilterContext::for_collection(&posts, &all).unwrap();
        let expr = filter::parse("title = author").unwrap();
        let err = compile(&expr, &context, &AuthContext::default()).unwrap_err();
        assert!(err.message().contains("two fields"), "{err}");
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
    fn a_relation_may_name_its_target_instead_of_identifying_it() {
        // The Studio sends `{"collection": "users"}`, a name; the field must still
        // land in `relations`, or `expand` refuses it and the record list fails.
        let posts = collection(
            "c_posts",
            "posts",
            vec![field(
                "author",
                FieldType::Relation,
                json!({"collection": "users"}),
            )],
        );
        let users = collection(
            "c_users",
            "users",
            vec![field("name", FieldType::Text, json!({}))],
        );
        let all = vec![users, posts.clone()];
        let context = FilterContext::for_collection(&posts, &all).unwrap();

        assert!(
            context.relations.contains_key("author"),
            "a name-targeted relation must be resolvable"
        );
        let expr = filter::parse("author.name = \"x\"").unwrap();
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
