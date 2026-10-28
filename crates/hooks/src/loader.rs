//! Finding, loading and reloading hook files.
//!
//! Hot reload keeps the **last known good** runtime: a file with a syntax error is
//! reported and skipped, and the version that was already serving keeps serving.
//! An edit in progress must never take down a working installation.
//!
//! Files are re-read only when their contents change, and the signature is a hash
//! of the bytes rather than a modification time. A timestamp has coarse
//! granularity on some filesystems, which would make a reload miss an edit — and a
//! test asserting on it would pass for the wrong reason.

use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::Hasher;
use std::path::{Path, PathBuf};

use lazypock_core::error::Result;
use serde_json::Value;

#[cfg(test)]
use crate::runtime::HookLimits;
use crate::runtime::{HookRuntime, HookSetup, RecordEvent};
use crate::strip::strip_types;
use lazypock_db::hooks::HookContext;

/// The suffix a hook file has.
pub const HOOK_SUFFIX: &str = ".pb.ts";

/// The suffix a plain JavaScript hook file has, which needs no stripping.
pub const JS_HOOK_SUFFIX: &str = ".pb.js";

/// What a load or reload did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LoadReport {
    /// Hook files found.
    pub files: usize,
    /// Handlers registered in total.
    pub hooks: usize,
    /// A file that failed, if any. The previous runtime kept serving.
    pub error: Option<String>,
}

/// A hook file and a hash of its contents.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FileSignature {
    path: PathBuf,
    hash: u64,
}

/// Loads hooks from a directory and keeps them current.
#[derive(Debug)]
pub struct HookLoader {
    directory: PathBuf,
    setup: HookSetup,
    /// The runtime currently serving, if hooks have ever loaded cleanly.
    runtime: Option<HookRuntime>,
    /// When the loaded runtime was actually replaced.
    signature: Vec<FileSignature>,
    /// The most recent failure, kept so it can be surfaced rather than lost.
    last_error: Option<String>,
}

impl HookLoader {
    /// Point a loader at a directory. Nothing is read until `load`.
    ///
    /// A [`crate::HookLimits`] on its own is accepted: it builds a runtime whose
    /// `$http` is refused, which is the right default for a caller that has not
    /// said which hosts, if any, hooks may reach.
    pub fn new(directory: impl Into<PathBuf>, setup: impl Into<HookSetup>) -> Self {
        Self {
            directory: directory.into(),
            setup: setup.into(),
            runtime: None,
            signature: Vec::new(),
            last_error: None,
        }
    }

    /// The directory being watched.
    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Whether any hook file exists.
    ///
    /// Used to avoid creating a runtime at all: an installation with no hooks
    /// should not pay for the capability (`PLAN.md` section 11.1).
    pub fn has_hooks(&self) -> bool {
        !self.discover().is_empty()
    }

    /// Whether a runtime is serving.
    pub fn is_active(&self) -> bool {
        self.runtime.is_some()
    }

    /// The most recent load failure, if the last load did not fully succeed.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    /// Read and load every hook file, replacing the runtime.
    ///
    /// On any failure the previous runtime is kept, so a broken edit leaves the
    /// installation as it was.
    pub fn load(&mut self) -> LoadReport {
        let files = self.discover();
        let mut report = LoadReport {
            files: files.len(),
            ..LoadReport::default()
        };

        if files.is_empty() {
            // No files: drop any runtime, rather than leaving a stale one serving.
            self.runtime = None;
            self.signature.clear();
            self.last_error = None;
            return report;
        }

        let runtime = match HookRuntime::new(self.setup.clone()) {
            Ok(runtime) => runtime,
            Err(err) => {
                let message = err.message().to_string();
                report.error = Some(message.clone());
                self.last_error = Some(message);
                return report;
            }
        };

        let mut signature = Vec::new();
        for path in &files {
            let source = match fs::read_to_string(path) {
                Ok(source) => source,
                Err(err) => {
                    report.error = Some(format!("cannot read {}: {err}", path.display()));
                    break;
                }
            };
            signature.push(FileSignature {
                path: path.clone(),
                hash: hash_of(source.as_bytes()),
            });

            // TypeScript is stripped; plain JavaScript is loaded as it is.
            let code = if path.to_string_lossy().ends_with(JS_HOOK_SUFFIX) {
                source
            } else {
                match strip_types(&source, &path.to_string_lossy()) {
                    Ok(code) => code,
                    Err(err) => {
                        report.error = Some(err.message().to_string());
                        break;
                    }
                }
            };

            if let Err(err) = runtime.load(&code) {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_default();
                report.error = Some(format!("{name}: {}", err.message()));
                break;
            }
        }

        if let Some(error) = report.error.clone() {
            // Keep whatever was already serving. Reporting the failure is what
            // makes it actionable; replacing a working runtime with a broken one
            // would make it an outage.
            self.last_error = Some(error);
            return report;
        }

        for event in RecordEvent::all() {
            for collection in collections_in(&files) {
                if let Ok(count) = runtime.registered(event, &collection) {
                    report.hooks += count;
                }
            }
        }

        self.runtime = Some(runtime);
        self.signature = signature;
        self.last_error = None;
        report
    }

