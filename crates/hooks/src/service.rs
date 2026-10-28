//! A hook runtime owned by a dedicated thread.
//!
//! QuickJS holds an `Rc` internally, so `rquickjs::Runtime` is not `Send` and a
//! runtime cannot live in shared state and be called from a request handler. It
//! has to be owned by exactly one thread. This is that thread, and the seam the
//! rest of the server sees is a channel.
//!
//! The thread also gives hook execution a natural serialisation point: one write
//! path at a time runs a hook, which is what a synchronous, in-transaction hook
//! wants anyway.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use lazypock_core::error::{Error, Result};
use lazypock_db::hooks::{HookContext, HookEvent, RecordHooks};
use lazypock_db::runner::BoxFuture;
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};

use crate::loader::{HookLoader, LoadReport};
#[cfg(test)]
use crate::runtime::HookLimits;
use crate::runtime::{HookSetup, RecordEvent};

/// How often the worker will look for edited hook files.
///
/// Hot reload is checked on the worker rather than by a watcher thread, throttled
/// so a busy write path cannot turn into a filesystem poll. The cost is that an
/// edit takes effect on the next hook run after this interval, not instantly.
const RELOAD_INTERVAL: Duration = Duration::from_secs(1);

/// One dispatch on its way to the worker.
///
/// Boxed in [`Request`]: the context alone is a couple of hundred bytes, and the
/// queue would otherwise carry that size around for every message.
struct Dispatch {
    event: RecordEvent,
    collection: String,
    payload: Value,
    /// The triggering request's context, which is what `$app` runs under.
    context: HookContext,
    reply: oneshot::Sender<Result<Option<Value>>>,
}

/// A request to the worker thread.
enum Request {
    Dispatch(Box<Dispatch>),
    Reload { reply: oneshot::Sender<LoadReport> },
}

/// A handle to the hook worker.
///
/// Cloning is cheap and the handle is `Send + Sync`, which is what lets it
/// implement [`RecordHooks`] and live in application state.
#[derive(Clone)]
pub struct HookService {
    requests: mpsc::UnboundedSender<Request>,
    /// Whether a runtime is currently serving, so the write path can skip the
    /// channel entirely when there is nothing to run.
    active: Arc<AtomicBool>,
}

impl HookService {
    /// Start a worker for a hooks directory.
    ///
    /// The worker starts even when the directory holds no hooks: it costs a thread
    /// that is idle, and it means a hook file appearing later is picked up without
    /// a restart.
    pub fn start(directory: impl Into<PathBuf>, setup: impl Into<HookSetup>) -> Self {
        let setup = setup.into();
        let (requests, mut queue) = mpsc::unbounded_channel::<Request>();
        let active = Arc::new(AtomicBool::new(false));
        let shared = Arc::clone(&active);

        let directory = directory.into();
        std::thread::Builder::new()
            .name("lazypock-hooks".to_string())
            .spawn(move || {
                let mut loader = HookLoader::new(&directory, setup);
                // Load once at startup so a misconfigured hook is reported at boot
                // rather than on the first write.
                let report = loader.load();
                if let Some(error) = &report.error {
                    tracing::error!(error = %error, "a hook file failed to load");
                }
                shared.store(loader.is_active(), Ordering::Relaxed);
                tracing::info!(
                    files = report.files,
                    hooks = report.hooks,
                    "hook worker started"
                );

                let mut last_check = Instant::now();
                // The queue closing is how the last handle going away stops the worker.
                while let Some(request) = queue.blocking_recv() {
                    match request {
                        Request::Dispatch(dispatch) => {
                            let Dispatch {
                                event,
                                collection,
                                payload,
                                context,
                                reply,
                            } = *dispatch;
                            // Look for edits here rather than in a watcher thread:
                            // one place owns the runtime, so one place reloads it.
                            if last_check.elapsed() >= RELOAD_INTERVAL {
                                last_check = Instant::now();
                                if let Some(report) = loader.reload_if_changed() {
                                    if let Some(error) = &report.error {
                                        tracing::error!(
                                            error = %error,
                                            "a hook edit did not compile; the previous hooks still serve"
                                        );
                                    }
                                    shared.store(loader.is_active(), Ordering::Relaxed);
                                }
                            }

                            let produced = if loader.is_active() {
                                match loader.dispatch(event, &collection, &payload, &context) {
                                    Ok(Some(value)) => Ok(Some(value)),
                                    Ok(None) => Ok(None),
                                    Err(err) => {
                                        tracing::error!(
                                            event = event.key(),
                                            collection = %collection,
                                            error = %err,
                                            "a hook failed"
                                        );
                                        Err(err)
                                    }
                                }
                            } else {
                                Ok(None)
                            };
                            // The caller may have gone away; that is not an error.
                            let _ = reply.send(produced);
                        }
                        Request::Reload { reply } => {
                            let report = loader.load();
                            if let Some(error) = &report.error {
                                tracing::error!(error = %error, "a hook file failed to load");
                            }
                            shared.store(loader.is_active(), Ordering::Relaxed);
                            let _ = reply.send(report);
                        }
                    }
                }

                // The last handle was dropped, so the worker stops and its runtime
                // is freed. There is nothing to interrupt: a dispatch is bounded by
                // the runtime's own deadline.
                tracing::info!("hook worker stopping");
            })
            .expect("spawning the hook worker thread");

        Self { requests, active }
    }

