//! Shared application state.

use std::sync::Arc;

use lazypock_auth::AuthService;
use lazypock_core::config::Config;
use lazypock_db::pool::Db;
use lazypock_db::schema::SchemaCache;

/// State handed to every handler.
///
/// `Db` is cheap to clone (the pool is reference counted) and the schema cache is
/// shared so a reload on any node is visible to every handler immediately.
#[derive(Clone)]
pub struct AppState {
    /// Resolved runtime configuration.
    pub config: Arc<Config>,
    /// Database handle.
    pub db: Db,
    /// Schema snapshot cache.
    pub schema: Arc<SchemaCache>,
    /// Signing keys and the login/refresh flows.
    pub auth: Arc<AuthService>,
}

impl AppState {
    /// Build the state.
    pub fn new(config: Config, db: Db, schema: Arc<SchemaCache>, auth: Arc<AuthService>) -> Self {
        Self {
            config: Arc::new(config),
            db,
            schema,
            auth,
        }
    }
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState").finish_non_exhaustive()
    }
}
