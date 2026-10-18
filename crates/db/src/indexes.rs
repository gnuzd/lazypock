//! Custom collection indexes: the `options.indexes` list.
//!
//! The documented import format, the published JSON Schema and the Studio's index
//! editor all promise this list, and PocketBase exports carry it as SQL statements.
//! Nothing read it: a collection was created, the list was stored in `_collections`
//! and returned to clients, and no index ever reached Postgres. This module turns a
//! declaration into DDL.
//!
//! Two spellings are accepted, and nothing else:
//!
//! ```text
//! UNIQUE email
//! created_at DESC, id
//! CREATE INDEX idx_posts_created ON posts (created_at DESC)
//! CREATE UNIQUE INDEX idx_posts_slug ON posts (slug) WHERE published = true
//! ```
//!
//! The short form is what the Studio emits and what the docs describe; the statement
//! form is PocketBase's, which an export arrives in. Both are parsed into the same
//! [`IndexSpec`] here, and the statement is **rebuilt** from the parsed parts rather
//! than executed as given: an import file is untrusted input, and "run this SQL for
//! me" is not a feature a backup format gets. Index *expressions*, `USING`, `COLLATE`
//! and `INCLUDE` are refused with a message saying so, never silently skipped.

use std::collections::HashSet;

use lazypock_core::error::{Error, Result};

use crate::collections::Collection;
use crate::ddl::{index_name, quote_ident};
use crate::query::compile::FilterContext;
use crate::query::filter;
use crate::rls::policy;
use crate::types::{validate_identifier, MAX_IDENTIFIER_LEN};

/// One column of an index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexColumn {
    /// Column name, validated and kept verbatim.
    pub name: String,
    /// `DESC` when the index is ordered descending.
    pub descending: bool,
}

/// A resolved index declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexSpec {
    /// Index name Postgres will hold.
    pub name: String,
    /// Whether the index enforces uniqueness.
    pub unique: bool,
    /// Indexed columns, in order.
    pub columns: Vec<IndexColumn>,
    /// Inlined `WHERE` predicate for a partial index, already compiled to SQL.
    pub predicate: Option<String>,
}

impl IndexSpec {
    /// `create index ...` for this declaration.
    ///
    /// `if not exists` is deliberately included: the caller drops what it is
    /// replacing first, so the only case it catches is a name that already belongs to
    /// another index (a built-in one, or one created outside lazypock). The caller
    /// reports that rather than treating it as a failure, because the alternative —
    /// replacing an index lazypock relies on (the case-insensitive unique email
    /// index) — would weaken a guarantee to satisfy a declaration.
    pub fn create_statement(&self, table: &str) -> String {
        let columns = self
            .columns
            .iter()
            .map(|column| {
                if column.descending {
                    format!("{} desc", quote_ident(&column.name))
                } else {
                    quote_ident(&column.name)
                }
            })
            .collect::<Vec<_>>()
            .join(", ");

        let unique = if self.unique { "unique " } else { "" };
        let predicate = self
            .predicate
            .as_ref()
            .map(|predicate| format!(" where {predicate}"))
            .unwrap_or_default();

        format!(
            "create {unique}index if not exists {} on {} ({columns}){predicate}",
            quote_ident(&self.name),
            quote_ident(table)
        )
    }

    /// `drop index ...` for this declaration.
    pub fn drop_statement(&self) -> String {
        format!("drop index if exists {}", quote_ident(&self.name))
    }

