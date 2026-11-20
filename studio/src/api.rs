//! The API client and the signed-in session.
//!
//! A release build is served by the API itself (`/_/`), so its default base URL is
//! empty and requests resolve against the page's origin. A dev build talks to the API
//! on its own port. Settings can point either build at another instance.
//!
//! The session lives in `Storage`, so it survives a launch. The access token is short
//! lived: a refusal on launch is refreshed once, and a refused refresh signs out.

use nsh_core::JsonValue;
use nsh_core::platform::HttpRequest;
use nsh_core::prelude::*;
use nsh_core::storage::Storage;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::json;

use crate::components::toast;
use crate::nav::{self, Route};

const SESSION_KEY: &str = "auth.session";
const BASE_URL_KEY: &str = "api.url";
const DEV_API: &str = "http://127.0.0.1:8000";
const SUPERUSERS: &str = "/api/collections/_superusers";

/// A request that did not happen, or one the backend refused.
enum Refused {
    Unreachable,
    Status { status: u16, reply: JsonValue },
}

/// Where requests go: the Settings override, else the dev API, else this origin.
pub fn base_url() -> String {
    let stored = Storage::get_item(BASE_URL_KEY).unwrap_or_default();
    let stored = stored.trim().trim_end_matches('/');
    if !stored.is_empty() {
        return stored.to_owned();
    }
    if cfg!(debug_assertions) {
        DEV_API.to_owned()
    } else {
        String::new()
    }
}

pub fn save_base_url(url: &str) {
    let url = url.trim().trim_end_matches('/');
    if url.is_empty() {
        Storage::remove_item(BASE_URL_KEY);
    } else {
        let _ = Storage::set_item(BASE_URL_KEY, url);
    }
}

struct Session {
    token: String,
    refresh_token: String,
}

fn session() -> Option<Session> {
    let data = Storage::get_item(SESSION_KEY)?;
    let value: JsonValue = serde_json::from_str(&data).ok()?;
    Some(Session {
        token: value.get("token")?.as_str()?.to_owned(),
        refresh_token: value.get("refreshToken")?.as_str()?.to_owned(),
    })
}

pub fn signed_in() -> bool {
    session().is_some()
}

/// A collection as the backend describes it.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Collection {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub fields: Vec<Field>,
}

/// One field of a collection's schema.
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Field {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
}

/// A page of records: each item is the JSON object the backend stored.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Records {
    pub total_items: i64,
    pub items: Vec<JsonValue>,
}

/// The signed-in account's e-mail, for the settings screen.
pub fn signed_in_email() -> Option<String> {
    let data = Storage::get_item(SESSION_KEY)?;
    let value: JsonValue = serde_json::from_str(&data).ok()?;
    value
        .get("record")?
        .get("email")?
        .as_str()
        .map(str::to_owned)
}

fn store_session(payload: &JsonValue) {
    let _ = Storage::set_item(SESSION_KEY, payload.to_string());
}

pub async fn healthcheck() -> Option<JsonValue> {
    match send("GET", "/api/health", None, Some("application/json")).await {
        Ok((200, reply)) => parse(reply, "load the health"),
        Ok((status, reply)) => {
            failed("load the health", Refused::Status { status, reply });
            None
        }
        Err(()) => {
            failed("load the health", Refused::Unreachable);
            None
        }
    }
}

/// Signs in with `identity` (the e-mail) and `password`.
pub async fn sign_in(identity: &str, password: &str) -> bool {
    let body = json!({ "identity": identity, "password": password }).to_string();
    match send(
        "POST",
        &format!("{SUPERUSERS}/auth-with-password"),
        Some(body),
        None,
    )
    .await
    {
        Ok((200, reply)) => {
            store_session(&reply);
            true
        }
        Ok((status, reply)) => failed("sign in", Refused::Status { status, reply }),
        Err(()) => failed("sign in", Refused::Unreachable),
    }
}

/// Whether the instance still has no superuser, so setup is open; a failure is
/// reported and leaves setup open, where the form reports the real problem.
pub async fn setup_required() -> bool {
    match send("GET", "/api/setup", None, None).await {
        Ok((200, reply)) => reply
            .get("required")
            .and_then(JsonValue::as_bool)
            .unwrap_or(false),
        Ok((status, reply)) => failed("check the setup state", Refused::Status { status, reply }),
        Err(()) => failed("check the setup state", Refused::Unreachable),
    }
}

/// Creates the first superuser; the reply signs them in.
pub async fn create_superuser(email: &str, password: &str) -> bool {
    let body = json!({ "email": email, "password": password }).to_string();
    match send("POST", "/api/setup", Some(body), None).await {
        Ok((200, reply)) => {
            store_session(&reply);
            true
        }
        Ok((status, reply)) => failed("create the superuser", Refused::Status { status, reply }),
        Err(()) => failed("create the superuser", Refused::Unreachable),
    }
}

/// Creates a collection; `ty` is `"base"` or `"auth"`. Fields are added later.
pub async fn create_collection(name: &str, ty: &str) -> Option<Collection> {
    let token = token("create a collection")?;
    let body = json!({ "name": name, "type": ty }).to_string();
    match send("POST", "/api/collections", Some(body), Some(&token)).await {
        Ok((201, reply)) => parse(reply, "create the collection"),
        Ok((status, reply)) => {
            failed("create the collection", Refused::Status { status, reply });
            None
        }
        Err(()) => {
            failed("create the collection", Refused::Unreachable);
            None
        }
    }
}

/// Every collection of the instance, by name.
pub async fn fetch_collections() -> Option<Vec<Collection>> {
    let token = token("load the collections")?;
    match send("GET", "/api/collections", None, Some(&token)).await {
        Ok((200, reply)) => parse(reply, "load the collections"),
        Ok((status, reply)) => {
            failed("load the collections", Refused::Status { status, reply });
            None
        }
        Err(()) => {
            failed("load the collections", Refused::Unreachable);
            None
        }
    }
}

