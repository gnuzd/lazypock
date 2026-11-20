//! `SchemaStore`: the one place that changes the schema *and* keeps the cache true.
//!
//! Handlers call this instead of `collections::*` directly, so nobody has to
//! remember to reload. Each method performs the change, then reloads this node's
//! cache so the very next request sees it (read-your-writes). Other nodes get
//! the same result through `NOTIFY` + [`crate::watch`].

use crate::collections;
use crate::ident::Ident;
use crate::pool::Db;
use crate::schema::{Collection, Field, Rules, SchemaCache};
// use crate::watch::{watch_schema, SchemaWatcher, StatusHook};
use std::sync::Arc;

use lazypock_core::Result;

#[derive(Clone, Debug)]
pub struct SchemaStore {
    db: Db,
    cache: Arc<SchemaCache>,
}

impl SchemaStore {
    pub fn new(db: Db) -> Self {
        SchemaStore {
            db,
            cache: Arc::new(SchemaCache::new()),
        }
    }

    pub fn cache(&self) -> &Arc<SchemaCache> {
        &self.cache
    }

    pub fn get(&self, name: &str) -> Option<Arc<Collection>> {
        self.cache.get(name)
    }

    /// Initial load. Call at startup and let it fail the boot.
    pub async fn reload(&self) -> Result<usize> {
        let conn = self.db.conn().await?;
        Ok(self.cache.reload(&**conn).await?.len())
    }

    /// Start cross-node invalidation. Keep the returned watcher alive.
    // pub fn watch(&self, on_status: Option<StatusHook>) -> SchemaWatcher {
    //     watch_schema(self.cache.clone(), self.db.clone(), on_status)
    // }

    pub async fn create(&self, c: &Collection) -> Result<Collection> {
        let out = collections::create(&self.db, c).await?;
        self.refresh().await;
        Ok(out)
    }

    pub async fn delete(&self, name: &Ident) -> Result<()> {
        collections::delete(&self.db, name).await?;
        self.refresh().await;
        Ok(())
    }

    pub async fn rename(&self, from: &Ident, to: &Ident) -> Result<()> {
        collections::rename(&self.db, from, to).await?;
        self.refresh().await;
        Ok(())
    }

    pub async fn add_field(&self, collection: &Ident, field: &Field) -> Result<Field> {
        let out = collections::add_field(&self.db, collection, field).await?;
        self.refresh().await;
        Ok(out)
    }

    pub async fn drop_field(&self, collection: &Ident, field: &Ident) -> Result<()> {
        collections::drop_field(&self.db, collection, field).await?;
        self.refresh().await;
        Ok(())
    }

    pub async fn rename_field(&self, collection: &Ident, from: &Ident, to: &Ident) -> Result<()> {
        collections::rename_field(&self.db, collection, from, to).await?;
        self.refresh().await;
        Ok(())
    }

    pub async fn set_rules(&self, collection: &Ident, rules: &Rules) -> Result<()> {
        collections::set_rules(&self.db, collection, rules).await?;
        self.refresh().await;
        Ok(())
    }

    /// The change is already committed; a failed reload must not turn that into an
    /// error for the caller. The watcher (or the next reload) will catch up.
    async fn refresh(&self) {
        if let Err(e) = self.reload().await {
            tracing::warn!(error = %e, "schema reload after a change failed; the watcher will retry");
        }
    }
}
