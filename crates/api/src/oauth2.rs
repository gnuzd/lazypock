//! OAuth2 external auth — PocketBase's `authWithOAuth2`.
//!
//! The flow is the reference one:
//!
//! 1. `GET /api/collections/{collection}/auth-methods` names the configured
//!    providers and hands back an authorization URL with a fresh PKCE challenge
//!    and `state`, plus the `codeVerifier` the client keeps.
//! 2. The client opens that URL in a popup. The provider redirects it back to
//!    `GET /api/oauth2-redirect`, which posts the `code` back to the app origin
//!    the flow started from.
//! 3. The client calls `POST /api/collections/{collection}/auth-with-oauth2` with
//!    `{provider, code, codeVerifier, redirectUrl}`, and the server exchanges the
//!    code, reads the provider's user, links or creates the record, and returns a
//!    token pair.
//!
//! Two decisions worth stating:
//!
//! * **Provider configuration lives in `_settings`** under `oauth2.providers`,
//!   so a superuser can add one from the Studio without a rebuild. `google` and
//!   `github` have presets; any other provider must name its three URLs.
//! * **The pending flow is stored in `_auth_origins`**, not in memory. The
//!   authorize request and the provider callback are two requests that may land
//!   on different nodes, and a per-node store would break the popup bridge on
//!   anything but a single instance.

use base64::Engine as _;
use lazypock_core::error::{Error, Result};
use lazypock_core::id::new_id;
use lazypock_db::deadpool_postgres::Client;
use lazypock_db::pool::db_error;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

/// How long a started flow may take to come back, in seconds.
///
/// A person signing in takes well under this; anything older is an abandoned tab
/// or a replay, and both should not be honoured.
pub const FLOW_TTL_SECONDS: i64 = 10 * 60;

/// Which provider this is, for user-payload normalization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// OpenID Connect userinfo: `sub`, `email`, `name`, `picture`.
    Google,
    /// GitHub's REST user, which may omit the email.
    GitHub,
    /// Anything else: the same fields as Google, with `id` accepted for `sub`.
    Generic,
}

/// A resolved provider.
#[derive(Debug, Clone)]
pub struct Provider {
    /// The name a client sends (and the key in `_external_auths.provider`).
    pub name: String,
    /// The OAuth2 client id.
    pub client_id: String,
    /// The OAuth2 client secret.
    pub client_secret: String,
    /// Where the popup is sent to authorize.
    pub authorize_url: String,
    /// Where the code is exchanged for a token.
    pub token_url: String,
    /// Where the user's profile is read.
    pub user_url: String,
    /// The scopes to request.
    pub scopes: Vec<String>,
    /// Where the provider sends the popup back; must match the provider's app config.
    pub redirect_uri: String,
    /// How to read the user payload.
    pub kind: ProviderKind,
}

/// The built-in URL presets.
struct Preset {
    authorize_url: &'static str,
    token_url: &'static str,
    user_url: &'static str,
    scopes: &'static str,
    kind: ProviderKind,
}

fn preset(name: &str) -> Option<Preset> {
    match name {
        "google" => Some(Preset {
            authorize_url: "https://accounts.google.com/o/oauth2/v2/auth",
            token_url: "https://oauth2.googleapis.com/token",
            user_url: "https://openidconnect.googleapis.com/v1/userinfo",
            scopes: "openid email profile",
            kind: ProviderKind::Google,
        }),
        "github" => Some(Preset {
            authorize_url: "https://github.com/login/oauth/authorize",
            token_url: "https://github.com/login/oauth/access_token",
            user_url: "https://api.github.com/user",
            scopes: "read:user user:email",
            kind: ProviderKind::GitHub,
        }),
        _ => None,
    }
}

