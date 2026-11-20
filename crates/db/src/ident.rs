//! `Ident`: the only way a user-supplied name reaches SQL.
//!
//! Collection and field names are validated once, here. Same rule as the rest of
//! lazypock: a lowercase first letter, then letters, digits or `_` (so
//! `firstName` works), at most 63 bytes, never `_`-prefixed (system tables).
//! Everything downstream receives an `Ident`, never a raw `&str`, so "forgot to
//! validate" is a type error rather than a bug.

use crate::error::{self, };
use lazypock_core::Error;
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

use lazypock_core::Result;

const MAX_LEN: usize = 63; // Postgres identifier limit

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct Ident(String);

impl Ident {
    pub fn new(s: impl AsRef<str>) -> Result<Self> {
        let s = s.as_ref();
        let bad = |why: &str| error::invalid_ident(format!("{s:?}: {why}"));
        let mut chars = s.chars();
        match chars.next() {
            Some(c) if c.is_ascii_lowercase() => {}
            _ => return Err(bad("must start with a lowercase letter a-z")),
        }
        if s.len() > MAX_LEN {
            return Err(bad("too long (max 63)"));
        }
        if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(bad("only letters, digits and _ are allowed"));
        }
        if s.starts_with("pg_") || s.starts_with("lazypock_") {
            return Err(bad("reserved prefix"));
        }
        Ok(Ident(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// `"name"` — safe because the charset cannot contain `"`.
    pub fn quoted(&self) -> String {
        format!("\"{}\"", self.0)
    }

    pub(crate) fn alias(&self) -> sea_query::Alias {
        sea_query::Alias::new(self.0.clone())
    }
}

impl fmt::Display for Ident {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq<str> for Ident {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl TryFrom<&str> for Ident {
    type Error = Error;
    fn try_from(s: &str) -> Result<Self> {
        Ident::new(s)
    }
}

impl std::str::FromStr for Ident {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        Ident::new(s)
    }
}

impl<'de> Deserialize<'de> for Ident {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Ident::new(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_and_camel_case_names() {
        for ok in ["posts", "a", "user_profiles", "t2", "firstName", "emailVisibility", &"a".repeat(63)] {
            assert!(Ident::new(ok).is_ok(), "{ok}");
        }
    }

    #[test]
    fn rejects_everything_that_could_reach_sql() {
        let long = "a".repeat(64);
        for bad in [
            "", "_x", "1x", "Posts", "a-b", "a b", "a\"b", "a;drop table x", "a'b", "a.b",
            "pg_catalog", "lazypock_api", "é", "a\0b", "a\nb", long.as_str(),
        ] {
            assert!(Ident::new(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn deserialize_validates() {
        assert!(serde_json::from_str::<Ident>("\"posts\"").is_ok());
        assert!(serde_json::from_str::<Ident>("\"x\\\"; --\"").is_err());
    }
}