    /// Whether a runtime is currently serving.
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    /// Re-read every hook file, replacing the runtime.
    ///
    /// Used by the admin surface and by tests; the worker also reloads on its own
    /// as it notices edits.
    pub async fn reload(&self) -> Result<LoadReport> {
        let (reply, received) = oneshot::channel();
        self.requests
            .send(Request::Reload { reply })
            .map_err(|_| Error::internal("the hook worker is not running"))?;
        received
            .await
            .map_err(|_| Error::internal("the hook worker stopped before answering"))
    }

    /// Send a dispatch to the worker and wait for the result.
    async fn dispatch(
        &self,
        event: RecordEvent,
        collection: &str,
        payload: Value,
        context: HookContext,
    ) -> Result<Option<Value>> {
        let (reply, received) = oneshot::channel();
        self.requests
            .send(Request::Dispatch(Box::new(Dispatch {
                event,
                collection: collection.to_string(),
                payload,
                context,
                reply,
            })))
            .map_err(|_| Error::internal("the hook worker is not running"))?;
        received
            .await
            .map_err(|_| Error::internal("the hook worker stopped before answering"))?
    }
}

/// Map a write event onto the hook event the runtime knows.
fn runtime_event(event: HookEvent, before: bool) -> RecordEvent {
    match (event, before) {
        (HookEvent::Create, true) => RecordEvent::BeforeCreate,
        (HookEvent::Create, false) => RecordEvent::AfterCreate,
        (HookEvent::Update, true) => RecordEvent::BeforeUpdate,
        (HookEvent::Update, false) => RecordEvent::AfterUpdate,
        (HookEvent::Delete, true) => RecordEvent::BeforeDelete,
        (HookEvent::Delete, false) => RecordEvent::AfterDelete,
    }
}

impl RecordHooks for HookService {
    fn before<'a>(
        &'a self,
        event: HookEvent,
        collection: &'a str,
        record: Value,
    ) -> BoxFuture<'a, Result<Value>> {
        // A caller that did not say who triggered the write gets the anonymous
        // context: a hook that reaches the database can then only do what the
        // public rules allow, rather than inheriting the server's own access.
        let context = HookContext::anonymous(collection);
        Box::pin(async move { self.before_ctx(&context, event, collection, record).await })
    }

    fn before_ctx<'a>(
        &'a self,
        context: &'a HookContext,
        event: HookEvent,
        collection: &'a str,
        record: Value,
    ) -> BoxFuture<'a, Result<Value>> {
        Box::pin(async move {
            // Nothing loaded: skip the channel entirely, so an installation with no
            // hooks pays only an atomic load.
            if !self.is_active() {
                return Ok(record);
            }

            let payload = json!({
                "collection": collection,
                "action": event.as_str(),
                "record": record,
            });
            let produced = self
                .dispatch(
                    runtime_event(event, true),
                    collection,
                    payload,
                    context.clone(),
                )
                .await?;

            // A hook that changed the record decides what is written.
            match produced {
                Some(value) => Ok(value.get("record").cloned().unwrap_or(record)),
                None => Ok(record),
            }
        })
    }

    fn after<'a>(
        &'a self,
        event: HookEvent,
        collection: &'a str,
        record: Value,
    ) -> BoxFuture<'a, Result<()>> {
        let context = HookContext::anonymous(collection);
        Box::pin(async move { self.after_ctx(&context, event, collection, record).await })
    }

    fn after_ctx<'a>(
        &'a self,
        context: &'a HookContext,
        event: HookEvent,
        collection: &'a str,
        record: Value,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            if !self.is_active() {
                return Ok(());
            }

            let payload = json!({
                "collection": collection,
                "action": event.as_str(),
                "record": record,
            });
            self.dispatch(
                runtime_event(event, false),
                collection,
                payload,
                context.clone(),
            )
            .await?;
            Ok(())
        })
    }
}

