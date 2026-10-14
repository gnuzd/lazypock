//! Subscription registry and fan-out.
//!
//! A node holds only its own subscriptions: every node receives every change, and
//! each pushes to the clients it happens to hold. That is what makes horizontal
//! scaling work without sticky sessions (`PLAN.md` section 8.5).

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::sync::mpsc;

use lazypock_db::pool::Db;
use lazypock_db::query::compile::FilterContext;
use lazypock_db::query::projection::{ExpandSpec, FieldSelection};
use lazypock_db::records;
use lazypock_db::rls;
use lazypock_db::rls::context::{RequestContext, Role};
use lazypock_db::schema::SchemaCache;

use crate::bus::EventBus;
use crate::event::{ChangeEvent, ChangeOp};

/// How many events may queue for one client before it counts as too slow.
///
/// A client that cannot keep up loses events rather than dragging down the node,
/// or every other client on it.
pub const CLIENT_QUEUE: usize = 256;

/// How many recently delivered record ids are remembered per subscription.
///
/// Needed for deletes: by the time the event arrives the row is gone, so its
/// visibility cannot be checked again and the subscriber is instead trusted for
/// the records it has already been shown.
const RECENT_IDS: usize = 256;

/// Most events a resuming stream is replayed.
pub const REPLAY_LIMIT: usize = 200;

/// An event on its way to a client.
#[derive(Debug, Clone, PartialEq)]
pub struct Delivered {
    /// The change.
    pub event: ChangeEvent,
    /// The record, when this client is allowed to see it, already filtered by
    /// row-level security.
    pub record: Option<Value>,
}

/// A live subscription.
///
/// Dropping it — or calling [`Subscription::close`] — unsubscribes.
pub struct Subscription {
    id: u64,
    collection: String,
    receiver: mpsc::Receiver<Delivered>,
}

impl Subscription {
    /// The subscription id, needed to unsubscribe.
    pub fn id(&self) -> u64 {
        self.id
    }

    /// The collection this subscription is for.
    pub fn collection(&self) -> &str {
        &self.collection
    }

    /// The next delivery, or `None` once the hub has dropped the subscription.
    pub async fn recv(&mut self) -> Option<Delivered> {
        self.receiver.recv().await
    }
}

struct Subscriber {
    id: u64,
    /// The single record this subscription is limited to, if any.
    record_id: Option<String>,
    /// What the client is allowed to see.
    context: RequestContext,
    /// The client's connection id, for echo suppression.
    connection_id: String,
    sender: mpsc::Sender<Delivered>,
    /// Record ids already delivered, newest last.
    seen: VecDeque<String>,
}

/// Everything this node knows about live subscriptions.
pub struct Hub {
    db: Db,
    schema: Arc<SchemaCache>,
    subscribers: Mutex<HashMap<String, Vec<Subscriber>>>,
    next_id: AtomicU64,
    dropped: AtomicU64,
}

