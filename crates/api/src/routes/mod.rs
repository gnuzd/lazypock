pub mod auth;
pub mod collections;
pub mod health;
pub mod records;
pub mod setup;
pub mod studio;

use crate::error::not_found;
use crate::state::AppState;
use axum::Router;
use tower_http::cors::CorsLayer;

type Config = fn(Router<AppState>) -> Router<AppState>;
const CONFIGS: &[Config] = &[
    health::config,
    setup::config,
    auth::config,
    records::config,
    collections::config,
    studio::config,
];

pub fn router() -> Router<AppState> {
    CONFIGS
        .iter()
        .fold(Router::new(), |r, f| f(r))
        // shortcut: any origin may call the API, so the studio also works from its dev
        // server; auth is a bearer token, never a cookie. Tighten before a public deployment.
        .layer(CorsLayer::permissive())
        .fallback(not_found)
}
