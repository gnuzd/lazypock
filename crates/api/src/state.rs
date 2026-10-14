//! Shared application state.

use std::sync::Arc;

use lazypock_auth::AuthService;
use lazypock_core::config::Config;
use lazypock_db::pool::Db;
use lazypock_db::schema::SchemaCache;
use lazypock_files::Files;
use lazypock_hooks::HookService;
use lazypock_realtime::Hub;

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
    /// Live subscriptions on this node.
    pub hub: Arc<Hub>,
    /// File storage: validation rules plus the object backend.
    pub files: Arc<Files>,
    /// The hook worker, or a handle to one that has nothing loaded.
    pub hooks: Arc<HookService>,
}

impl AppState {
    /// Build the state.
    pub fn new(
        config: Config,
        db: Db,
        schema: Arc<SchemaCache>,
        auth: Arc<AuthService>,
        hub: Arc<Hub>,
        files: Arc<Files>,
        hooks: Arc<HookService>,
    ) -> Self {
        Self {
            config: Arc::new(config),
            db,
            schema,
            auth,
            hub,
            files,
            hooks,
        }
    }
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState").finish_non_exhaustive()
    }
}