    /// Reload only when a file's contents have changed.
    ///
    /// Returns `None` when nothing changed, so a caller polling this can tell the
    /// difference between "checked" and "reloaded".
    pub fn reload_if_changed(&mut self) -> Option<LoadReport> {
        let files = self.discover();
        let current: Vec<FileSignature> = files
            .iter()
            .filter_map(|path| {
                fs::read(path).ok().map(|bytes| FileSignature {
                    path: path.clone(),
                    hash: hash_of(&bytes),
                })
            })
            .collect();

        if current == self.signature {
            return None;
        }

        tracing::info!(files = files.len(), "reloading hooks");
        Some(self.load())
    }

    /// Run the hooks for an event.
    ///
    /// `Ok(None)` means no runtime is serving, which is the ordinary state of an
    /// installation with no hooks.
    pub fn dispatch(
        &self,
        event: RecordEvent,
        collection: &str,
        payload: &Value,
        context: &HookContext,
    ) -> Result<Option<Value>> {
        let Some(runtime) = &self.runtime else {
            return Ok(None);
        };
        runtime
            .dispatch(event, collection, payload, context)
            .map(Some)
    }

    /// How many handlers are registered for an event and collection.
    pub fn registered(&self, event: RecordEvent, collection: &str) -> usize {
        self.runtime
            .as_ref()
            .and_then(|runtime| runtime.registered(event, collection).ok())
            .unwrap_or(0)
    }

    /// Every hook file in the directory, sorted.
    fn discover(&self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(&self.directory) else {
            // A missing directory is an installation with no hooks, not an error.
            return Vec::new();
        };

        let mut files: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| {
                path.is_file()
                    && (path.to_string_lossy().ends_with(HOOK_SUFFIX)
                        || path.to_string_lossy().ends_with(JS_HOOK_SUFFIX))
            })
            .collect();
        files.sort();
        files
    }
}

/// A hash of a file's contents.
fn hash_of(bytes: &[u8]) -> u64 {
    let mut hasher = DefaultHasher::new();
    hasher.write(bytes);
    hasher.finish()
}

