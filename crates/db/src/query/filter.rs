//! PocketBase-compatible filter expressions.
//!
//! This module is a hand-written lexer + precedence-climbing parser producing an
//! AST. It deliberately does **not** build SQL: [`crate::query::compile`] turns
//! the AST into a fully parameterized statement with a whitelisted column set.
//!
//! Filter input is untrusted, so the parser is bounded: at most [`MAX_TOKENS`]
//! tokens and [`MAX_DEPTH`] levels of nesting. Anything the parser does not
//! understand is an error — a filter is never partially applied.

use lazypock_core::error::{Error, Result};

/// Maximum number of tokens in one filter expression.
pub const MAX_TOKENS: usize = 512;
/// Maximum nesting depth of parentheses/logical operators.
pub const MAX_DEPTH: usize = 32;

/// Comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    /// `=`
    Eq,
    /// `!=`
    NotEq,
    /// `>`
    Gt,
    /// `>=`
    Gte,
    /// `<`
    Lt,
    /// `<=`
    Lte,
    /// `~` — case-insensitive contains, PocketBase semantics.
    Like,
    /// `!~` — case-insensitive does-not-contain.
    NotLike,
    /// `?=` — any of the multiple values equals the operand.
    Any,
    /// `?!=` — none of the multiple values equals the operand.
    NotAny,
}

impl CompareOp {
    /// Wire syntax.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::NotEq => "!=",
            Self::Gt => ">",
            Self::Gte => ">=",
            Self::Lt => "<",
            Self::Lte => "<=",
            Self::Like => "~",
            Self::NotLike => "!~",
            Self::Any => "?=",
            Self::NotAny => "?!=",
        }
    }
}

/// A reference to the authenticated request context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthRef {
    /// `@request.auth.id`
    Id,
    /// `@request.auth.email`
    Email,
    /// `@request.auth.collectionId`
    CollectionId,
    /// `@request.auth.collectionName`
    CollectionName,
    /// `@request.auth.<claim>`
    Claim(String),
}

/// The right-hand side of a comparison.
#[derive(Debug, Clone, PartialEq)]
pub enum Operand {
    /// A string literal.
    Text(String),
    /// A numeric literal.
    Number(f64),
    /// A boolean literal.
    Bool(bool),
    /// `null`
    Null,
    /// `@now`
    Now,
    /// `@request.auth.*`
    Auth(AuthRef),
}

/// One side of a comparison.
#[derive(Debug, Clone, PartialEq)]
pub enum Side {
    /// A dotted field path; length 1 is a plain column (`author`, `author.name`).
    Path(Vec<String>),
    /// A literal, `@now`, or `@request.auth.*`.
    Value(Operand),
}

/// A filter expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Both sides must match.
    And(Box<Expr>, Box<Expr>),
    /// Either side may match.
    Or(Box<Expr>, Box<Expr>),
    /// `side op side`.
    ///
    /// Either side may be a field or a value. PocketBase writes its documented rules
    /// with the request reference on the **left** (`@request.auth.id != ""`,
    /// `@request.auth.role = "admin"`), so a comparison that could only hold a field
    /// path there refused half the documented vocabulary — including rules the
    /// format's own examples use. A comparison of two field paths is parsed and then
    /// refused by the compilers, because it needs a column-to-column predicate the
    /// rule and query builders do not build.
    Compare {
        /// Left-hand side.
        left: Side,
        /// Comparison operator.
        op: CompareOp,
        /// Right-hand side.
        right: Side,
    },
}

impl Expr {
    /// Build a `path op operand` comparison, the shape most rules use.
    pub fn compare(path: &[&str], op: CompareOp, operand: Operand) -> Self {
        Expr::Compare {
            left: Side::Path(path.iter().map(|segment| segment.to_string()).collect()),
            op,
            right: Side::Value(operand),
        }
    }
}

/// Parse a filter expression.
pub fn parse(input: &str) -> Result<Expr> {
    match parse_optional(input)? {
        Some(expr) => Ok(expr),
        None => Err(Error::invalid("filter must not be empty")),
    }
}

