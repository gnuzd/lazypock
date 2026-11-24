//! Reading records: filter + sort + pagination, under row-level security.
//!
//! Rows come back as JSON built by Postgres (`to_jsonb`), so this crate never
//! needs per-type decoding. Only [`Collection::visible_columns`] are selected:
//! credentials (`password_hash`, `token_key`) and hidden fields are never read.

use crate::error::PgExt;
use crate::filter::{self, Mode};
use crate::params::query_values;
use crate::pool::Db;
use crate::rls::{self, Action, RequestContext};
use crate::schema::Collection;
use crate::sort;
use crate::types::CollectionType;
use sea_query::{Alias, Expr, PostgresQueryBuilder, Query};
use serde::Serialize;
use tokio_postgres::GenericClient;

use lazypock_core::Result;

pub const MAX_PER_PAGE: u32 = 500;

#[derive(Clone, Copy, Debug)]
pub struct ListParams<'a> {
    pub filter: Option<&'a str>,
    pub sort: Option<&'a str>,
    pub page: u32,
    pub per_page: u32,
}

impl Default for ListParams<'_> {
    fn default() -> Self {
        ListParams {
            filter: None,
            sort: None,
            page: 1,
            per_page: 30,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub page: u32,
    pub per_page: u32,
    pub total_items: i64,
    pub items: Vec<serde_json::Value>,
}

/// Run on a transaction from [`rls::begin`] with `Action::List`.
pub async fn list<C: GenericClient>(
    client: &C,
    c: &Collection,
    p: &ListParams<'_>,
) -> Result<Page> {
    let per_page = p.per_page.clamp(1, MAX_PER_PAGE);
    let page = p.page.max(1);

    let cond = match p.filter.map(str::trim).filter(|s| !s.is_empty()) {
        Some(f) => Some(filter::compile(c, &filter::parse(f)?, Mode::Bind)?),
        None => None,
    };
    let order = sort::parse(c, p.sort)?;

    let mut inner = Query::select();
    for col in c.visible_columns() {
        inner.column(Alias::new(col));
    }
    inner.from(c.name.alias());
    let mut count = Query::select();
    count.expr(Expr::cust("count(*)")).from(c.name.alias());
    if let Some(cond) = cond {
        inner.and_where(cond.clone());
        count.and_where(cond);
    }
    for (col, dir) in order {
        inner.order_by(col, dir);
    }
    inner
        .limit(u64::from(per_page))
        .offset(u64::from(page - 1) * u64::from(per_page));

    let (isql, ivals) = inner.build(PostgresQueryBuilder);
    let rows = query_values(
        client,
        &format!("select to_jsonb(t) from ({isql}) as t"),
        &ivals,
    )
    .await?;
    let items = rows
        .iter()
        .map(|r| r.get::<_, serde_json::Value>(0))
        .collect();

    let (csql, cvals) = count.build(PostgresQueryBuilder);
    let total_items: i64 = query_values(client, &csql, &cvals).await?[0].get(0);

    Ok(Page {
        page,
        per_page,
        total_items,
        items,
    })
}

/// Convenience: one connection, one transaction, the right role and action.
pub async fn list_as(
    db: &Db,
    c: &Collection,
    ctx: &RequestContext,
    p: &ListParams<'_>,
) -> Result<Page> {
    let mut conn = db.conn().await?;
    let tx = rls::begin(&mut conn, &ctx.clone().with_action(Action::List)).await?;
    let page = list(&tx, c, p).await?;
    tx.commit().await.pg()?;
    Ok(page)
}

// ============================================================================
// Writes
// ============================================================================
//
// Every write runs on a transaction from `rls::begin` (role + `request.*` set), so
// Postgres enforces the collection's rules: `createRule` is the INSERT policy's
// WITH CHECK, `updateRule` / `deleteRule` their USING, and a row the caller may not
// touch is simply not there. The `*_as` wrappers do the whole begin/run/commit.
//
// Values travel as text with an explicit cast (`$2::text::bigint`) through
// `query_typed`, so nothing is prepared under a name (poolers) and no value is ever
// part of the SQL text.

use crate::error;
use crate::params::typed;
use crate::schema::{CREDENTIAL_COLUMNS, Field};
use crate::types::FieldType;
use lazypock_core::id::new_id;
use serde_json::{Map, Value};
use tokio_postgres::Transaction;

/// A column the caller supplies directly (credentials the API prepared), as
/// `(column, text value)`. Never taken from a request body.
pub type Extra<'a> = (&'a str, &'a str);

/// What a client may write: visible, non-credential fields of a type this crate can store.
fn writable(c: &Collection) -> Vec<&Field> {
    c.fields
        .iter()
        .filter(|f| {
            !f.hidden()
                && f.ty != FieldType::Password
                && !CREDENTIAL_COLUMNS.contains(&f.name.as_str())
                && f.ty.column_type(&f.options).is_ok()
        })
        .collect()
}

fn cast_for(f: &Field) -> &'static str {
    match f.ty.column_type(&f.options).unwrap_or("text") {
        "bigint" => "bigint",
        "double precision" => "double precision",
        "boolean" => "boolean",
        "timestamptz" => "timestamptz",
        "jsonb" => "jsonb",
        _ => "text",
    }
}