/// Parse the `oauth2` setting into resolved providers.
///
/// An incomplete provider is an error rather than one silently missing from the
/// list: a provider an operator configured and the server quietly dropped is the
/// kind of misconfiguration that only shows up as "the button does nothing".
pub fn providers(settings: &Value) -> Result<Vec<Provider>> {
    let Some(list) = settings.get("providers").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };

    let mut out = Vec::with_capacity(list.len());
    for entry in list {
        let name = entry
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .ok_or_else(|| Error::config("an oauth2 provider needs a `name`"))?;
        let config = entry
            .get("config")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        out.push(provider(name, &config)?);
    }
    Ok(out)
}

fn provider(name: &str, config: &Map<String, Value>) -> Result<Provider> {
    let text = |key: &str| {
        config
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };

    let required = |key: &str| {
        text(key).ok_or_else(|| Error::config(format!("oauth2 provider {name:?} needs `{key}`")))
    };

    let client_id = required("client_id")?;
    let client_secret = required("client_secret")?;
    let redirect_uri = required("redirect_uri")?;

    let p = preset(name);
    let url = |key: &str, preset: fn(&Preset) -> &'static str| -> Result<String> {
        text(key)
            .or_else(|| p.as_ref().map(preset).map(str::to_string))
            .ok_or_else(|| {
                Error::config(format!(
                    "oauth2 provider {name:?} needs `{key}`; it is not a built-in preset \
                 (google, github)"
                ))
            })
    };

    let authorize_url = url("authorize_url", |p| p.authorize_url)?;
    let token_url = url("token_url", |p| p.token_url)?;
    let user_url = url("user_url", |p| p.user_url)?;

    let scopes = match config.get("scopes") {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        Some(Value::String(raw)) => raw.split_whitespace().map(str::to_string).collect(),
        _ => p
            .as_ref()
            .map(|p| p.scopes.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default(),
    };

    Ok(Provider {
        name: name.to_string(),
        client_id,
        client_secret,
        authorize_url,
        token_url,
        user_url,
        scopes,
        redirect_uri,
        kind: p.map(|p| p.kind).unwrap_or(ProviderKind::Generic),
    })
}

/// The configured providers, read from the `oauth2` setting.
pub async fn configured(client: &Client) -> Result<Vec<Provider>> {
    let row = client
        .query_opt("select value from _settings where key = 'oauth2'", &[])
        .await
        .map_err(db_error)?;
    let settings: Value = row.map(|row| row.get("value")).unwrap_or(Value::Null);
    providers(&settings)
}

/// A PKCE challenge and the state that ties a callback to the request that started it.
#[derive(Debug, Clone)]
pub struct Pkce {
    /// The secret the client keeps and sends back at exchange time.
    pub verifier: String,
    /// The S256 challenge sent to the provider.
    pub challenge: String,
    /// The opaque value echoed back through the provider.
    pub state: String,
}

impl Pkce {
    /// Generate a fresh verifier, challenge and state.
    pub fn new() -> Result<Self> {
        let verifier = random_urlsafe(48)?;
        let state = random_urlsafe(24)?;
        let digest = Sha256::digest(verifier.as_bytes());
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
        Ok(Self {
            verifier,
            challenge,
            state,
        })
    }
}

/// The URL the popup is sent to, with PKCE and state.
pub fn authorize_url(provider: &Provider, pkce: &Pkce) -> String {
    let mut params: Vec<(&str, String)> = vec![
        ("response_type", "code".to_string()),
        ("client_id", provider.client_id.clone()),
        ("redirect_uri", provider.redirect_uri.clone()),
        ("state", pkce.state.clone()),
        ("code_challenge", pkce.challenge.clone()),
        ("code_challenge_method", "S256".to_string()),
    ];
    if !provider.scopes.is_empty() {
        params.push(("scope", provider.scopes.join(" ")));
    }

    let query = serde_urlencoded::to_string(&params).unwrap_or_default();
    let separator = if provider.authorize_url.contains('?') {
        '&'
    } else {
        '?'
    };
    format!("{}{}{}", provider.authorize_url, separator, query)
}

/// The provider's access token.
pub struct Token {
    /// The bearer token to call the provider's API with.
    pub access_token: String,
}

