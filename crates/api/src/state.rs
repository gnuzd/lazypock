use std::sync::Arc;

use lazypock_core::Config;
use lazypock_db::pool::Db;

#[derive(Debug, Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: Db,
}

impl AppState {
    pub fn new(config: Config, db: Db) -> Self {
        AppState {
            config: Arc::new(config),
            db,
        }
    }
}
