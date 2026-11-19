pub mod health;

use axum::Router;

use crate::state::AppState;

type Config = fn(Router<AppState>) -> Router<AppState>;
const CONFIGS: &[Config] = &[health::config];

pub fn router() -> Router<AppState> {
    CONFIGS.iter().fold(Router::new(), |r, f| f(r))
}