/// A provider user, normalized to the fields a record can use.
pub struct OAuthUser {
    /// The user's id at the provider; the second half of the link key.
    pub provider_id: String,
    /// The verified email, when the provider gave one.
    pub email: Option<String>,
    /// The display name, when the provider gave one.
    pub name: Option<String>,
    /// An avatar URL, when the provider gave one.
    pub picture: Option<String>,
}

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|err| Error::internal(format!("cannot build an HTTP client: {err}")))
}

/// Exchange the authorization code for an access token.
///
/// The redirect URI is the one the client sends back (PocketBase parity): the
/// provider requires the same value at both steps, and the client is the party
/// that opened the popup. It is validated as an http(s) URL before use.
pub async fn exchange(
    provider: &Provider,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<Token> {
    let redirect_uri = normalize_redirect_uri(redirect_uri)
        .ok_or_else(|| Error::invalid("`redirectUrl` must be an http:// or https:// URL"))?;

    let form = [
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri.as_str()),
        ("client_id", provider.client_id.as_str()),
        ("client_secret", provider.client_secret.as_str()),
        ("code_verifier", verifier),
    ];

    let body = serde_urlencoded::to_string(form).unwrap_or_default();

    let response = http_client()?
        .post(&provider.token_url)
        .header(reqwest::header::ACCEPT, "application/json")
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(body)
        .send()
        .await
        .map_err(|err| {
            Error::unauthorized(format!(
                "could not reach the {} token endpoint: {err}",
                provider.name
            ))
        })?;

    let status = response.status();
    let text = response.text().await.unwrap_or_default();
    let body: Value = serde_json::from_str(&text).unwrap_or(Value::Null);

    if !status.is_success() {
        // The provider's own reason, never the request, which carries the secret.
        let message = body
            .get("error_description")
            .or_else(|| body.get("error"))
            .and_then(Value::as_str)
            .unwrap_or("the provider refused the exchange");
        return Err(Error::unauthorized(format!(
            "the {} token exchange failed: {message}",
            provider.name
        )));
    }

    let access_token = body
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| {
            Error::unauthorized(format!(
                "the {} token response carried no access_token",
                provider.name
            ))
        })?
        .to_string();

    Ok(Token { access_token })
}

/// Read the provider's profile for a token.
pub async fn user(provider: &Provider, token: &Token) -> Result<OAuthUser> {
    let client = http_client()?;
    let payload = fetch_json(&client, &provider.user_url, &token.access_token).await?;

    let provider_id = match provider.kind {
        ProviderKind::GitHub => payload.get("id").and_then(scalar_string),
        _ => field_string(&payload, &["sub", "id"]),
    }
    .ok_or_else(|| {
        Error::unauthorized(format!(
            "the {} profile carried no id, so the account cannot be linked",
            provider.name
        ))
    })?;

    let mut email = field_string(&payload, &["email"]);
    if email.is_none() && provider.kind == ProviderKind::GitHub {
        // GitHub omits the email unless the user made it public; the dedicated
        // endpoint returns it (and which one is primary) with the `user:email` scope.
        email = github_primary_email(&client, token).await;
    }

    Ok(OAuthUser {
        provider_id,
        email,
        name: field_string(&payload, &["name", "login", "preferred_username"]),
        picture: field_string(&payload, &["picture", "avatar_url"]),
    })
}

async fn fetch_json(client: &reqwest::Client, url: &str, token: &str) -> Result<Value> {
    let response = client
        .get(url)
        .bearer_auth(token)
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::USER_AGENT, "lazypock")
        .send()
        .await
        .map_err(|err| {
            Error::unauthorized(format!("could not read the provider profile: {err}"))
        })?;

    if !response.status().is_success() {
        return Err(Error::unauthorized(format!(
            "the provider profile request failed with {}",
            response.status()
        )));
    }

    let text = response.text().await.map_err(|err| {
        Error::unauthorized(format!("could not read the provider profile: {err}"))
    })?;
    serde_json::from_str(&text)
        .map_err(|err| Error::unauthorized(format!("the provider profile was not JSON: {err}")))
}

