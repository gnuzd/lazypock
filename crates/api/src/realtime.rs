//! The realtime SSE endpoint (`PLAN.md` section 8.1).
//!
//! A client subscribes here and receives one SSE event per change it is allowed
//! to see, filtered by the same row-level security as an ordinary read.

use std::convert::Infallible;
use std::pin::Pin;
use std::time::Duration;

use axum::extract::{Extension, Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::{stream, StreamExt};
use lazypock_core::error::Error;
use lazypock_db::rls::context::{Action, RequestContext};
use lazypock_realtime::{Delivered, Hub, Subscription};
use serde::Deserialize;
use serde_json::json;

use crate::auth::Principal;
use crate::error::ApiError;
use crate::state::AppState;

/// Heartbeat interval. Long enough to be cheap, short enough that a dead proxy
/// connection is noticed.
const HEARTBEAT: Duration = Duration::from_secs(25);

/// Query parameters for `GET /api/realtime`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RealtimeParams {
    /// What to subscribe to, comma separated: `posts`, `posts/p1`, or `*`.
    ///
    /// A single value rather than a repeated parameter because that is what the
    /// query-string parser supports; the SDK builds it.
    pub subscribe: Option<String>,
    /// Resume after this `_rt_log` id, which is the `id` of the last event the
    /// client saw.
    pub last_event_id: Option<i64>,
}

/// `GET /api/realtime`
pub async fn realtime(
    State(state): State<AppState>,
    Query(params): Query<RealtimeParams>,
    Extension(principal): Extension<Principal>,
) -> Result<Response, ApiError> {
    // The base context; each subscription overrides its action (list for a
    // collection, view for a record) before it is stored.
    let context = principal.request_context(Action::List);
    let connection_id = context.connection_id.clone();

    let requested = match params.subscribe.as_deref().map(str::trim) {
        None | Some("") => {
            return Err(ApiError::new(Error::invalid(
                "`subscribe` is required, for example `subscribe=posts` or `subscribe=*`",
            )))
        }
        Some(value) => value,
    };

    let mut subscriptions: Vec<(Subscription, RequestContext)> = Vec::new();
    for spec in requested.split(',') {
        let spec = spec.trim();
        if spec.is_empty() {
            continue;
        }
        if spec == lazypock_realtime::REGISTRY_TOPIC && !principal.is_superuser() {
            return Err(ApiError::new(Error::forbidden(
                "the collection registry is only available to superusers",
            )));
        }
        for (collection, record_id) in expand(&state, spec)? {
            // A collection-wide subscription is a list; a record-specific one is
            // a view. They must not share an action, or a public `viewRule`
            // would widen a subscription to the whole collection.
            let action = if record_id.is_some() {
                Action::View
            } else {
                Action::List
            };
            let context = context.clone().with_action(action);
            let subscription = state.hub.subscribe(
                collection,
                record_id,
                context.clone(),
                connection_id.clone(),
            );
            subscriptions.push((subscription, context));
        }
    }

    if subscriptions.is_empty() {
        return Err(ApiError::new(Error::invalid(
            "`subscribe` did not name any collection",
        )));
    }

    // Replay first, so a resuming client is caught up before live events arrive.
    let mut replay: Vec<Event> = Vec::new();
    if let Some(since) = params.last_event_id {
        for (subscription, context) in &subscriptions {
            for delivery in state
                .hub
                .replay(subscription.collection(), since, context)
                .await
            {
                replay.push(to_sse(&delivery));
            }
        }
    }

    // Boxed so they can be merged: `select_all` needs the streams to be `Unpin`,
    // and these are built from async blocks.
    let streams: Vec<EventStream> = subscriptions
        .into_iter()
        .map(|(subscription, _context)| -> EventStream {
            Box::pin(subscription_stream(state.hub.clone(), subscription))
        })
        .collect();

    let live = stream::select_all(streams);
    let body = stream::iter(replay.into_iter().map(Ok::<_, Infallible>)).chain(live);

    Ok(Sse::new(body)
        .keep_alive(KeepAlive::new().interval(HEARTBEAT))
        .into_response())
}