/// Parse a filter expression, treating blank input as "no filter".
pub fn parse_optional(input: &str) -> Result<Option<Expr>> {
    if input.trim().is_empty() {
        return Ok(None);
    }
    let tokens = Lexer::new(input).tokenize()?;
    if tokens.len() > MAX_TOKENS {
        return Err(Error::invalid(format!(
            "filter is too complex: {} tokens (maximum {MAX_TOKENS})",
            tokens.len()
        )));
    }
    let mut parser = Parser {
        tokens,
        position: 0,
        depth: 0,
    };
    let expr = parser.parse_or()?;
    if let Some(token) = parser.peek() {
        return Err(Error::invalid(format!(
            "unexpected {token} after the end of the filter"
        )));
    }
    Ok(Some(expr))
}

// ---------------------------------------------------------------- tokenizer

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Path(Vec<String>),
    Text(String),
    Number(f64),
    Bool(bool),
    Null,
    Now,
    Auth(AuthRef),
    Op(CompareOp),
    And,
    Or,
    LParen,
    RParen,
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Path(segments) => write!(f, "field `{}`", segments.join(".")),
            Self::Text(value) => write!(f, "string {value:?}"),
            Self::Number(value) => write!(f, "number {value}"),
            Self::Bool(value) => write!(f, "{value}"),
            Self::Null => f.write_str("null"),
            Self::Now => f.write_str("@now"),
            Self::Auth(_) => f.write_str("auth reference"),
            Self::Op(op) => write!(f, "operator `{}`", op.as_str()),
            Self::And => f.write_str("`&&`"),
            Self::Or => f.write_str("`||`"),
            Self::LParen => f.write_str("`(`"),
            Self::RParen => f.write_str("`)`"),
        }
    }
}

struct Lexer<'a> {
    input: &'a str,
    position: usize,
}

