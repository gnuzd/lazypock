//! Password handling for records of **auth collections**.
//!
//! The database layer writes whatever columns it is given and knows nothing about
//! passwords. For an auth collection the API calls these first: they take
//! `password` / `passwordConfirm` / `oldPassword` out of the request body, hash the
//! password, and return the credential columns (`password_hash`, a fresh
//! `token_key`) to pass to `records::create` / `records::update` as `extra`.
//!
//! A new `token_key` is what makes a password change cut off access tokens that
//! were already issued (see `AuthService::request_context`).

use crate::{password, sessions};
use lazypock_core::{Error, Result};
use serde_json::{Map, Value};

/// A body with the password fields removed, and the columns to write instead.
#[derive(Debug)]
pub struct Prepared {
    pub body: Value,
    /// `(column, value)`; empty when the password is not being changed.
    pub extra: Vec<(&'static str, String)>,
    pub password_changed: bool,
}

impl Prepared {
    /// Borrowed form for `records::create` / `records::update`.
    pub fn extra(&self) -> Vec<(&str, &str)> {
        self.extra.iter().map(|(c, v)| (*c, v.as_str())).collect()
    }
}

fn bad(msg: &str) -> Error {
    Error::Validation(msg.to_string())
}

fn object(body: &Value) -> Result<Map<String, Value>> {
    body.as_object().cloned().ok_or_else(|| bad("the request body must be a JSON object"))
}

fn take(map: &mut Map<String, Value>, key: &str) -> Result<Option<String>> {
    match map.remove(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(Error::Validation(format!("{key} must be a string"))),
    }
}

async fn credentials(plain: String) -> Result<Vec<(&'static str, String)>> {
    Ok(vec![
        ("password_hash", password::hash_blocking(plain).await?),
        ("token_key", sessions::generate_token()?),
    ])
}

/// New account: `password` and a matching `passwordConfirm` are required.
pub async fn prepare_create(body: &Value) -> Result<Prepared> {
    let mut map = object(body)?;
    let plain = take(&mut map, "password")?.ok_or_else(|| bad("password is required"))?;
    let confirm = take(&mut map, "passwordConfirm")?;
    map.remove("oldPassword");
    if confirm.as_deref() != Some(plain.as_str()) {
        return Err(bad("passwordConfirm does not match password"));
    }
    Ok(Prepared { body: Value::Object(map), extra: credentials(plain).await?, password_changed: true })
}

/// Existing account. Without `password` nothing changes. With it, `passwordConfirm`
/// must match and, when `require_old`, `oldPassword` must verify against
/// `current_hash` (an unknown account looks exactly like a wrong password).
pub async fn prepare_update(current_hash: Option<&str>, body: &Value, require_old: bool) -> Result<Prepared> {
    let mut map = object(body)?;
    let plain = take(&mut map, "password")?;
    let confirm = take(&mut map, "passwordConfirm")?;
    let old = take(&mut map, "oldPassword")?;
    let Some(plain) = plain else {
        if confirm.is_some() || old.is_some() {
            return Err(bad("passwordConfirm and oldPassword can only be sent together with password"));
        }
        return Ok(Prepared { body: Value::Object(map), extra: Vec::new(), password_changed: false });
    };
    if confirm.as_deref() != Some(plain.as_str()) {
        return Err(bad("passwordConfirm does not match password"));
    }
    if require_old {
        let old = old.ok_or_else(|| bad("oldPassword is required to change the password"))?;
        let stored = current_hash.unwrap_or_default().to_string();
        // Verify even when there is nothing to verify against, so timing does not tell.
        let stored = if stored.is_empty() { crate::service::dummy_hash_for_record().to_string() } else { stored };
        if !password::verify_blocking(old, stored).await || current_hash.is_none_or(str::is_empty) {
            return Err(bad("oldPassword is not correct"));
        }
    }
    Ok(Prepared { body: Value::Object(map), extra: credentials(plain).await?, password_changed: true })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn create_strips_password_fields_and_returns_credentials() {
        let p = prepare_create(&json!({"email": "a@b.co", "name": "A", "password": "long enough", "passwordConfirm": "long enough", "oldPassword": "x"})).await.unwrap();
        assert_eq!(p.body, json!({"email": "a@b.co", "name": "A"}));
        let cols: Vec<&str> = p.extra.iter().map(|(c, _)| *c).collect();
        assert_eq!(cols, ["password_hash", "token_key"]);
        assert!(p.extra[0].1.starts_with("$argon2id$") && p.extra[1].1.len() == 43);
        assert!(p.password_changed);
    }

    #[tokio::test]
    async fn create_needs_a_matching_confirmed_password() {
        for body in [
            json!({"email": "a@b.co"}),
            json!({"password": "long enough"}),
            json!({"password": "long enough", "passwordConfirm": "different one"}),
            json!({"password": "short", "passwordConfirm": "short"}),
            json!({"password": 12345678, "passwordConfirm": 12345678}),
            json!([1, 2]),
        ] {
            assert!(matches!(prepare_create(&body).await, Err(Error::Validation(_))), "{body}");
        }
    }

    #[tokio::test]
    async fn update_without_password_changes_nothing() {
        let p = prepare_update(None, &json!({"name": "B"}), true).await.unwrap();
        assert_eq!((p.body, p.extra.len(), p.password_changed), (json!({"name": "B"}), 0, false));
        assert!(prepare_update(None, &json!({"passwordConfirm": "x"}), true).await.is_err());
        assert!(prepare_update(None, &json!({"oldPassword": "x"}), true).await.is_err());
    }

    #[tokio::test]
    async fn changing_a_password_checks_the_old_one() {
        let stored = password::hash("old password").unwrap();
        let body = |old: Option<&str>| {
            let mut b = json!({"password": "new password", "passwordConfirm": "new password"});
            if let Some(o) = old {
                b["oldPassword"] = json!(o);
            }
            b
        };
        let ok = prepare_update(Some(&stored), &body(Some("old password")), true).await.unwrap();
        assert!(ok.password_changed && ok.extra.len() == 2);
        for (hash, old) in [(Some(stored.as_str()), Some("wrong one")), (Some(stored.as_str()), None), (None, Some("old password")), (Some(""), Some("old password"))] {
            assert!(matches!(prepare_update(hash, &body(old), true).await, Err(Error::Validation(_))), "{hash:?} {old:?}");
        }
        // a superuser does not need the old password
        assert!(prepare_update(None, &body(None), false).await.unwrap().password_changed);
    }
}