/// Turn one subscription specifier into concrete subscriptions.
fn expand(state: &AppState, spec: &str) -> Result<Vec<(String, Option<String>)>, ApiError> {
    // The collection **registry** is a topic, not a table: it reports changes to the
    // collection definitions themselves, and is what the Studio subscribes to so its
    // sidebar and record table refresh. It is gated to superusers where it is
    // expanded, because the payload is a collection's definition (its rules).
    if spec == lazypock_realtime::REGISTRY_TOPIC {
        return Ok(vec![(lazypock_realtime::REGISTRY_TOPIC.to_string(), None)]);
    }

    if spec == "*" {
        let schema = state.schema.snapshot();
        return Ok(schema
            .collections()
            .iter()
            .map(|collection| (collection.name.clone(), None))
            .collect());
    }

    let (collection, record_id) = match spec.split_once('/') {
        Some((collection, record_id)) => (collection, Some(record_id.to_string())),
        None => (spec, None),
    };
    if collection.is_empty() {
        return Err(ApiError::new(Error::invalid(format!(
            "`{spec}` does not name a collection"
        ))));
    }

    // Anything else is refused, so a typo is not silently a subscription that
    // never fires.
    let schema = state.schema.snapshot();
    let Some(resolved) = schema.get(collection) else {
        return Err(ApiError::new(Error::not_found(format!(
            "no collection named {collection:?}"
        ))));
    };
    // A view fires no change events (there is no trigger on it), so a subscription
    // would be an open stream that never delivers. Refusing says so.
    if resolved.collection_type == lazypock_db::types::CollectionType::View {
        return Err(ApiError::new(Error::invalid(format!(
            "the collection {collection:?} is a view: views deliver no realtime events",
        ))));
    }

    Ok(vec![(collection.to_string(), record_id)])
}

/// A boxed SSE stream, so several subscriptions can be merged into one response.
type EventStream = Pin<Box<dyn stream::Stream<Item = Result<Event, Infallible>> + Send>>;

/// One subscription as a stream, unsubscribing when it is dropped.
fn subscription_stream(
    hub: std::sync::Arc<Hub>,
    subscription: Subscription,
) -> impl stream::Stream<Item = Result<Event, Infallible>> + Send {
    struct State {
        subscription: Subscription,
        /// Present so the subscription is removed when the stream ends, however
        /// it ends — including a client that simply disconnects.
        _guard: Unsubscriber,
    }

    struct Unsubscriber {
        hub: std::sync::Arc<Hub>,
        collection: String,
        id: u64,
    }

    impl Drop for Unsubscriber {
        fn drop(&mut self) {
            self.hub.unsubscribe(&self.collection, self.id);
        }
    }

    let guard = Unsubscriber {
        collection: subscription.collection().to_string(),
        id: subscription.id(),
        hub,
    };

    stream::unfold(
        State {
            subscription,
            _guard: guard,
        },
        |mut state| async move {
            let delivery = state.subscription.recv().await?;
            Some((Ok(to_sse(&delivery)), state))
        },
    )
}

/// Render one delivery as an SSE event.
fn to_sse(delivery: &Delivered) -> Event {
    let data = json!({
        "collection": delivery.event.collection,
        "action": delivery.event.op.as_str(),
        "id": delivery.event.record_id,
        "record": delivery.record,
    });

    Event::default()
        // The `_rt_log` id, which is what a client sends back as `lastEventId`.
        .id(delivery.event.log_id.to_string())
        .event("record")
        .data(data.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazypock_realtime::{ChangeEvent, ChangeOp};
    use serde_json::Value;

    fn delivery(record: Option<Value>) -> Delivered {
        Delivered {
            event: ChangeEvent {
                log_id: 42,
                collection: "posts".into(),
                op: ChangeOp::Create,
                record_id: "p1".into(),
                origin: None,
            },
            record,
        }
    }

    #[test]
    fn an_event_carries_the_resume_id() {
        let event = to_sse(&delivery(None));
        // axum does not expose the fields, so the rendered form is checked here.
        assert!(format!("{event:?}").contains("42"), "{event:?}");
    }

    #[test]
    fn the_data_names_the_collection_action_and_record() {
        let event = to_sse(&delivery(Some(json!({"id": "p1"}))));
        let rendered = format!("{event:?}");
        assert!(rendered.contains("posts"), "{rendered}");
        assert!(rendered.contains("create"), "{rendered}");
        assert!(rendered.contains("p1"), "{rendered}");
    }

    #[test]
    fn a_delete_has_no_record() {
        let mut delivered = delivery(None);
        delivered.event.op = ChangeOp::Delete;
        let event = to_sse(&delivered);
        assert!(format!("{event:?}").contains("delete"), "{event:?}");
    }
}