    /// What the index actually indexes, as a stable string.
    ///
    /// Used to derive a name that cannot collide with one the collection already
    /// holds: the same declaration always produces the same suffix, so a re-import
    /// resolves to the same index instead of piling up new ones.
    fn signature_hash(&self) -> String {
        let columns = self
            .columns
            .iter()
            .map(|column| {
                format!(
                    "{}{}",
                    column.name,
                    if column.descending { " desc" } else { "" }
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        sha(&format!(
            "{}|{}|{}",
            self.unique,
            columns,
            self.predicate.as_deref().unwrap_or_default()
        ))
    }
}

/// Resolve a whole `indexes` list against a collection.
///
/// Column names are checked against the collection's own columns ([`FilterContext`]),
/// and a `WHERE` predicate is compiled with the rule compiler, so a partial index
/// can only reference the same fields a rule could. Every entry is validated before
/// anything is created, so a bad third index cannot leave the first two behind.
pub fn parse_all(
    collection: &Collection,
    declared: &[String],
    context: &FilterContext,
) -> Result<Vec<IndexSpec>> {
    let table = collection.name.as_str();
    let mut specs: Vec<IndexSpec> = Vec::with_capacity(declared.len());
    let mut taken = reserved_names(collection);

    for raw in declared {
        let mut spec = parse_one(table, raw, context)?;

        if spec.name.is_empty() {
            spec.name = generated_name(table, &spec.columns);
        }

        // A name the collection already holds (a unique field's index, the auth email
        // index) must not be reused: `create index if not exists` would then find it
        // and the declaration would never be applied. The disambiguated name is
        // derived from the declaration, so re-importing the same file resolves to the
        // same name instead of creating a new index each time.
        if taken.contains(&spec.name) {
            spec.name = disambiguated_name(&spec.name, &spec.signature_hash());
        }

        if specs.iter().any(|existing| existing.name == spec.name) {
            return Err(Error::invalid(format!(
                "the index name {:?} is declared more than once for collection {table:?}",
                spec.name
            )));
        }

        taken.insert(spec.name.clone());
        specs.push(spec);
    }

    Ok(specs)
}

/// Index names a collection already has from its own definition.
///
/// These are what a generated name must not collide with: a unique field's index, the
/// plain index a field's `indexed` flag asks for, and the auth collection's
/// case-insensitive email index all use `idx_<table>_<column>`.
fn reserved_names(collection: &Collection) -> HashSet<String> {
    let table = collection.name.as_str();
    let mut names: HashSet<String> = HashSet::new();
    for field in &collection.fields {
        if field.is_unique || field.is_indexed() {
            names.insert(index_name(table, &field.name));
        }
    }
    if collection.collection_type.is_auth() {
        names.insert(index_name(table, "email"));
    }
    names
}

fn generated_name(table: &str, columns: &[IndexColumn]) -> String {
    let suffix = columns
        .iter()
        .map(|column| column.name.as_str())
        .collect::<Vec<_>>()
        .join("_");
    let base = format!("idx_{table}_{suffix}");
    if base.len() <= MAX_IDENTIFIER_LEN {
        return base;
    }
    // Reuse the column-name helper's truncation: same limit, same deterministic hash.
    let mut name = index_name(table, &suffix);
    if name.len() > MAX_IDENTIFIER_LEN {
        name.truncate(MAX_IDENTIFIER_LEN);
    }
    name
}

fn disambiguated_name(base: &str, hash: &str) -> String {
    let suffix = format!("_{hash}");
    let keep = MAX_IDENTIFIER_LEN.saturating_sub(suffix.len());
    format!("{}{suffix}", &base[..base.len().min(keep)])
}

fn sha(value: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(value.as_bytes())[..4]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Parse one declaration.
fn parse_one(table: &str, raw: &str, context: &FilterContext) -> Result<IndexSpec> {
    let tokens = tokenize(raw)?;
    if tokens.is_empty() {
        return Err(Error::invalid("an index declaration must not be empty"));
    }

    let statement = tokens.len() >= 3
        && tokens[0].upper() == "CREATE"
        && (tokens[1].upper() == "UNIQUE" || tokens[1].upper() == "INDEX");

    if statement {
        parse_statement(table, raw, &tokens, context)
    } else {
        parse_expression(table, &tokens, context)
    }
}

/// `[UNIQUE] column [ASC|DESC][, ...]`
fn parse_expression(table: &str, tokens: &[Token], context: &FilterContext) -> Result<IndexSpec> {
    let mut index = 0;
    let mut unique = false;
    if tokens[index].upper() == "UNIQUE" {
        unique = true;
        index += 1;
    }

    let mut columns = Vec::new();
    loop {
        let column = tokens.get(index).ok_or_else(|| {
            Error::invalid(format!(
                "the index {:?} has no column; write e.g. \"UNIQUE email\" or \
                 \"created_at DESC, id\"",
                render(tokens)
            ))
        })?;
        if column.kind != TokenKind::Identifier {
            return Err(unexpected(table, column));
        }
        index += 1;

        let mut descending = false;
        if let Some(direction) = tokens.get(index) {
            match direction.upper().as_str() {
                "ASC" => index += 1,
                "DESC" => {
                    descending = true;
                    index += 1;
                }
                _ => {}
            }
        }

        columns.push(IndexColumn {
            name: validate_column(table, &column.text, context)?,
            descending,
        });

        match tokens.get(index) {
            None => break,
            Some(token) if token.text == "," => index += 1,
            Some(token) => {
                return Err(Error::invalid(format!(
                    "unexpected {:?} in the index {:?}; the supported forms are an expression \
                     list (`UNIQUE email`, `created_at DESC, id`) or a full \
                     `CREATE INDEX ... ON <collection> (...)` statement",
                    token.text,
                    render(tokens)
                )))
            }
        }
    }

    Ok(IndexSpec {
        name: String::new(),
        unique,
        columns,
        predicate: None,
    })
}

/// `CREATE [UNIQUE] INDEX [IF NOT EXISTS] name ON table (column [ASC|DESC], ...) [WHERE ...]`
fn parse_statement(
    table: &str,
    raw: &str,
    tokens: &[Token],
    context: &FilterContext,
) -> Result<IndexSpec> {
    let mut index = 0;
    let mut unique = false;

    expect_keyword(raw, tokens, &mut index, "CREATE")?;
    if tokens.get(index).map(Token::upper).as_deref() == Some("UNIQUE") {
        unique = true;
        index += 1;
    }
    expect_keyword(raw, tokens, &mut index, "INDEX")?;
    if tokens.get(index).map(Token::upper).as_deref() == Some("IF") {
        index += 1;
        expect_keyword(raw, tokens, &mut index, "NOT")?;
        expect_keyword(raw, tokens, &mut index, "EXISTS")?;
    }

    let name = tokens.get(index).ok_or_else(|| {
        Error::invalid("a CREATE INDEX statement needs an index name".to_string())
    })?;
    if name.kind != TokenKind::Identifier {
        return Err(unexpected(table, name));
    }
    let name = name.text.clone();
    validate_identifier("index name", &name)?;
    index += 1;

    expect_keyword(raw, tokens, &mut index, "ON")?;

    let target = tokens
        .get(index)
        .ok_or_else(|| Error::invalid("a CREATE INDEX statement needs a table name".to_string()))?;
    if target.kind != TokenKind::Identifier || !target.text.eq_ignore_ascii_case(table) {
        return Err(Error::invalid(format!(
            "the index {name:?} is declared on {:?} but the collection is {table:?}",
            target.text
        )));
    }
    index += 1;

    let open = tokens.get(index).ok_or_else(|| {
        Error::invalid(format!(
            "the index {name:?} needs a column list in parentheses"
        ))
    })?;
    if open.text != "(" {
        return Err(Error::invalid(format!(
            "unexpected {:?} after the table name in the index {name:?}; expected `(`",
            open.text
        )));
    }
    index += 1;

    let mut columns = Vec::new();
    loop {
        let column = tokens.get(index).ok_or_else(|| {
            Error::invalid(format!(
                "the index {name:?} has an unterminated column list"
            ))
        })?;
        if column.text == "(" {
            return Err(Error::unsupported(format!(
                "the index {name:?} uses an expression; only plain columns are supported. \
                 Drop it from the file, or declare the columns it is built from"
            )));
        }
        if column.kind != TokenKind::Identifier {
            return Err(unexpected(table, column));
        }
        index += 1;

        let mut descending = false;
        if let Some(direction) = tokens.get(index) {
            match direction.upper().as_str() {
                "ASC" => index += 1,
                "DESC" => {
                    descending = true;
                    index += 1;
                }
                "COLLATE" | "NULLS" | "USING" | "INCLUDE" => {
                    return Err(Error::unsupported(format!(
                        "the index {name:?} uses `{}`, which is not supported; remove it from \
                         the declaration",
                        direction.upper()
                    )))
                }
                _ => {}
            }
        }

        columns.push(IndexColumn {
            name: validate_column(table, &column.text, context)?,
            descending,
        });

        match tokens.get(index) {
            Some(token) if token.text == "," => index += 1,
            Some(token) if token.text == ")" => {
                index += 1;
                break;
            }
            Some(token) => return Err(unexpected(table, token)),
            None => {
                return Err(Error::invalid(format!(
                    "the index {name:?} has an unterminated column list"
                )))
            }
        }
    }

    let mut predicate = None;
    if let Some(token) = tokens.get(index) {
        if token.upper() != "WHERE" {
            return Err(Error::unsupported(format!(
                "unexpected {:?} in the index {name:?}; only a plain column list and an optional \
                 WHERE predicate are supported",
                token.text
            )));
        }
        // The rest of the raw declaration, from the predicate's first character, is a
        // filter expression: it is compiled to inline SQL exactly as a collection rule
        // is, so it can reference the collection's own columns and nothing else.
        let rest = raw[token.end..].replace('`', "");
        let expression = filter::parse(rest.trim())?;
        predicate = Some(policy::predicate(&expression, context)?);
    }

    Ok(IndexSpec {
        name,
        unique,
        columns,
        predicate,
    })
}

fn expect_keyword(raw: &str, tokens: &[Token], index: &mut usize, keyword: &str) -> Result<()> {
    let token = tokens.get(*index).ok_or_else(|| {
        Error::invalid(format!(
            "the index declaration {raw:?} ended where `{keyword}` was expected"
        ))
    })?;
    if token.upper() != keyword {
        return Err(Error::invalid(format!(
            "expected `{keyword}` in the index declaration {raw:?}, found {:?}",
            token.text
        )));
    }
    *index += 1;
    Ok(())
}

fn validate_column(table: &str, name: &str, context: &FilterContext) -> Result<String> {
    if !context.columns.contains_key(name) {
        return Err(Error::invalid(format!(
            "the index on collection {table:?} names the unknown column {name:?}; it may index \
             one of that collection's own columns"
        )));
    }
    Ok(name.to_string())
}

fn unexpected(table: &str, token: &Token) -> Error {
    Error::invalid(format!(
        "unexpected {:?} in an index on collection {table:?}; write a column name, a direction \
         (ASC/DESC), a comma or a closing parenthesis",
        token.text
    ))
}

fn render(tokens: &[Token]) -> String {
    tokens
        .iter()
        .map(|token| token.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

// ----------------------------------------------------------------- tokenizer

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenKind {
    /// A bare or quoted identifier, or a keyword.
    Identifier,
    /// `(`, `)` or `,`.
    Punctuation,
}

#[derive(Debug, Clone)]
struct Token {
    text: String,
    kind: TokenKind,
    /// Byte offset one past the token's last character. Kept so a `WHERE` predicate
    /// can be sliced out of the raw declaration and handed to the filter parser.
    end: usize,
}

impl Token {
    fn upper(&self) -> String {
        self.text.to_ascii_uppercase()
    }
}

/// The character at `position`, or `None` at (or past) the end of the input.
///
/// Byte-indexed rather than iterated, because the tokenizer also needs the byte
/// offsets — a `WHERE` predicate is sliced out of the raw declaration and needs a
/// position, not just the characters.
fn char_at(raw: &str, position: usize) -> Option<char> {
    raw.get(position..).and_then(|rest| rest.chars().next())
}

/// Split a declaration into identifiers, quoted identifiers and punctuation.
///
/// Quoting is accepted in all three spellings an export can arrive in — `"email"`,
/// `` `email` `` and `[email]` — and the quotes are removed, so the column or index
/// name is validated here rather than by Postgres.
fn tokenize(raw: &str) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut position = 0;

    while let Some(character) = char_at(raw, position) {
        if character.is_whitespace() {
            position += character.len_utf8();
            continue;
        }

        if matches!(character, '(' | ')' | ',') {
            tokens.push(Token {
                text: character.to_string(),
                kind: TokenKind::Punctuation,
                end: position + 1,
            });
            position += 1;
            continue;
        }

        if matches!(character, '"' | '`' | '[') {
            let close = match character {
                '"' => '"',
                '`' => '`',
                _ => ']',
            };
            position += 1;
            let mut text = String::new();
            let mut closed = false;
            while let Some(next) = char_at(raw, position) {
                position += next.len_utf8();
                if next == close {
                    closed = true;
                    break;
                }
                text.push(next);
            }
            if !closed {
                return Err(Error::invalid(format!(
                    "the index declaration {raw:?} has an unterminated quoted identifier"
                )));
            }
            tokens.push(Token {
                text,
                kind: TokenKind::Identifier,
                end: position,
            });
            continue;
        }

        if character.is_ascii_alphanumeric() || character == '_' || character == '$' {
            let start = position;
            while let Some(next) = char_at(raw, position) {
                if next.is_ascii_alphanumeric() || next == '_' || next == '$' {
                    position += next.len_utf8();
                } else {
                    break;
                }
            }
            let word = raw[start..position].to_string();
            let is_where = word.eq_ignore_ascii_case("where");
            tokens.push(Token {
                text: word,
                kind: TokenKind::Identifier,
                end: position,
            });
            if is_where {
                // Everything after `WHERE` is a filter expression, not index syntax, so
                // it is left as raw text for the filter parser. Tokenising it here would
                // reject the operators a filter is made of.
                break;
            }
            continue;
        }

        return Err(Error::unsupported(format!(
            "the index declaration {raw:?} contains {character:?}, which is not supported; only \
             plain columns and an optional WHERE predicate can be declared"
        )));
    }

    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::{Collection, Field};
    use crate::types::{CollectionType, FieldType};
    use serde_json::json;
    use std::time::SystemTime;

    fn field(name: &str, field_type: FieldType) -> Field {
        Field {
            id: format!("f_{name}"),
            collection_id: "c1".into(),
            name: name.into(),
            field_type,
            required: false,
            is_unique: false,
            options: json!({}),
            position: 0,
        }
    }

    fn collection() -> Collection {
        Collection {
            id: "c1".into(),
            name: "posts".into(),
            collection_type: CollectionType::Base,
            options: json!({}),
            rules: json!({}),
            fields: vec![
                field("title", FieldType::Text),
                field("slug", FieldType::Text),
                field("created_at", FieldType::Date),
                field("published", FieldType::Bool),
            ],
            created: SystemTime::now(),
            updated: SystemTime::now(),
        }
    }

    fn parse(raw: &[&str]) -> Result<Vec<IndexSpec>> {
        let posts = collection();
        let context = FilterContext::for_collection(&posts, std::slice::from_ref(&posts)).unwrap();
        let declared: Vec<String> = raw.iter().map(|entry| entry.to_string()).collect();
        parse_all(&posts, &declared, &context)
    }

    #[test]
    fn the_short_form_generates_a_stable_name() {
        let specs = parse(&["UNIQUE slug", "created_at DESC, id"]).expect("parse");
        assert_eq!(specs[0].name, "idx_posts_slug");
        assert!(specs[0].unique);
        assert_eq!(specs[1].name, "idx_posts_created_at_id");
        assert!(!specs[1].unique);
        assert_eq!(specs[1].columns[0].name, "created_at");
        assert!(specs[1].columns[0].descending);
    }

    #[test]
    fn a_statement_is_rebuilt_not_executed() {
        let specs = parse(&["CREATE UNIQUE INDEX idx_posts_slug ON posts (slug)"]).expect("parse");
        assert_eq!(specs[0].name, "idx_posts_slug");
        assert_eq!(
            specs[0].create_statement("posts"),
            "create unique index if not exists \"idx_posts_slug\" on \"posts\" (\"slug\")"
        );
    }

    #[test]
    fn pocketbase_quoting_and_a_partial_predicate_are_understood() {
        let specs = parse(&[
            "CREATE UNIQUE INDEX `idx_posts_slug` ON `posts` (`slug`) WHERE `published` = true",
        ])
        .expect("parse");
        assert_eq!(
            specs[0].create_statement("posts"),
            "create unique index if not exists \"idx_posts_slug\" on \"posts\" (\"slug\") \
             where \"published\" = true::boolean"
        );
    }

    #[test]
    fn a_generated_name_never_shadows_a_built_in_index() {
        // A unique field already owns `idx_posts_slug`; a generated name must not
        // reuse it, because `create index if not exists` would then leave the
        // declaration unapplied.
        let mut posts = collection();
        posts.fields[1].is_unique = true;
        let context = FilterContext::for_collection(&posts, std::slice::from_ref(&posts)).unwrap();
        let specs = parse_all(&posts, &["slug".to_string()], &context).expect("parse");
        assert!(
            specs[0].name.starts_with("idx_posts_slug_") && specs[0].name != "idx_posts_slug",
            "{}",
            specs[0].name
        );

        // The same declaration resolves to the same name, so a re-import is idempotent.
        let again = parse_all(&posts, &["slug".to_string()], &context).expect("parse");
        assert_eq!(specs[0].name, again[0].name);
    }

    #[test]
    fn unknown_columns_table_mismatches_and_expressions_are_refused() {
        for bad in [
            "nope",
            "CREATE INDEX idx_x ON other (slug)",
            "CREATE INDEX idx_x ON posts (lower(slug))",
            "CREATE INDEX idx_x ON posts (slug) USING btree",
            "CREATE INDEX idx_x ON posts (slug) WHERE nope = 1",
            "slug WHERE",
        ] {
            let err = parse(&[bad]).unwrap_err();
            assert!(
                err.message().contains("unknown column")
                    || err.message().contains("unknown field")
                    || err.message().contains("but the collection is")
                    || err.message().contains("expression")
                    || err.message().contains("supported")
                    || err.message().contains("unexpected")
                    || err.message().contains("needs"),
                "{bad:?} produced {err}"
            );
        }
    }

    #[test]
    fn an_injection_attempt_stays_a_validation_error() {
        let err = parse(&["CREATE INDEX x ON posts (slug); drop table posts"]).unwrap_err();
        assert!(err.message().contains("supported") || err.message().contains("unexpected"));
    }
}
