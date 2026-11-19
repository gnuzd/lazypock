use crate::state::AppState;
use axum::{Json, Router, routing::get};
use serde_json::{Value, json};

pub fn config(r: Router<AppState>) -> Router<AppState> {
    r.route("/api/health", get(health))
}

async fn health() -> Json<Value> {
    let unavailable = lazypock_core::health::unavailable();

    let mut data = serde_json::Map::new();
    // Compile-time, from the workspace version, so a binary built from a release tag
    // reports that tag: a deployment can tell a stale container from a fresh one
    // without shell access. `version` is always present; `unavailable` is not.
    data.insert("version".to_string(), json!(env!("CARGO_PKG_VERSION")));
    if !unavailable.is_empty() {
        data.insert("unavailable".to_string(), json!(unavailable));
    }

    Json(json!({
        "code": 200,
        "message": "OK",
        "data": data,
    }))
}
