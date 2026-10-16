//! In-RAM schema cache.
//!
//! Reads on the request path must never take a lock or hit the database for
//! collection metadata, so the schema lives behind an [`ArcSwap`]: readers grab
//! an immutable snapshot, and a schema change swaps in a new one. Other nodes
//! are told to reload by `NOTIFY lazypock_schema_changed`
//! ([`crate::collections::SCHEMA_CHANGED_CHANNEL`]).

use std::collections::HashMap;
use std::sync::Arc;

use arc_swap::ArcSwap;
use lazypock_core::error::Result;
use tokio_postgres::Client;

use crate::collections::{self, Collection};

/// An immutable snapshot of every collection.
///
/// Lookup indexes are built once, at construction, so `get` is a hash lookup.
#[derive(Debug, Default)]
pub struct Schema {
    collections: Vec<Collection>,
    by_name: HashMap<String, usize>,
    by_id: HashMap<String, usize>,
}

impl Schema {
    /// Build a snapshot from a collection list.
    pub fn new(collections: Vec<Collection>) -> Self {
        let mut by_name = HashMap::with_capacity(collections.len());
        let mut by_id = HashMap::with_capacity(collections.len());
        for (position, collection) in collections.iter().enumerate() {
            by_name.insert(collection.name.clone(), position);
            by_id.insert(collection.id.clone(), position);
        }
        Self {
            collections,
            by_name,
            by_id,
        }
    }

    /// Load a fresh snapshot from the database.
    pub async fn load(client: &Client) -> Result<Self> {
        Ok(Self::new(collections::list(client).await?))
    }

    /// Every collection, ordered by name.
    pub fn collections(&self) -> &[Collection] {
        &self.collections
    }

    /// Look a collection up by name or id.
    pub fn get(&self, name_or_id: &str) -> Option<&Collection> {
        self.by_name
            .get(name_or_id)
            .or_else(|| self.by_id.get(name_or_id))
            .map(|&position| &self.collections[position])
    }

    /// Number of collections.
    pub fn len(&self) -> usize {
        self.collections.len()
    }

    /// Whether there are no collections.
    pub fn is_empty(&self) -> bool {
        self.collections.is_empty()
    }
}

/// Atomically swappable schema snapshot.
#[derive(Debug)]
pub struct SchemaCache {
    inner: ArcSwap<Schema>,
}

impl Default for SchemaCache {
    fn default() -> Self {
        Self::new()
    }
}

impl SchemaCache {
    /// An empty cache. Call [`SchemaCache::reload`] before serving requests.
    pub fn new() -> Self {
        Self {
            inner: ArcSwap::from_pointee(Schema::default()),
        }
    }

    /// The current snapshot. Cheap: no locks, no cloning of the collections.
    pub fn snapshot(&self) -> Arc<Schema> {
        self.inner.load_full()
    }

    /// Replace the snapshot.
    pub fn store(&self, schema: Schema) {
        self.inner.store(Arc::new(schema));
    }

    /// Reload from the database and install the result.
    ///
    /// On failure the previous snapshot is kept: a transient database error must
    /// not take the whole schema offline.
    pub async fn reload(&self, client: &Client) -> Result<Arc<Schema>> {
        let schema = Arc::new(Schema::load(client).await?);
        self.inner.store(schema.clone());
        tracing::debug!(collections = schema.len(), "schema cache reloaded");
        Ok(schema)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::Field;
    use crate::types::CollectionType;
    use serde_json::json;
    use std::time::SystemTime;

    fn collection(id: &str, name: &str) -> Collection {
        Collection {
            id: id.into(),
            name: name.into(),
            collection_type: CollectionType::Base,
            options: json!({}),
            rules: json!({}),
            fields: Vec::<Field>::new(),
            created: SystemTime::now(),
            updated: SystemTime::now(),
        }
    }

    #[test]
    fn lookups_work_by_name_and_by_id() {
        let schema = Schema::new(vec![
            collection("c1", "posts"),
            collection("c2", "comments"),
        ]);

        assert_eq!(schema.len(), 2);
        assert!(!schema.is_empty());
        assert_eq!(schema.get("posts").unwrap().id, "c1");
        assert_eq!(schema.get("c2").unwrap().name, "comments");
        assert!(schema.get("missing").is_none());
        assert_eq!(
            schema
                .collections()
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            vec!["posts", "comments"]
        );
    }

    #[test]
    fn cache_swaps_snapshots_atomically() {
        let cache = SchemaCache::new();
        assert!(cache.snapshot().is_empty());

        cache.store(Schema::new(vec![collection("c1", "posts")]));
        let first = cache.snapshot();
        assert_eq!(first.get("c1").unwrap().name, "posts");

        cache.store(Schema::new(vec![collection("c2", "comments")]));
        assert!(cache.snapshot().get("posts").is_none());
        assert_eq!(cache.snapshot().get("comments").unwrap().id, "c2");
        // The previously handed-out snapshot is unaffected.
        assert_eq!(first.get("posts").unwrap().name, "posts");
    }

    #[test]
    fn default_snapshot_is_empty() {
        let schema = Schema::default();
        assert!(schema.is_empty());
        assert_eq!(schema.len(), 0);
    }
}