/// A page of `collection`'s records, as the collection's rules allow.
pub async fn fetch_records(collection: &str) -> Option<Records> {
    let token = token("load the records")?;
    let path = format!("/api/collections/{collection}/records");
    match send("GET", &path, None, Some(&token)).await {
        Ok((200, reply)) => parse(reply, "load the records"),
        Ok((status, reply)) => {
            failed("load the records", Refused::Status { status, reply });
            None
        }
        Err(()) => {
            failed("load the records", Refused::Unreachable);
            None
        }
    }
}

/// Checks the stored session on launch.
pub async fn resume() {
    let Some(session) = session() else {
        return;
    };
    // One request at a time: a refusal is refreshed here, never by two requests at once.
    match send("GET", "/api/me", None, Some(&session.token)).await {
        Ok((401, _)) => {
            if !refresh(&session.refresh_token).await {
                // The stored session is spent: sign out, and say why.
                toast::failure(
                    "Session expired",
                    "Sign in again with your e-mail and password.",
                );
                sign_out();
            }
        }
        // Offline, or another answer: keep the session and let the next request report.
        // shortcut: an unreachable backend is not reported at launch; the first
        // action that needs it raises the toast instead.
        _ => {}
    }
}

async fn refresh(refresh_token: &str) -> bool {
    let body = json!({ "refreshToken": refresh_token }).to_string();
    matches!(
        send("POST", &format!("{SUPERUSERS}/auth-refresh"), Some(body), None).await,
        Ok((200, reply)) if { store_session(&reply); true }
    )
}

/// Signs out: ends the session on the server (best effort) and here, then shows sign-in.
pub fn sign_out() {
    if let Some(session) = session() {
        spawn(async move {
            let body = json!({ "refreshToken": session.refresh_token }).to_string();
            let _ = send(
                "POST",
                &format!("{SUPERUSERS}/logout"),
                Some(body),
                Some(&session.token),
            )
            .await;
        });
    }
    Storage::remove_item(SESSION_KEY);
    nav::navigate(Route::Login);
}

async fn send(
    method: &str,
    path: &str,
    body: Option<String>,
    token: Option<&str>,
) -> Result<(u16, JsonValue), ()> {
    let base = base_url();
    let (status, reply) = request(&base, method, path, &body, token).await?;
    // A web dev server answers `/api/...` with its own HTML 404 (`nsh dev` builds
    // the web target without debug assertions): the API is on its dev port then.
    if status == 404 && reply.is_null() && base.is_empty() {
        return request(DEV_API, method, path, &body, token).await;
    }
    Ok((status, reply))
}

async fn request(
    base: &str,
    method: &str,
    path: &str,
    body: &Option<String>,
    token: Option<&str>,
) -> Result<(u16, JsonValue), ()> {
    let mut request =
        HttpRequest::new(method, format!("{base}{path}")).header("accept", "application/json");
    if let Some(body) = body {
        request = request
            .header("content-type", "application/json")
            .body(body.clone().into_bytes());
    }
    if let Some(token) = token {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    let response = fetch(request).await.map_err(|_| ())?;
    let reply = serde_json::from_str(&response.text()).unwrap_or(JsonValue::Null);
    Ok((response.status(), reply))
}

/// The access token, or a toast that says a session is needed.
fn token(action: &str) -> Option<String> {
    let token = session().map(|s| s.token);
    if token.is_none() {
        toast::failure("Sign in first", format!("You need a session to {action}."));
    }
    token
}

/// A 2xx reply as `T`; a body of another shape is reported as a failure.
fn parse<T: DeserializeOwned>(reply: JsonValue, action: &str) -> Option<T> {
    match serde_json::from_value(reply) {
        Ok(value) => Some(value),
        Err(_) => {
            toast::failure(
                format!("Can't {action}"),
                "The backend answered with something else.",
            );
            None
        }
    }
}

/// Raises a toast that says what failed and what to do, and reports failure.
fn failed(action: &str, error: Refused) -> bool {
    let (title, hint) = match error {
        Refused::Unreachable => (
            "Can't reach the backend".to_owned(),
            "Check that lazypock is running, then try again.".to_owned(),
        ),
        Refused::Status { status, reply } => {
            let said = message_of(&reply);
            let or = |fallback: &str| said.clone().unwrap_or_else(|| fallback.to_owned());
            match status {
                400 | 401 | 422 => (
                    format!("Can't {action}"),
                    or("Check the values and try again."),
                ),
                403 => ("Not allowed".to_owned(), or("This account cannot do that.")),
                404 => (
                    "The backend has no such endpoint".to_owned(),
                    "Check the backend URL in Settings.".to_owned(),
                ),
                409 => (
                    "Already done".to_owned(),
                    or("There is nothing left to do here."),
                ),
                429 => (
                    "Too many attempts".to_owned(),
                    "Wait a minute, then try again.".to_owned(),
                ),
                _ if status >= 500 => (
                    "The backend had a problem".to_owned(),
                    or("Try again in a moment."),
                ),
                _ => (
                    "That didn't work".to_owned(),
                    or(&format!("The backend answered {status}.")),
                ),
            }
        }
    };
    toast::failure(title, hint);
    false
}

/// The API's message without core's error-kind prefix (`unauthorized: ...`).
fn message_of(reply: &JsonValue) -> Option<String> {
    let message = reply.get("message").and_then(JsonValue::as_str)?;
    let message = message.split_once(": ").map_or(message, |(_, rest)| rest);
    (!message.is_empty()).then(|| message.to_owned())
}