impl std::fmt::Debug for HookService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HookService")
            .field("active", &self.is_active())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "lazypock-hookservice-{label}-{}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("temp dir");
            Self(path)
        }

        fn write(&self, name: &str, contents: &str) {
            fs::write(self.0.join(name), contents).expect("write");
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn a_hook_runs_on_the_worker_thread_and_decides_the_record() {
        let dir = TempDir::new("dispatch");
        dir.write(
            "main.pb.ts",
            r#"onRecordBeforeCreate("posts", (e) => { e.record.slug = "from-hook"; });"#,
        );

        let service = HookService::start(&dir.0, HookLimits::default());
        // The worker loads asynchronously; keep asking until it is ready.
        let mut waited = 0;
        while !service.is_active() && waited < 100 {
            tokio::time::sleep(Duration::from_millis(10)).await;
            waited += 1;
        }
        assert!(
            service.is_active(),
            "the worker should have loaded the hook"
        );

        let produced = service
            .before(HookEvent::Create, "posts", json!({ "title": "T" }))
            .await
            .expect("dispatch");
        assert_eq!(produced["slug"], json!("from-hook"));
    }

    #[tokio::test]
    async fn a_dispatch_with_no_hooks_returns_the_record_unchanged() {
        let dir = TempDir::new("none");
        let service = HookService::start(&dir.0, HookLimits::default());

        let record = json!({ "title": "T" });
        let produced = service
            .before(HookEvent::Create, "posts", record.clone())
            .await
            .expect("dispatch");
        assert_eq!(produced, record);
        assert!(!service.is_active());
    }

    #[tokio::test]
    async fn a_failing_hook_is_reported_through_the_channel_rather_than_hanging() {
        let dir = TempDir::new("throwing");
        dir.write(
            "main.pb.ts",
            r#"onRecordBeforeCreate("posts", () => { throw new Error("boom"); });"#,
        );

        let service = HookService::start(&dir.0, HookLimits::default());
        let mut waited = 0;
        while !service.is_active() && waited < 100 {
            tokio::time::sleep(Duration::from_millis(10)).await;
            waited += 1;
        }

        let error = service
            .before(HookEvent::Create, "posts", json!({}))
            .await
            .expect_err("a throwing hook should be an error");
        assert!(
            error.message().contains("running hooks"),
            "{}",
            error.message()
        );
    }

    #[tokio::test]
    async fn an_explicit_reload_picks_up_a_new_file() {
        let dir = TempDir::new("reload");
        let service = HookService::start(&dir.0, HookLimits::default());
        assert!(!service.is_active());

        dir.write(
            "main.pb.ts",
            r#"onRecordBeforeCreate("posts", (e) => { e.record.slug = "late"; });"#,
        );
        let report = service.reload().await.expect("reload");
        assert_eq!(report.error, None, "{report:?}");
        assert_eq!(report.hooks, 1);
        assert!(service.is_active());

        let produced = service
            .before(HookEvent::Create, "posts", json!({}))
            .await
            .expect("dispatch");
        assert_eq!(produced["slug"], json!("late"));
    }

    #[tokio::test]
    async fn after_hooks_run_through_the_same_worker() {
        let dir = TempDir::new("after");
        dir.write("main.pb.ts", r#"onRecordAfterCreate("posts", () => {});"#);
        let service = HookService::start(&dir.0, HookLimits::default());
        let mut waited = 0;
        while !service.is_active() && waited < 100 {
            tokio::time::sleep(Duration::from_millis(10)).await;
            waited += 1;
        }

        service
            .after(HookEvent::Create, "posts", json!({ "id": "p1" }))
            .await
            .expect("after");
    }

    #[tokio::test]
    async fn the_handle_is_usable_from_several_tasks() {
        // `AppState` is shared across handlers, so the handle has to be.
        let dir = TempDir::new("shared");
        dir.write(
            "main.pb.ts",
            r#"onRecordBeforeCreate("posts", (e) => { e.record.n = (e.record.n || 0) + 1; });"#,
        );
        let service = Arc::new(HookService::start(&dir.0, HookLimits::default()));
        let mut waited = 0;
        while !service.is_active() && waited < 100 {
            tokio::time::sleep(Duration::from_millis(10)).await;
            waited += 1;
        }

        let mut tasks = Vec::new();
        for _ in 0..8 {
            let service = Arc::clone(&service);
            tasks.push(tokio::spawn(async move {
                service
                    .before(HookEvent::Create, "posts", json!({}))
                    .await
                    .expect("dispatch")
            }));
        }
        for task in tasks {
            let produced = task.await.expect("task");
            assert_eq!(produced["n"], json!(1), "each call is its own event");
        }
    }

    #[test]
    fn every_write_event_maps_to_a_before_and_an_after_hook() {
        for event in [HookEvent::Create, HookEvent::Update, HookEvent::Delete] {
            let before = runtime_event(event, true);
            let after = runtime_event(event, false);
            assert!(before.is_before(), "{event:?} before");
            assert!(!after.is_before(), "{event:?} after");
            assert_ne!(before.key(), after.key());
        }
    }
}
