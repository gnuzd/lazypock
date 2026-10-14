//! Shared application state.

use std::sync::Arc;

use lazypock_auth::AuthService;
use lazypock_core::config::Config;
use lazypock_db::pool::Db;
use lazypock_db::schema::SchemaCache;
use lazypock_files::Files;
use lazypock_hooks::HookService;

use crate::cors::CorsPolicy;
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
    /// The CORS allowlist, reloadable while the node runs.
    pub cors: Arc<CorsPolicy>,
}

impl AppState {
    /// Build the state.
    pub fn new(config: Config, db: Db, schema: Arc<SchemaCache>, services: Services) -> Self {
        let Services {
            auth,
            hub,
            files,
            hooks,
            cors,
        } = services;

        Self {
            config: Arc::new(config),
            db,
            schema,
            auth,
            hub,
            files,
            hooks,
            cors,
        }
    }
}

/// The services a node wires together.
///
/// Grouped so the state constructor takes four arguments rather than eight: a
/// positional list that long is one where two Arc-wrapped services of similar shape
/// can be swapped without the compiler noticing.
#[derive(Clone)]
pub struct Services {
    /// Authentication: passwords, tokens, sessions.
    pub auth: Arc<AuthService>,
    /// Realtime fan-out on this node.
    pub hub: Arc<Hub>,
    /// File storage.
    pub files: Arc<Files>,
    /// The hook worker.
    pub hooks: Arc<HookService>,
    /// The CORS allowlist.
    pub cors: Arc<CorsPolicy>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState").finish_non_exhaustive()
    }
}