fn bad(msg: impl std::fmt::Display) -> lazypock_core::Error {
    lazypock_core::Error::Validation(msg.to_string())
}

fn valid_email(s: &str) -> bool {
    s.len() <= 254
        && !s.chars().any(|c| c.is_whitespace() || c.is_control())
        && s.split_once('@').is_some_and(|(l, d)| {
            !l.is_empty()
                && d.contains('.')
                && !d.starts_with('.')
                && !d.ends_with('.')
                && !d.contains('@')
        })
}

/// The text to bind for one field, `None` for SQL NULL.
fn field_text(f: &Field, v: &Value) -> Result<Option<String>> {
    let name = f.name.as_str();
    if v.is_null() {
        return if f.required {
            Err(bad(format!("{name} is required")))
        } else {
            Ok(None)
        };
    }
    let wrong = |expected: &str| bad(format!("{name} must be {expected}"));
    Ok(Some(match f.ty {
        FieldType::Text | FieldType::Editor => {
            let s = v.as_str().ok_or_else(|| wrong("a string"))?;
            if f.required && s.is_empty() {
                return Err(bad(format!("{name} is required")));
            }
            s.to_string()
        }
        FieldType::Email => {
            let s = v.as_str().ok_or_else(|| wrong("a string"))?.trim();
            if s.is_empty() && !f.required {
                return Ok(None);
            }
            if !valid_email(s) {
                return Err(bad(format!("{name} must be a valid email address")));
            }
            s.to_string()
        }
        FieldType::Url => {
            let s = v.as_str().ok_or_else(|| wrong("a string"))?;
            let host = s
                .strip_prefix("https://")
                .or_else(|| s.strip_prefix("http://"));
            if s.is_empty() && !f.required {
                return Ok(None);
            }
            if !host.is_some_and(|h| {
                !h.is_empty() && !h.starts_with('/') && !s.chars().any(char::is_whitespace)
            }) {
                return Err(bad(format!("{name} must be an http(s) URL")));
            }
            s.to_string()
        }
        FieldType::Select => {
            let s = v.as_str().ok_or_else(|| wrong("a string"))?;
            if let Some(allowed) = f.options.get("values").and_then(Value::as_array) {
                if !allowed.iter().any(|a| a.as_str() == Some(s)) {
                    return Err(bad(format!("{name} is not one of the allowed values")));
                }
            }
            s.to_string()
        }
        FieldType::Number => {
            let n = v.as_number().ok_or_else(|| wrong("a number"))?;
            if crate::types::option_bool(&f.options, "onlyInt") {
                match n.as_i64() {
                    Some(i) => i.to_string(),
                    None => match n.as_f64() {
                        Some(x) if x.fract() == 0.0 && x.abs() < 9.0e15 => (x as i64).to_string(),
                        _ => return Err(wrong("an integer")),
                    },
                }
            } else {
                n.to_string()
            }
        }
        FieldType::Bool => v
            .as_bool()
            .ok_or_else(|| wrong("true or false"))?
            .to_string(),
        FieldType::Date => {
            let s = v.as_str().ok_or_else(|| wrong("a date string"))?;
            if s.is_empty() && !f.required {
                return Ok(None);
            }
            s.to_string()
        }
        FieldType::Json => v.to_string(),
        _ => return Err(bad(format!("{name} cannot be written"))),
    }))
}

fn object(body: &Value) -> Result<&Map<String, Value>> {
    body.as_object()
        .ok_or_else(|| bad("the request body must be a JSON object"))
}