impl<'a> Lexer<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, position: 0 }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.position..].chars().next()
    }

    fn peek_second(&self) -> Option<char> {
        let mut chars = self.input[self.position..].chars();
        chars.next();
        chars.next()
    }

    fn bump(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.position += character.len_utf8();
        Some(character)
    }

    fn eat(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.bump();
            true
        } else {
            false
        }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.bump();
        }
    }

    fn error(&self, message: impl std::fmt::Display) -> Error {
        Error::invalid(format!(
            "invalid filter at byte {}: {message}",
            self.position
        ))
    }

    fn tokenize(mut self) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();
        loop {
            self.skip_whitespace();
            let Some(character) = self.peek() else {
                break;
            };
            let token = match character {
                '(' => {
                    self.bump();
                    Token::LParen
                }
                ')' => {
                    self.bump();
                    Token::RParen
                }
                '&' => {
                    self.bump();
                    if !self.eat('&') {
                        return Err(self.error("expected `&&`"));
                    }
                    Token::And
                }
                '|' => {
                    self.bump();
                    if !self.eat('|') {
                        return Err(self.error("expected `||`"));
                    }
                    Token::Or
                }
                '"' | '\'' => Token::Text(self.read_string(character)?),
                '@' => self.read_at_reference()?,
                '~' => {
                    self.bump();
                    Token::Op(CompareOp::Like)
                }
                '=' => {
                    self.bump();
                    Token::Op(CompareOp::Eq)
                }
                '!' => {
                    self.bump();
                    match self.peek() {
                        Some('=') => {
                            self.bump();
                            Token::Op(CompareOp::NotEq)
                        }
                        Some('~') => {
                            self.bump();
                            Token::Op(CompareOp::NotLike)
                        }
                        _ => return Err(self.error("expected `!=` or `!~`")),
                    }
                }
                '>' => {
                    self.bump();
                    if self.eat('=') {
                        Token::Op(CompareOp::Gte)
                    } else {
                        Token::Op(CompareOp::Gt)
                    }
                }
                '<' => {
                    self.bump();
                    if self.eat('=') {
                        Token::Op(CompareOp::Lte)
                    } else {
                        Token::Op(CompareOp::Lt)
                    }
                }
                '?' => {
                    self.bump();
                    match self.peek() {
                        Some('=') => {
                            self.bump();
                            Token::Op(CompareOp::Any)
                        }
                        Some('!') => {
                            self.bump();
                            if !self.eat('=') {
                                return Err(self.error("expected `?!=`"));
                            }
                            Token::Op(CompareOp::NotAny)
                        }
                        _ => return Err(self.error("expected `?=` or `?!=`")),
                    }
                }
                '-' if matches!(self.peek_second(), Some(c) if c.is_ascii_digit()) => {
                    self.bump();
                    let digits = self.read_number_digits();
                    Token::Number(parse_number(&format!("-{digits}"), self.position)?)
                }
                c if c.is_ascii_digit() => Token::Number(self.read_number()?),
                c if c.is_ascii_alphabetic() || c == '_' => self.read_word()?,
                other => return Err(self.error(format!("unexpected character {other:?}"))),
            };
            tokens.push(token);
        }
        Ok(tokens)
    }

    fn read_string(&mut self, quote: char) -> Result<String> {
        let start = self.position;
        self.bump(); // opening quote
        let mut out = String::new();
        loop {
            let Some(character) = self.bump() else {
                return Err(Error::invalid(format!(
                    "invalid filter at byte {start}: unterminated string literal"
                )));
            };
            if character == quote {
                return Ok(out);
            }
            if character == '\\' {
                let Some(escape) = self.bump() else {
                    return Err(self.error("unterminated escape sequence"));
                };
                match escape {
                    '\\' => out.push('\\'),
                    '\'' => out.push('\''),
                    '"' => out.push('"'),
                    'n' => out.push('\n'),
                    'r' => out.push('\r'),
                    't' => out.push('\t'),
                    other => {
                        return Err(self.error(format!(
                            "unsupported escape sequence `\\{other}`; \
                             supported escapes are \\\\ \\' \\\" \\n \\r \\t"
                        )))
                    }
                }
                continue;
            }
            out.push(character);
        }
    }

    fn read_at_reference(&mut self) -> Result<Token> {
        let start = self.position;
        self.bump(); // '@'
        let word = self.read_ident_word();
        let segments: Vec<&str> = word.split('.').filter(|s| !s.is_empty()).collect();

        match segments.as_slice() {
            ["now"] => Ok(Token::Now),
            ["request", "auth", property] => Ok(Token::Auth(match *property {
                "id" => AuthRef::Id,
                "email" => AuthRef::Email,
                "collectionId" => AuthRef::CollectionId,
                "collectionName" => AuthRef::CollectionName,
                claim => AuthRef::Claim(claim.to_string()),
            })),
            ["request", "auth"] => Err(Error::invalid(format!(
                "invalid filter at byte {start}: `@request.auth` needs a property \
                 (id, email, collectionId, collectionName, or a claim name)"
            ))),
            ["request", ..] => Err(Error::invalid(format!(
                "invalid filter at byte {start}: only `@request.auth.*` is supported, got `@{word}`"
            ))),
            _ => Err(Error::invalid(format!(
                "invalid filter at byte {start}: unsupported reference `@{word}`; \
                 supported references are `@now` and `@request.auth.*`"
            ))),
        }
    }

    fn read_number(&mut self) -> Result<f64> {
        let start = self.position;
        let digits = self.read_number_digits();
        parse_number(&digits, start)
    }

    fn read_number_digits(&mut self) -> String {
        let mut out = String::new();
        while let Some(character) = self.peek() {
            if character.is_ascii_digit() || character == '.' {
                out.push(character);
                self.bump();
            } else {
                break;
            }
        }
        out
    }

    /// Read a bare word (no dots); used inside `@` references.
    fn read_ident_word(&mut self) -> String {
        let mut out = String::new();
        while let Some(character) = self.peek() {
            if character.is_ascii_alphanumeric() || character == '_' || character == '.' {
                out.push(character);
                self.bump();
            } else {
                break;
            }
        }
        out
    }

    fn read_word(&mut self) -> Result<Token> {
        let word = self.read_ident_word();
        match word.as_str() {
            "true" => return Ok(Token::Bool(true)),
            "false" => return Ok(Token::Bool(false)),
            "null" => return Ok(Token::Null),
            _ => {}
        }

        let segments: Vec<String> = word.split('.').map(str::to_string).collect();
        for segment in &segments {
            if segment.is_empty() {
                return Err(self.error(format!("malformed field path `{word}`")));
            }
            let mut chars = segment.chars();
            let first = chars.next().expect("non-empty");
            if !(first.is_ascii_alphabetic() || first == '_') {
                return Err(self.error(format!(
                    "malformed field path `{word}`: `{segment}` must start with a letter"
                )));
            }
            if let Some(bad) = chars.find(|c| !(c.is_ascii_alphanumeric() || *c == '_')) {
                return Err(
                    self.error(format!("malformed field path `{word}`: unexpected {bad:?}"))
                );
            }
        }
        Ok(Token::Path(segments))
    }
}