async fn github_primary_email(client: &reqwest::Client, token: &Token) -> Option<String> {
    let emails = fetch_json(
        client,
        "https://api.github.com/user/emails",
        &token.access_token,
    )
    .await
    .ok()?;
    let list = emails.as_array()?;
    list.iter()
        .find(|entry| {
            entry.get("primary").and_then(Value::as_bool) == Some(true)
                && entry.get("verified").and_then(Value::as_bool) == Some(true)
        })
        .or_else(|| {
            list.iter()
                .find(|entry| entry.get("verified").and_then(Value::as_bool) == Some(true))
        })
        .and_then(|entry| entry.get("email").and_then(Value::as_str))
        .map(str::to_string)
}

/// The record id linked to a provider identity, if any.
pub async fn find_link(
    client: &Client,
    collection: &str,
    provider: &str,
    provider_id: &str,
) -> Result<Option<String>> {
    let row = client
        .query_opt(
            "select user_id from _external_auths \
             where collection = $1 and provider = $2 and provider_id = $3",
            &[
                &collection.to_string(),
                &provider.to_string(),
                &provider_id.to_string(),
            ],
        )
        .await
        .map_err(db_error)?;
    Ok(row.map(|row| row.get("user_id")))
}

/// Link a provider identity to a record, returning the winning record id.
///
/// Idempotent and race-safe: two callbacks for the same identity insert once and
/// both read back the same row, rather than one of them overwriting the link.
pub async fn link(
    client: &Client,
    collection: &str,
    provider: &str,
    provider_id: &str,
    user_id: &str,
) -> Result<String> {
    client
        .execute(
            "insert into _external_auths (id, collection, provider, provider_id, user_id) \
             values ($1, $2, $3, $4, $5) \
             on conflict (collection, provider, provider_id) do nothing",
            &[
                &new_id(),
                &collection.to_string(),
                &provider.to_string(),
                &provider_id.to_string(),
                &user_id.to_string(),
            ],
        )
        .await
        .map_err(db_error)?;

    find_link(client, collection, provider, provider_id)
        .await?
        .ok_or_else(|| Error::internal("the external auth link vanished after it was written"))
}

/// Remember a started flow, keyed by its state.
pub async fn remember_flow(
    client: &Client,
    collection: &str,
    state: &str,
    data: Value,
) -> Result<()> {
    // Sweep first: a bounded, indexed delete per authorize call is cheaper than a
    // worker, and it is the only writer, so the table cannot grow without bound.
    client
        .execute(
            "delete from _auth_origins where created_at < now() - make_interval(secs => $1)",
            &[&(FLOW_TTL_SECONDS as f64)],
        )
        .await
        .map_err(db_error)?;

    client
        .execute(
            "insert into _auth_origins (id, collection_ref, fingerprint, data) \
             values ($1, $2, $3, $4) \
             on conflict (collection_ref, fingerprint) do update \
                 set data = excluded.data, updated_at = now()",
            &[
                &new_id(),
                &collection.to_string(),
                &state.to_string(),
                &data,
            ],
        )
        .await
        .map_err(db_error)?;
    Ok(())
}

/// Consume a flow by its state. Single-use: the row is deleted, so a replayed
/// callback finds nothing.
pub async fn take_flow(client: &Client, state: &str) -> Result<Option<Value>> {
    let row = client
        .query_opt(
            "delete from _auth_origins \
             where fingerprint = $1 and created_at >= now() - make_interval(secs => $2) \
             returning data",
            &[&state.to_string(), &(FLOW_TTL_SECONDS as f64)],
        )
        .await
        .map_err(db_error)?;
    Ok(row.map(|row| row.get("data")))
}

/// Accept only an absolute http(s) URL, with any fragment dropped.
///
/// The reference does the same: the redirect URI is echoed from the client, and
/// a `javascript:`/`data:` scheme must never reach the token request.
pub fn normalize_redirect_uri(url: &str) -> Option<String> {
    let trimmed = url.trim();
    let lower = trimmed.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return None;
    }
    let without_fragment = trimmed.split('#').next().unwrap_or(trimmed);
    let rest = without_fragment
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or("");
    if rest.is_empty() || rest.starts_with('/') {
        return None;
    }
    Some(without_fragment.to_string())
}