/// Check the body against the writable fields; unknown keys are an error, so a
/// typo is reported instead of silently dropped.
fn checked<'a>(
    c: &'a Collection,
    body: &'a Map<String, Value>,
    allow_id: bool,
) -> Result<Vec<(&'a Field, &'a Value)>> {
    let fields = writable(c);
    let mut out = Vec::new();
    for (key, value) in body {
        if key == "id" && allow_id {
            continue;
        }
        match fields.iter().find(|f| f.name.as_str() == key) {
            Some(f) => out.push((*f, value)),
            None => return Err(bad(format!("{key:?} is not a field you can set"))),
        }
    }
    Ok(out)
}

fn valid_id(id: &str) -> bool {
    id.len() == 15
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

fn select_cols(c: &Collection) -> String {
    c.visible_columns()
        .iter()
        .map(|col| format!("\"{col}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

async fn read_back<C: GenericClient>(
    client: &C,
    c: &Collection,
    id: &str,
) -> Result<Option<Value>> {
    view(client, c, id).await
}

/// One record by id, as whatever role the transaction runs as.
pub async fn view<C: GenericClient>(client: &C, c: &Collection, id: &str) -> Result<Option<Value>> {
    let sql = format!(
        "select to_jsonb(t) from (select {} from {} where \"id\" = $1::text) t",
        select_cols(c),
        c.name.quoted()
    );
    let rows = client.query_typed(&sql, &[typed!(id, TEXT)]).await.pg()?;
    Ok(rows.first().map(|r| r.get::<_, Value>(0)))
}

/// Insert. `extra` columns (credentials, prepared by the API) are appended after the
/// validated body. Returns the stored record, read back as the owner.
pub async fn create(
    tx: &Transaction<'_>,
    c: &Collection,
    body: &Value,
    extra: &[Extra<'_>],
) -> Result<Value> {
    if c.ty == CollectionType::View {
        return Err(bad("views are read-only"));
    }
    let body = object(body)?;
    let given = checked(c, body, true)?;

    let id = match body.get("id") {
        None => new_id(),
        Some(v) => match v.as_str() {
            Some(s) if valid_id(s) => s.to_string(),
            _ => return Err(bad("id must be 15 lowercase letters or digits")),
        },
    };
    for f in writable(c) {
        let supplied = body.get(f.name.as_str()).is_some_and(|v| !v.is_null());
        if f.required && !supplied {
            return Err(bad(format!("{} is required", f.name)));
        }
    }

    let mut cols = vec!["\"id\"".to_string()];
    let mut marks = vec!["$1::text".to_string()];
    let mut params: Vec<Option<String>> = vec![Some(id.clone())];
    for (f, v) in given {
        cols.push(f.name.quoted());
        push_value(&mut marks, &mut params, field_text(f, v)?, cast_for(f));
    }
    for (name, value) in extra {
        cols.push(format!("\"{}\"", name.replace('"', "")));
        push_value(&mut marks, &mut params, Some((*value).to_string()), "text");
    }

    let sql = format!(
        "insert into {} ({}) values ({})",
        c.name.quoted(),
        cols.join(", "),
        marks.join(", ")
    );
    run(tx, &sql, &params).await?;

    rls::assume_owner(tx).await?;
    read_back(tx, c, &id).await?.ok_or_else(|| {
        lazypock_core::Error::Internal("the record was written but could not be read back".into())
    })
}

/// Update the given fields. `None` when no such record is visible to the caller.
pub async fn update(
    tx: &Transaction<'_>,
    c: &Collection,
    id: &str,
    body: &Value,
    extra: &[Extra<'_>],
) -> Result<Option<Value>> {
    if c.ty == CollectionType::View {
        return Err(bad("views are read-only"));
    }
    let given = checked(c, object(body)?, false)?;

    let mut sets = vec!["\"updated\" = now()".to_string()];
    let mut marks = vec!["$1::text".to_string()];
    let mut params: Vec<Option<String>> = vec![Some(id.to_string())];
    for (f, v) in given {
        let text = field_text(f, v)?;
        sets.push(format!(
            "{} = {}",
            f.name.quoted(),
            placeholder(&mut marks, &mut params, text, cast_for(f))
        ));
    }
    for (name, value) in extra {
        sets.push(format!(
            "\"{}\" = {}",
            name.replace('"', ""),
            placeholder(&mut marks, &mut params, Some((*value).to_string()), "text")
        ));
    }

    let sql = format!(
        "update {} set {} where \"id\" = $1::text returning \"id\"",
        c.name.quoted(),
        sets.join(", ")
    );
    if run(tx, &sql, &params).await?.is_empty() {
        return Ok(None);
    }
    rls::assume_owner(tx).await?;
    read_back(tx, c, id).await
}

/// Delete. `false` when no such record is visible to the caller.
pub async fn delete(tx: &Transaction<'_>, c: &Collection, id: &str) -> Result<bool> {
    if c.ty == CollectionType::View {
        return Err(bad("views are read-only"));
    }
    let sql = format!(
        "delete from {} where \"id\" = $1::text returning \"id\"",
        c.name.quoted()
    );
    Ok(!run(tx, &sql, &[Some(id.to_string())]).await?.is_empty())
}

fn push_value(
    marks: &mut Vec<String>,
    params: &mut Vec<Option<String>>,
    value: Option<String>,
    cast: &str,
) {
    let m = placeholder(marks, params, value, cast);
    marks.push(m);
}

/// A `$n::text::cast` (or `NULL`) expression for `value`; the parameter is recorded.
fn placeholder(
    _marks: &mut Vec<String>,
    params: &mut Vec<Option<String>>,
    value: Option<String>,
    cast: &str,
) -> String {
    match value {
        None => "NULL".to_string(),
        Some(v) => {
            params.push(Some(v));
            if cast == "text" {
                format!("${}::text", params.len())
            } else {
                format!("${}::text::{cast}", params.len())
            }
        }
    }
}

async fn run(
    tx: &Transaction<'_>,
    sql: &str,
    params: &[Option<String>],
) -> Result<Vec<tokio_postgres::Row>> {
    use tokio_postgres::types::{ToSql, Type};
    let typed: Vec<(&(dyn ToSql + Sync), Type)> = params
        .iter()
        .map(|p| (p as &(dyn ToSql + Sync), Type::TEXT))
        .collect();
    tx.query_typed(sql, &typed).await.map_err(error::pg_error)
}

// ---- begin/run/commit wrappers ---------------------------------------------------

/// begin (role + action) / run / commit, rolling back on error.
macro_rules! in_tx {
    ($db:expr, $ctx:expr, $action:expr, |$tx:ident| $body:expr) => {{
        let mut conn = $db.conn().await?;
        let $tx = rls::begin(&mut conn, &$ctx.clone().with_action($action)).await?;
        match $body.await {
            Ok(v) => {
                $tx.commit().await.pg()?;
                Ok(v)
            }
            Err(e) => {
                let _ = $tx.rollback().await;
                Err(e)
            }
        }
    }};
}

pub async fn view_as(
    db: &Db,
    c: &Collection,
    ctx: &RequestContext,
    id: &str,
) -> Result<Option<Value>> {
    in_tx!(db, ctx, Action::View, |tx| view(&tx, c, id))
}

pub async fn create_as(
    db: &Db,
    c: &Collection,
    ctx: &RequestContext,
    body: &Value,
    extra: &[Extra<'_>],
) -> Result<Value> {
    in_tx!(db, ctx, Action::Write, |tx| create(&tx, c, body, extra))
}

pub async fn update_as(
    db: &Db,
    c: &Collection,
    ctx: &RequestContext,
    id: &str,
    body: &Value,
    extra: &[Extra<'_>],
) -> Result<Option<Value>> {
    in_tx!(db, ctx, Action::Update, |tx| update(
        &tx, c, id, body, extra
    ))
}

pub async fn delete_as(db: &Db, c: &Collection, ctx: &RequestContext, id: &str) -> Result<bool> {
    in_tx!(db, ctx, Action::Delete, |tx| delete(&tx, c, id))
}

#[cfg(test)]
mod write_tests {
    use super::*;
    use crate::types::CollectionType;
    use serde_json::json;

    fn field(name: &str, ty: FieldType, required: bool, options: Value) -> Field {
        let mut f = Field::new(name, ty).unwrap();
        f.required = required;
        f.options = options;
        f
    }

    fn coll() -> Collection {
        let mut c = Collection::new("posts", CollectionType::Base).unwrap();
        c.fields = vec![
            field("title", FieldType::Text, true, json!({})),
            field("views", FieldType::Number, false, json!({"onlyInt": true})),
            field("score", FieldType::Number, false, json!({})),
            field("mail", FieldType::Email, false, json!({})),
            field("site", FieldType::Url, false, json!({})),
            field(
                "kind",
                FieldType::Select,
                false,
                json!({"values": ["a", "b"]}),
            ),
            field("meta", FieldType::Json, false, json!({})),
            field("ok", FieldType::Bool, false, json!({})),
        ];
        c
    }

    fn text(name: &str, v: Value) -> Result<Option<String>> {
        let c = coll();
        let f = c.fields.iter().find(|f| f.name.as_str() == name).unwrap();
        field_text(f, &v)
    }

    #[test]
    fn values_are_converted_and_checked() {
        assert_eq!(text("title", json!("hi")).unwrap().as_deref(), Some("hi"));
        assert!(
            text("title", json!("")).is_err(),
            "required text must be non-empty"
        );
        assert!(text("title", json!(null)).is_err());
        assert!(text("title", json!(5)).is_err());
        assert_eq!(text("views", json!(7)).unwrap().as_deref(), Some("7"));
        assert_eq!(text("views", json!(7.0)).unwrap().as_deref(), Some("7"));
        assert!(text("views", json!(7.5)).is_err());
        assert!(text("views", json!("7")).is_err());
        assert_eq!(text("score", json!(1.5)).unwrap().as_deref(), Some("1.5"));
        assert_eq!(
            text("mail", json!(" a@b.co ")).unwrap().as_deref(),
            Some("a@b.co")
        );
        assert!(text("mail", json!("nope")).is_err());
        assert_eq!(
            text("mail", json!("")).unwrap(),
            None,
            "empty optional email is null"
        );
        assert!(text("site", json!("https://x.io/p")).is_ok());
        assert!(text("site", json!("ftp://x.io")).is_err());
        assert!(text("site", json!("javascript:alert(1)")).is_err());
        assert!(text("kind", json!("a")).is_ok());
        assert!(text("kind", json!("z")).is_err());
        assert_eq!(
            text("meta", json!({"a": [1, 2]})).unwrap().as_deref(),
            Some(r#"{"a":[1,2]}"#)
        );
        assert_eq!(text("ok", json!(true)).unwrap().as_deref(), Some("true"));
        assert!(text("ok", json!("true")).is_err());
        assert_eq!(text("views", json!(null)).unwrap(), None);
    }

    #[test]
    fn only_writable_known_fields_are_accepted() {
        let c = coll();
        let ok = json!({"title": "t", "views": 1});
        assert_eq!(checked(&c, ok.as_object().unwrap(), true).unwrap().len(), 2);
        for bad in [
            json!({"nope": 1}),
            json!({"created": "x"}),
            json!({"updated": "x"}),
            json!({"password_hash": "x"}),
        ] {
            assert!(
                checked(&c, bad.as_object().unwrap(), true).is_err(),
                "{bad}"
            );
        }
        assert!(
            checked(&c, json!({"id": "abc"}).as_object().unwrap(), true).is_ok(),
            "id allowed on create"
        );
        assert!(
            checked(&c, json!({"id": "abc"}).as_object().unwrap(), false).is_err(),
            "not on update"
        );
        // auth collections: credentials and hidden fields are never writable
        let mut auth = Collection::new("users", CollectionType::Auth).unwrap();
        auth.fields = crate::schema::auth_system_fields();
        let names: Vec<&str> = writable(&auth).iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["email", "emailVisibility"]);
    }

    #[test]
    fn ids_and_placeholders() {
        assert!(
            valid_id("abcde12345fghij")
                && !valid_id("short")
                && !valid_id("ABCDE12345FGHIJ")
                && !valid_id("abcde12345fghi!")
        );
        let (mut marks, mut params) = (vec![], vec![Some("id".to_string())]);
        assert_eq!(
            placeholder(&mut marks, &mut params, Some("7".into()), "bigint"),
            "$2::text::bigint"
        );
        assert_eq!(placeholder(&mut marks, &mut params, None, "bigint"), "NULL");
        assert_eq!(
            placeholder(&mut marks, &mut params, Some("x".into()), "text"),
            "$3::text"
        );
        assert_eq!(params.len(), 3, "NULL takes no parameter");
    }
}