fn parse_number(digits: &str, position: usize) -> Result<f64> {
    digits.parse::<f64>().map_err(|_| {
        Error::invalid(format!(
            "invalid filter at byte {position}: `{digits}` is not a valid number"
        ))
    })
}

// ------------------------------------------------------------------- parser

struct Parser {
    tokens: Vec<Token>,
    position: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn bump(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.position).cloned();
        if token.is_some() {
            self.position += 1;
        }
        token
    }

    fn parse_or(&mut self) -> Result<Expr> {
        self.enter()?;
        let mut left = self.parse_and()?;
        while matches!(self.peek(), Some(Token::Or)) {
            self.bump();
            let right = self.parse_and()?;
            left = Expr::Or(Box::new(left), Box::new(right));
        }
        self.leave();
        Ok(left)
    }

    fn parse_and(&mut self) -> Result<Expr> {
        let mut left = self.parse_comparison()?;
        while matches!(self.peek(), Some(Token::And)) {
            self.bump();
            let right = self.parse_comparison()?;
            left = Expr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr> {
        match self.bump() {
            Some(Token::LParen) => {
                let inner = self.parse_or()?;
                match self.bump() {
                    Some(Token::RParen) => Ok(inner),
                    Some(other) => Err(Error::invalid(format!(
                        "expected `)` but found {other} in the filter"
                    ))),
                    None => Err(Error::invalid("unbalanced `(` in the filter")),
                }
            }
            Some(Token::Path(path)) => self.parse_compare_from(Side::Path(path), None),
            // PocketBase's documented rules put the request reference on the left.
            Some(Token::Auth(reference)) => {
                let description = "the request reference";
                self.parse_compare_from(Side::Value(Operand::Auth(reference)), Some(description))
            }
            Some(Token::Now) => self.parse_compare_from(Side::Value(Operand::Now), Some("`@now`")),
            Some(other) => Err(Error::invalid(format!(
                "expected a field or `(`, found {other} in the filter"
            ))),
            None => Err(Error::invalid(
                "filter ended where a field or `(` was expected",
            )),
        }
    }

    /// Read the operator and the right-hand side for an already-read left side.
    fn parse_compare_from(&mut self, left: Side, left_description: Option<&str>) -> Result<Expr> {
        let Some(Token::Op(op)) = self.bump() else {
            return Err(match (&left, left_description) {
                (Side::Path(path), _) => Error::invalid(format!(
                    "field `{}` must be followed by a comparison operator \
                     (= != > >= < <= ~ !~ ?= ?!=)",
                    path.join(".")
                )),
                (_, Some(description)) => Error::invalid(format!(
                    "{description} must be followed by a comparison operator \
                     (= != > >= < <= ~ !~)"
                )),
                (_, None) => Error::invalid("expected a comparison operator"),
            });
        };
        let right = self.parse_side()?;
        Ok(Expr::Compare { left, op, right })
    }

    /// Read one comparison side that is not the leading field path.
    fn parse_side(&mut self) -> Result<Side> {
        match self.bump() {
            Some(Token::Path(path)) => Ok(Side::Path(path)),
            Some(Token::Text(value)) => Ok(Side::Value(Operand::Text(value))),
            Some(Token::Number(value)) => Ok(Side::Value(Operand::Number(value))),
            Some(Token::Bool(value)) => Ok(Side::Value(Operand::Bool(value))),
            Some(Token::Null) => Ok(Side::Value(Operand::Null)),
            Some(Token::Now) => Ok(Side::Value(Operand::Now)),
            Some(Token::Auth(reference)) => Ok(Side::Value(Operand::Auth(reference))),
            Some(other) => Err(Error::invalid(format!(
                "expected a value after the operator, found {other} in the filter"
            ))),
            None => Err(Error::invalid("filter ended where a value was expected")),
        }
    }

    fn enter(&mut self) -> Result<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(Error::invalid(format!(
                "filter is too complex: more than {MAX_DEPTH} levels of nesting"
            )));
        }
        Ok(())
    }

    fn leave(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compare(path: &[&str], op: CompareOp, operand: Operand) -> Expr {
        Expr::compare(path, op, operand)
    }

    #[test]
    fn parses_a_simple_comparison() {
        assert_eq!(
            parse("title = \"abc\"").unwrap(),
            compare(&["title"], CompareOp::Eq, Operand::Text("abc".into()))
        );
    }

    #[test]
    fn parses_every_operator() {
        let cases = [
            ("=", CompareOp::Eq),
            ("!=", CompareOp::NotEq),
            (">", CompareOp::Gt),
            (">=", CompareOp::Gte),
            ("<", CompareOp::Lt),
            ("<=", CompareOp::Lte),
            ("~", CompareOp::Like),
            ("!~", CompareOp::NotLike),
            ("?=", CompareOp::Any),
            ("?!=", CompareOp::NotAny),
        ];
        for (syntax, op) in cases {
            let expr = parse(&format!("views {syntax} 10")).unwrap();
            assert_eq!(
                expr,
                compare(&["views"], op, Operand::Number(10.0)),
                "operator {syntax}"
            );
        }
    }

    #[test]
    fn and_binds_tighter_than_or() {
        // a || b && c  ==  a || (b && c)
        let expr = parse("a = 1 || b = 2 && c = 3").unwrap();
        let expected = Expr::Or(
            Box::new(compare(&["a"], CompareOp::Eq, Operand::Number(1.0))),
            Box::new(Expr::And(
                Box::new(compare(&["b"], CompareOp::Eq, Operand::Number(2.0))),
                Box::new(compare(&["c"], CompareOp::Eq, Operand::Number(3.0))),
            )),
        );
        assert_eq!(expr, expected);
    }

    #[test]
    fn parentheses_override_precedence() {
        let expr = parse("(a = 1 || b = 2) && c = 3").unwrap();
        let expected = Expr::And(
            Box::new(Expr::Or(
                Box::new(compare(&["a"], CompareOp::Eq, Operand::Number(1.0))),
                Box::new(compare(&["b"], CompareOp::Eq, Operand::Number(2.0))),
            )),
            Box::new(compare(&["c"], CompareOp::Eq, Operand::Number(3.0))),
        );
        assert_eq!(expr, expected);
    }

    #[test]
    fn parses_dotted_paths_and_references() {
        assert_eq!(
            parse("author.name = \"x\"").unwrap(),
            compare(
                &["author", "name"],
                CompareOp::Eq,
                Operand::Text("x".into())
            )
        );
        assert_eq!(
            parse("created > @now").unwrap(),
            compare(&["created"], CompareOp::Gt, Operand::Now)
        );
        assert_eq!(
            parse("author = @request.auth.id").unwrap(),
            compare(&["author"], CompareOp::Eq, Operand::Auth(AuthRef::Id))
        );
        assert_eq!(
            parse("owner = @request.auth.collectionName").unwrap(),
            compare(
                &["owner"],
                CompareOp::Eq,
                Operand::Auth(AuthRef::CollectionName)
            )
        );
        assert_eq!(
            parse("role = @request.auth.realm").unwrap(),
            compare(
                &["role"],
                CompareOp::Eq,
                Operand::Auth(AuthRef::Claim("realm".into()))
            )
        );
    }

    #[test]
    fn the_request_reference_may_be_on_the_left() {
        // PocketBase's documented rules use this shape: `@request.auth.id != ""`,
        // `@request.auth.role = "admin"`.
        assert_eq!(
            parse("@request.auth.id != ''").unwrap(),
            Expr::Compare {
                left: Side::Value(Operand::Auth(AuthRef::Id)),
                op: CompareOp::NotEq,
                right: Side::Value(Operand::Text(String::new())),
            }
        );
        assert_eq!(
            parse("@request.auth.role = 'admin'").unwrap(),
            Expr::Compare {
                left: Side::Value(Operand::Auth(AuthRef::Claim("role".into()))),
                op: CompareOp::Eq,
                right: Side::Value(Operand::Text("admin".into())),
            }
        );
        assert_eq!(
            parse("@request.auth.id != '' && owner = @request.auth.id").unwrap(),
            Expr::And(
                Box::new(Expr::Compare {
                    left: Side::Value(Operand::Auth(AuthRef::Id)),
                    op: CompareOp::NotEq,
                    right: Side::Value(Operand::Text(String::new())),
                }),
                Box::new(compare(
                    &["owner"],
                    CompareOp::Eq,
                    Operand::Auth(AuthRef::Id)
                )),
            )
        );
    }

    #[test]
    fn a_request_reference_may_be_compared_with_a_field() {
        assert_eq!(
            parse("@request.auth.id = author").unwrap(),
            Expr::Compare {
                left: Side::Value(Operand::Auth(AuthRef::Id)),
                op: CompareOp::Eq,
                right: Side::Path(vec!["author".into()]),
            }
        );
        assert_eq!(
            parse("@now > created").unwrap(),
            Expr::Compare {
                left: Side::Value(Operand::Now),
                op: CompareOp::Gt,
                right: Side::Path(vec!["created".into()]),
            }
        );
    }

    #[test]
    fn a_lone_request_reference_is_still_refused() {
        for bad in ["@request.auth.id", "@now", "@request.auth.id ="] {
            assert!(parse(bad).is_err(), "{bad:?} should have been rejected");
        }
    }

    #[test]
    fn parses_literal_kinds_and_escapes() {
        assert_eq!(
            parse("published = true").unwrap(),
            compare(&["published"], CompareOp::Eq, Operand::Bool(true))
        );
        assert_eq!(
            parse("deleted = null").unwrap(),
            compare(&["deleted"], CompareOp::Eq, Operand::Null)
        );
        assert_eq!(
            parse("views = -12.5").unwrap(),
            compare(&["views"], CompareOp::Eq, Operand::Number(-12.5))
        );
        // Both quote styles, and escapes.
        assert_eq!(
            parse("title = 'it\\'s'").unwrap(),
            compare(&["title"], CompareOp::Eq, Operand::Text("it's".into()))
        );
        assert_eq!(
            parse("title = \"a\\nb\"").unwrap(),
            compare(&["title"], CompareOp::Eq, Operand::Text("a\nb".into()))
        );
        assert_eq!(
            parse("title = \"50% \\\\ _\"").unwrap(),
            compare(&["title"], CompareOp::Eq, Operand::Text("50% \\ _".into()))
        );
    }

    #[test]
    fn blank_filters_are_no_filter() {
        assert_eq!(parse_optional("").unwrap(), None);
        assert_eq!(parse_optional("   ").unwrap(), None);
        assert!(parse("").is_err());
    }

    #[test]
    fn rejects_malformed_input() {
        let cases = [
            "",
            "title",
            "title =",
            "= \"abc\"",
            "title = \"abc\" extra",
            "(title = \"abc\"",
            "title = \"abc\")",
            "title = \"abc\" &&",
            "title = \"unterminated",
            "title @ \"abc\"",
            "title = @tomorrow",
            "title = @request",
            "title = @request.auth",
            "title = @request.auth.id.extra",
            "title === 1",
            "title & = 1",
            "title = 1 | 2",
            "1 = title",
            "!title = 1",
            "title = \"a\\qb\"",
            "title = 1.2.3",
            "tit le = 1",
            "title = `x`",
        ];
        for case in cases {
            assert!(parse(case).is_err(), "{case:?} should have been rejected");
        }
    }

    #[test]
    fn rejects_filters_that_are_too_complex() {
        let long = (0..MAX_TOKENS)
            .map(|i| format!("f{i} = {i}"))
            .collect::<Vec<_>>()
            .join(" && ");
        assert!(parse(&long).is_err(), "token limit was not enforced");

        let deep = format!(
            "{}a = 1{}",
            "(".repeat(MAX_DEPTH + 5),
            ")".repeat(MAX_DEPTH + 5)
        );
        let err = parse(&deep).unwrap_err();
        assert!(err.message().contains("too complex"), "{err}");
    }

    #[test]
    fn errors_are_specific_enough_to_act_on() {
        let err = parse("title @ \"x\"").unwrap_err();
        assert!(err.message().contains("unsupported reference"), "{err}");
        let err = parse("title =").unwrap_err();
        assert!(err.message().contains("value"), "{err}");
        let err = parse("title").unwrap_err();
        assert!(err.message().contains("comparison operator"), "{err}");
    }
}