/// The collection names that appear in the loaded hook sources.
///
/// Used only to report how many handlers were registered; the runner is asked once
/// per collection it might have registered against.
fn collections_in(files: &[PathBuf]) -> Vec<String> {
    let mut names = Vec::new();
    for path in files {
        let Ok(source) = fs::read_to_string(path) else {
            continue;
        };
        for name in collection_names(&source) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

/// The first string argument of each `onRecord*` call.
fn collection_names(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = source;
    while let Some(index) = rest.find("onRecord") {
        rest = &rest[index..];
        // The collection name is the first quoted string after the `(`.
        if let Some(open) = rest.find('(') {
            let after = &rest[open + 1..];
            let trimmed = after.trim_start();
            let quote = trimmed.chars().next();
            if let Some(quote @ ('"' | '\'' | '`')) = quote {
                let body = &trimmed[quote.len_utf8()..];
                if let Some(end) = body.find(quote) {
                    names.push(body[..end].to_string());
                }
            }
        }
        rest = &rest[1..];
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A directory of its own, removed when the test ends.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(label: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("lazypock-hooks-{label}-{}", std::process::id()));
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

    fn payload() -> Value {
        json!({ "collection": "posts", "record": { "title": "Hello" } })
    }

    #[test]
    fn a_directory_with_no_hook_files_loads_nothing() {
        let dir = TempDir::new("empty");
        let mut loader = HookLoader::new(&dir.0, HookLimits::default());

        assert!(!loader.has_hooks());
        let report = loader.load();
        assert_eq!(report, LoadReport::default());
        assert!(!loader.is_active(), "no files means no runtime at all");
        // ... and dispatch is a no-op rather than an error.
        assert!(loader
            .dispatch(
                RecordEvent::BeforeCreate,
                "posts",
                &payload(),
                &HookContext::default(),
            )
            .expect("dispatch")
            .is_none());
    }

    #[test]
    fn a_typescript_hook_file_is_stripped_and_loaded() {
        let dir = TempDir::new("ts");
        dir.write(
            "main.pb.ts",
            r#"
            interface Options { prefix: string }
            const options: Options = { prefix: "slug" };
            onRecordBeforeCreate("posts", (e: any) => {
                e.record.slug = `${options.prefix}-${e.record.title}`;
            });
            "#,
        );

        let mut loader = HookLoader::new(&dir.0, HookLimits::default());
        assert!(loader.has_hooks());
        let report = loader.load();
        assert_eq!(report.error, None, "{report:?}");
        assert_eq!(report.files, 1);
        assert!(loader.is_active());
        assert_eq!(loader.registered(RecordEvent::BeforeCreate, "posts"), 1);

        let produced = loader
            .dispatch(
                RecordEvent::BeforeCreate,
                "posts",
                &payload(),
                &HookContext::default(),
            )
            .expect("dispatch")
            .expect("a runtime");
        assert_eq!(produced["record"]["slug"], json!("slug-Hello"));
    }

    #[test]
    fn a_plain_javascript_hook_file_needs_no_stripping() {
        let dir = TempDir::new("js");
        dir.write(
            "main.pb.js",
            r#"onRecordAfterCreate("posts", (e) => { e.record.seen = true; });"#,
        );

        let mut loader = HookLoader::new(&dir.0, HookLimits::default());
        let report = loader.load();
        assert_eq!(report.error, None, "{report:?}");
        assert_eq!(loader.registered(RecordEvent::AfterCreate, "posts"), 1);
    }

    #[test]
    fn a_file_that_does_not_compile_keeps_the_previous_runtime_serving() {
        let dir = TempDir::new("broken");
        dir.write(
            "main.pb.ts",
            r#"onRecordBeforeCreate("posts", (e) => { e.record.slug = "good"; });"#,
        );

        let mut loader = HookLoader::new(&dir.0, HookLimits::default());
        assert_eq!(loader.load().error, None, "the first load should work");
        assert_eq!(loader.registered(RecordEvent::BeforeCreate, "posts"), 1);

        // A broken edit must not take the working version down.
        dir.write("main.pb.ts", r#"onRecordBeforeCreate("posts", (e) => {"#);
        let report = loader.load();
        assert!(report.error.is_some(), "the failure should be reported");
        assert!(
            loader.is_active(),
            "the previous runtime should still serve"
        );
        assert_eq!(
            loader.registered(RecordEvent::BeforeCreate, "posts"),
            1,
            "the last good hooks should still be registered"
        );
        assert!(loader.last_error().is_some());

        let produced = loader
            .dispatch(
                RecordEvent::BeforeCreate,
                "posts",
                &payload(),
                &HookContext::default(),
            )
            .expect("dispatch")
            .expect("a runtime");
        assert_eq!(produced["record"]["slug"], json!("good"));
    }

    #[test]
    fn a_reload_only_happens_when_a_file_actually_changes() {
        let dir = TempDir::new("reload");
        dir.write(
            "main.pb.ts",
            r#"onRecordBeforeCreate("posts", (e) => { e.record.v = 1; });"#,
        );

        let mut loader = HookLoader::new(&dir.0, HookLimits::default());
        loader.load();

        // Nothing changed: no reload, which is what makes polling cheap.
        assert!(loader.reload_if_changed().is_none());

        dir.write(
            "main.pb.ts",
            r#"onRecordBeforeCreate("posts", (e) => { e.record.v = 2; });"#,
        );
        let report = loader.reload_if_changed().expect("a reload");
        assert_eq!(report.error, None, "{report:?}");

        let produced = loader
            .dispatch(
                RecordEvent::BeforeCreate,
                "posts",
                &payload(),
                &HookContext::default(),
            )
            .expect("dispatch")
            .expect("a runtime");
        assert_eq!(
            produced["record"]["v"],
            json!(2),
            "the edit should take effect"
        );
    }

    #[test]
    fn deleting_every_hook_file_stops_the_runtime() {
        let dir = TempDir::new("delete");
        dir.write(
            "main.pb.ts",
            r#"onRecordBeforeCreate("posts", (e) => { e.record.v = 1; });"#,
        );

        let mut loader = HookLoader::new(&dir.0, HookLimits::default());
        loader.load();
        assert!(loader.is_active());

        fs::remove_file(dir.0.join("main.pb.ts")).expect("remove");
        loader.reload_if_changed().expect("a reload");

        assert!(!loader.is_active(), "no files means no runtime");
        assert!(loader
            .dispatch(
                RecordEvent::BeforeCreate,
                "posts",
                &payload(),
                &HookContext::default(),
            )
            .expect("dispatch")
            .is_none());
    }

    #[test]
    fn a_missing_directory_is_an_installation_without_hooks() {
        let path = std::env::temp_dir().join("lazypock-hooks-does-not-exist");
        let mut loader = HookLoader::new(&path, HookLimits::default());
        assert!(!loader.has_hooks());
        let report = loader.load();
        assert_eq!(report.files, 0);
        assert_eq!(report.error, None, "a missing directory is not a failure");
    }

    #[test]
    fn several_files_load_together_and_the_counts_add_up() {
        let dir = TempDir::new("many");
        dir.write(
            "a.pb.ts",
            r#"
            onRecordBeforeCreate("posts", () => {});
            onRecordAfterCreate("posts", () => {});
            "#,
        );
        dir.write("b.pb.js", r#"onRecordBeforeCreate("posts", () => {});"#);

        let mut loader = HookLoader::new(&dir.0, HookLimits::default());
        let report = loader.load();
        assert_eq!(report.error, None, "{report:?}");
        assert_eq!(report.files, 2);
        assert_eq!(report.hooks, 3, "the total across both files");
        assert_eq!(loader.registered(RecordEvent::BeforeCreate, "posts"), 2);
    }

    #[test]
    fn collection_names_are_read_from_the_source() {
        let names = collection_names(
            r#"onRecordBeforeCreate("posts", () => {}); onRecordAfterUpdate('orders', () => {});"#,
        );
        assert_eq!(names, vec!["posts".to_string(), "orders".to_string()]);
        assert!(collection_names("const x = 1;").is_empty());
    }
}