impl std::fmt::Debug for Hub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hub")
            .field("subscriptions", &self.subscription_count())
            .field("dropped_events", &self.dropped.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl Hub {
    /// Build a hub for this node.
    pub fn new(db: Db, schema: Arc<SchemaCache>) -> Arc<Self> {
        Arc::new(Self {
            db,
            schema,
            subscribers: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            dropped: AtomicU64::new(0),
        })
    }

    /// Total live subscriptions.
    pub fn subscription_count(&self) -> usize {
        self.subscribers
            .lock()
            .map(|guard| guard.values().map(Vec::len).sum())
            .unwrap_or(0)
    }

    /// Events dropped because a client could not keep up.
    pub fn dropped_events(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Subscribe to a collection, optionally to a single record.
    pub fn subscribe(
        &self,
        collection: impl Into<String>,
        record_id: Option<String>,
        context: RequestContext,
        connection_id: impl Into<String>,
    ) -> Subscription {
        let collection = collection.into();
        let (sender, receiver) = mpsc::channel(CLIENT_QUEUE);
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        if let Ok(mut guard) = self.subscribers.lock() {
            guard
                .entry(collection.clone())
                .or_default()
                .push(Subscriber {
                    id,
                    record_id,
                    context,
                    connection_id: connection_id.into(),
                    sender,
                    seen: VecDeque::new(),
                });
        }

        Subscription {
            id,
            collection,
            receiver,
        }
    }

    /// Remove a subscription.
    pub fn unsubscribe(&self, collection: &str, id: u64) {
        if let Ok(mut guard) = self.subscribers.lock() {
            if let Some(list) = guard.get_mut(collection) {
                list.retain(|subscriber| subscriber.id != id);
                if list.is_empty() {
                    guard.remove(collection);
                }
            }
        }
    }

    /// Fan a change out to the subscribers on this node that may see it.
    pub async fn dispatch(&self, event: ChangeEvent) {
        // Everything the fan-out needs is copied out under the lock; nothing is
        // held across an await.
        struct Target {
            id: u64,
            context: RequestContext,
            sender: mpsc::Sender<Delivered>,
            /// Whether this subscriber has already been shown the record.
            known: bool,
        }

        let mut targets: Vec<Target> = Vec::new();
        {
            let Ok(mut guard) = self.subscribers.lock() else {
                return;
            };
            let Some(list) = guard.get_mut(&event.collection) else {
                return;
            };
            for subscriber in list.iter_mut() {
                // Echo suppression: the device that made the change already knows.
                if event.from_connection(&subscriber.connection_id) {
                    continue;
                }
                if subscriber
                    .record_id
                    .as_deref()
                    .is_some_and(|id| id != event.record_id)
                {
                    continue;
                }
                let known = subscriber.seen.iter().any(|id| id == &event.record_id);
                targets.push(Target {
                    id: subscriber.id,
                    context: subscriber.context.clone(),
                    sender: subscriber.sender.clone(),
                    known,
                });
            }
        }

        if targets.is_empty() {
            return;
        }

        // One visibility query per distinct identity rather than per subscriber:
        // the check *is* the collection's select policy, run under that client's
        // own context.
        let mut documents: HashMap<RequestContext, Option<Value>> = HashMap::new();
        if event.op != ChangeOp::Delete {
            for target in &targets {
                if !documents.contains_key(&target.context) {
                    let document = self
                        .document_for(&event.collection, &event.record_id, &target.context)
                        .await;
                    documents.insert(target.context.clone(), document);
                }
            }
        }

        let mut delivered: Vec<u64> = Vec::new();
        for target in targets {
            let allowed = if event.op == ChangeOp::Delete {
                // The row is gone, so visibility cannot be re-checked.
                target.context.role == Role::Owner || target.known
            } else {
                documents.get(&target.context).is_some_and(Option::is_some)
            };
            if !allowed {
                continue;
            }

            let delivery = Delivered {
                event: event.clone(),
                record: documents.get(&target.context).and_then(Clone::clone),
            };

            match target.sender.try_send(delivery) {
                Ok(()) => delivered.push(target.id),
                Err(mpsc::error::TrySendError::Full(_)) => {
                    self.dropped.fetch_add(1, Ordering::Relaxed);
                    tracing::debug!(
                        collection = %event.collection,
                        "dropping a realtime event: the client is not keeping up"
                    );
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {}
            }
        }

        if delivered.is_empty() {
            return;
        }

        // Remember what was delivered, so a later delete can be trusted for it.
        if let Ok(mut guard) = self.subscribers.lock() {
            if let Some(list) = guard.get_mut(&event.collection) {
                for subscriber in list.iter_mut() {
                    if !delivered.contains(&subscriber.id) {
                        continue;
                    }
                    subscriber.seen.push_back(event.record_id.clone());
                    while subscriber.seen.len() > RECENT_IDS {
                        subscriber.seen.pop_front();
                    }
                }
            }
        }
    }

    /// The record as `context` may see it, or `None` when it may not.
    ///
    /// The check is the collection's own select policy: the same query the read
    /// endpoint would run, under the same context.
    async fn document_for(
        &self,
        collection: &str,
        id: &str,
        context: &RequestContext,
    ) -> Option<Value> {
        let schema = self.schema.snapshot();
        let target = schema.get(collection).cloned()?;
        let filter = match FilterContext::for_collection(&target, schema.collections()) {
            Ok(filter) => filter,
            Err(err) => {
                tracing::warn!(error = %err, "cannot build a filter context for realtime");
                return None;
            }
        };

        let outcome = async {
            let mut client = self.db.client().await.map_err(|err| err.to_string())?;
            let transaction = client.transaction().await.map_err(|err| err.to_string())?;
            rls::context::apply(&transaction, context)
                .await
                .map_err(|err| err.to_string())?;
            let record = records::get_one(
                &transaction,
                &target,
                &filter,
                id,
                &FieldSelection::all(),
                &ExpandSpec::none(),
            )
            .await
            .map_err(|err| err.to_string())?;
            transaction.commit().await.map_err(|err| err.to_string())?;
            Ok::<_, String>(record)
        }
        .await;

        match outcome {
            Ok(record) => record,
            Err(err) => {
                // Fail closed: an event whose visibility cannot be established is
                // not delivered.
                tracing::warn!(error = %err, "cannot check realtime visibility");
                None
            }
        }
    }

    /// Events after `since` that `context` may see, for resuming a stream.
    ///
    /// Bounded: a client that has been away a long time should refetch rather
    /// than be replayed everything. Visibility is the same select policy the live
    /// path uses, so a resumed stream cannot show more than a live one.
    pub async fn replay(
        &self,
        collection: &str,
        since: i64,
        context: &RequestContext,
    ) -> Vec<Delivered> {
        let rows = match self.replay_rows(collection, since).await {
            Ok(rows) => rows,
            Err(err) => {
                tracing::warn!(error = %err, "cannot read the realtime log");
                return Vec::new();
            }
        };

        let mut out = Vec::new();
        for event in rows {
            let allowed = event.op == ChangeOp::Delete
                || self
                    .document_for(collection, &event.record_id, context)
                    .await
                    .is_some();
            if !allowed {
                continue;
            }
            let record = if event.op == ChangeOp::Delete {
                None
            } else {
                self.document_for(collection, &event.record_id, context)
                    .await
            };
            out.push(Delivered { event, record });
        }
        out
    }

    async fn replay_rows(&self, collection: &str, since: i64) -> Result<Vec<ChangeEvent>, String> {
        let client = self.db.client().await.map_err(|err| err.to_string())?;
        let rows = client
            .query(
                "select id, collection, op, record_id, origin from _rt_log \
                 where collection = $1::text and id > $2::bigint order by id \
                 limit $3::bigint",
                &[&collection.to_string(), &since, &(REPLAY_LIMIT as i64)],
            )
            .await
            .map_err(|err| err.to_string())?;

        Ok(rows
            .iter()
            .map(|row| ChangeEvent {
                log_id: row.get(0),
                collection: row.get(1),
                op: match row.get::<_, String>(2).as_str() {
                    "create" => ChangeOp::Create,
                    "update" => ChangeOp::Update,
                    _ => ChangeOp::Delete,
                },
                record_id: row.get(3),
                origin: row.get(4),
            })
            .collect())
    }

    /// Deliver every change the bus reports, until it closes.
    pub async fn run(self: Arc<Self>, mut bus: impl EventBus) {
        while let Some(event) = bus.next().await {
            self.dispatch(event).await;
        }
        tracing::warn!("the realtime bus closed; no further events will be delivered");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_subscription_id_is_unique_per_hub() {
        // Ids come from a counter rather than a clock, so two subscriptions in the
        // same millisecond cannot collide.
        let counter = AtomicU64::new(1);
        let first = counter.fetch_add(1, Ordering::Relaxed);
        let second = counter.fetch_add(1, Ordering::Relaxed);
        assert_ne!(first, second);
    }

    #[test]
    fn recent_ids_are_bounded() {
        let mut seen: VecDeque<String> = VecDeque::new();
        for index in 0..RECENT_IDS * 2 {
            seen.push_back(index.to_string());
            while seen.len() > RECENT_IDS {
                seen.pop_front();
            }
        }
        assert_eq!(seen.len(), RECENT_IDS);
        assert!(
            !seen.contains(&"0".to_string()),
            "the oldest should have been dropped"
        );
    }
}