fn field_string(payload: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| payload.get(*key).and_then(scalar_string))
}

fn scalar_string(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn random_urlsafe(len: usize) -> Result<String> {
    let mut bytes = vec![0u8; len];
    getrandom::fill(&mut bytes)
        .map_err(|err| Error::internal(format!("cannot generate randomness: {err}")))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn settings(providers: Value) -> Value {
        json!({ "providers": providers })
    }

    #[test]
    fn a_preset_provider_is_complete_from_just_credentials() {
        let list = providers(&settings(json!([{
            "name": "google",
            "config": {
                "client_id": "id",
                "client_secret": "secret",
                "redirect_uri": "https://app.example/api/oauth2-redirect"
            }
        }])))
        .expect("providers");

        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "google");
        assert_eq!(list[0].kind, ProviderKind::Google);
        assert_eq!(list[0].token_url, "https://oauth2.googleapis.com/token");
        assert!(list[0].scopes.contains(&"email".to_string()));
    }

    #[test]
    fn a_generic_provider_must_name_its_urls() {
        let err = providers(&settings(json!([{
            "name": "acme",
            "config": {
                "client_id": "id",
                "client_secret": "secret",
                "redirect_uri": "https://app.example/cb"
            }
        }])))
        .expect_err("refused");
        assert!(err.to_string().contains("authorize_url"), "{err}");

        let list = providers(&settings(json!([{
            "name": "acme",
            "config": {
                "client_id": "id",
                "client_secret": "secret",
                "redirect_uri": "https://app.example/cb",
                "authorize_url": "https://acme.example/authorize",
                "token_url": "https://acme.example/token",
                "user_url": "https://acme.example/user",
                "scopes": ["openid", "email"]
            }
        }])))
        .expect("providers");
        assert_eq!(list[0].kind, ProviderKind::Generic);
        assert_eq!(list[0].scopes, vec!["openid", "email"]);
    }

    #[test]
    fn no_oauth2_setting_means_no_providers() {
        assert!(providers(&Value::Null).expect("none").is_empty());
        assert!(providers(&json!({})).expect("none").is_empty());
        assert!(providers(&json!({ "providers": [] }))
            .expect("none")
            .is_empty());
    }

    #[test]
    fn the_authorization_url_carries_pkce_and_state() {
        let provider = &providers(&settings(json!([{
            "name": "github",
            "config": {
                "client_id": "cid",
                "client_secret": "s",
                "redirect_uri": "https://app.example/api/oauth2-redirect"
            }
        }])))
        .expect("providers")[0];
        let pkce = Pkce::new().expect("pkce");

        let url = authorize_url(provider, &pkce);
        assert!(
            url.starts_with("https://github.com/login/oauth/authorize?"),
            "{url}"
        );
        assert!(url.contains("client_id=cid"), "{url}");
        assert!(url.contains("code_challenge_method=S256"), "{url}");
        assert!(url.contains(&format!("state={}", pkce.state)), "{url}");
        assert!(url.contains("scope=read%3Auser+user%3Aemail"), "{url}");
        // The verifier is a secret and must never appear in the URL.
        assert!(!url.contains(&pkce.verifier), "{url}");
    }

    #[test]
    fn a_redirect_url_must_be_http() {
        assert!(normalize_redirect_uri("https://app.example/cb").is_some());
        assert_eq!(
            normalize_redirect_uri("https://app.example/cb#frag").as_deref(),
            Some("https://app.example/cb")
        );
        assert!(normalize_redirect_uri("javascript:alert(1)").is_none());
        assert!(normalize_redirect_uri("data:text/html,x").is_none());
        assert!(normalize_redirect_uri("file:///etc/passwd").is_none());
        assert!(normalize_redirect_uri("not a url").is_none());
    }
}
