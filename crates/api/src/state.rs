use std::sync::Arc;

use crate::login_guard::LoginGuard;
use lazypock_auth::AuthService;
use lazypock_core::Config;
use lazypock_db::{Db, SchemaStore};

#[derive(Debug, Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub db: Db,
    /// Collections: read with `.get()` / `.cache().snapshot()`, change with `.create()` etc.
    pub schemas: SchemaStore,
    pub auth: Arc<AuthService>,
    pub login_guard: Arc<LoginGuard>,
}

impl AppState {
    pub fn new(config: Config, db: Db, schemas: SchemaStore, auth: Arc<AuthService>) -> Self {
        AppState {
            config: Arc::new(config),
            db,
            schemas,
            auth,
            login_guard: Arc::new(LoginGuard::default()),
        }
    }
}
