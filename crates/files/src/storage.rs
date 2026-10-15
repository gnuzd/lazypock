//! The storage backends and the file service.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use bytes::Bytes;
use lazypock_core::error::{Error, Result};

use crate::config::{FilesConfig, StorageAdapter};
use crate::validation::{self, ImageFormat, UploadLimits};

/// An object as it is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredObject {
    /// Content type to serve it with.
    pub content_type: String,
    /// The bytes.
    pub body: Bytes,
}

/// A boxed, `Send` future, matching the shape used elsewhere in the workspace.
pub type StorageFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

/// Where objects live.
///
/// Object safe so the backend can be swapped without the callers changing, which
/// is also how the S3 backend will slot in next to the local one.
pub trait Storage: Send + Sync {
    /// Store an object, replacing any object already at the key.
    fn put<'a>(&'a self, key: &'a str, content_type: &'a str, body: Bytes)
        -> StorageFuture<'a, ()>;

    /// Fetch an object, or `None` when there is nothing at the key.
    fn get<'a>(&'a self, key: &'a str) -> StorageFuture<'a, Option<StoredObject>>;

    /// Remove an object. A missing object is not an error, so a retried delete
    /// succeeds.
    fn delete<'a>(&'a self, key: &'a str) -> StorageFuture<'a, ()>;

    /// Whether the backend is usable.
    fn health(&self) -> StorageFuture<'_, ()>;

    /// Every key under a prefix, sorted. An empty prefix means the whole store.
    ///
    /// Used by the orphan sweeper, which is the one caller that needs to see what
    /// is actually there rather than what something asked for.
    fn list<'a>(&'a self, prefix: &'a str) -> StorageFuture<'a, Vec<String>>;

    /// A URL a client can `PUT` an object to directly, when the backend can offer one.
    ///
    /// `None` means it cannot — a local directory has no HTTP endpoint of its own — and
    /// the client uploads through the API instead. Not an error: it is the reason the
    /// SDK falls back to a plain upload.
    fn presign_put(&self, key: &str, expires_seconds: i64) -> Option<String>;
}

/// The local filesystem backend.
#[derive(Debug, Clone)]
pub struct LocalStorage {
    root: PathBuf,
}

impl LocalStorage {
    /// Store objects under `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The directory objects live under.
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    /// Resolve a key to a path, refusing anything that could escape the root.
    ///
    /// Keys are built by [`crate::keys::storage_key`], so this is belt and
    /// braces — but a path traversal here would be arbitrary file write, which is
    /// worth two checks.
    fn path_for(&self, key: &str) -> Result<PathBuf> {
        if key.is_empty() {
            return Err(Error::invalid("a storage key cannot be empty"));
        }
        let mut path = self.root.clone();
        for segment in key.split('/') {
            if segment.is_empty()
                || segment == "."
                || segment == ".."
                || segment.contains('\\')
                || segment.contains('\0')
            {
                return Err(Error::invalid(format!(
                    "refusing an unsafe storage key {key:?}"
                )));
            }
            path.push(segment);
        }
        Ok(path)
    }
}

impl Storage for LocalStorage {
    fn put<'a>(
        &'a self,
        key: &'a str,
        _content_type: &'a str,
        body: Bytes,
    ) -> StorageFuture<'a, ()> {
        Box::pin(async move {
            let path = self.path_for(key)?;
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await.map_err(|err| {
                    Error::internal(format!("cannot create {}: {err}", parent.display()))
                })?;
            }

            // Write then rename, so a reader never sees a half-written object.
            let temporary = path.with_extension("part");
            tokio::fs::write(&temporary, &body).await.map_err(|err| {
                Error::internal(format!("cannot write {}: {err}", temporary.display()))
            })?;
            tokio::fs::rename(&temporary, &path).await.map_err(|err| {
                Error::internal(format!("cannot finalise {}: {err}", path.display()))
            })?;
            Ok(())
        })
    }

    fn get<'a>(&'a self, key: &'a str) -> StorageFuture<'a, Option<StoredObject>> {
        Box::pin(async move {
            let path = self.path_for(key)?;
            match tokio::fs::read(&path).await {
                Ok(body) => Ok(Some(StoredObject {
                    // Derived from the extension, which is the only thing the
                    // local backend knows; the upload path already decided it.
                    content_type: crate::keys::content_type_for(
                        validation::sniff(&body).unwrap_or(ImageFormat::Png),
                    )
                    .to_string(),
                    body: Bytes::from(body),
                })),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(err) => Err(Error::internal(format!(
                    "cannot read {}: {err}",
                    path.display()
                ))),
            }
        })
    }

    fn delete<'a>(&'a self, key: &'a str) -> StorageFuture<'a, ()> {
        Box::pin(async move {
            let path = self.path_for(key)?;
            match tokio::fs::remove_file(&path).await {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(Error::internal(format!(
                    "cannot remove {}: {err}",
                    path.display()
                ))),
            }
        })
    }

    fn list<'a>(&'a self, prefix: &'a str) -> StorageFuture<'a, Vec<String>> {
        Box::pin(async move {
            let start = if prefix.is_empty() {
                self.root.clone()
            } else {
                self.path_for(prefix)?
            };

            let mut keys = Vec::new();
            let mut stack = vec![start];
            while let Some(directory) = stack.pop() {
                let mut entries = match tokio::fs::read_dir(&directory).await {
                    Ok(entries) => entries,
                    // Nothing stored here yet is not an error.
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(err) => {
                        return Err(Error::internal(format!(
                            "cannot read {}: {err}",
                            directory.display()
                        )))
                    }
                };

                while let Some(entry) = entries.next_entry().await.map_err(|err| {
                    Error::internal(format!("cannot read {}: {err}", directory.display()))
                })? {
                    let path = entry.path();
                    let file_type = entry.file_type().await.map_err(|err| {
                        Error::internal(format!("cannot stat {}: {err}", path.display()))
                    })?;

                    if file_type.is_dir() {
                        stack.push(path);
                    } else if file_type.is_file() {
                        if let Ok(relative) = path.strip_prefix(&self.root) {
                            if let Some(key) = relative.to_str() {
                                // Keys are always `/`-separated, whatever the
                                // platform's path separator is.
                                keys.push(key.replace('\\', "/"));
                            }
                        }
                    }
                }
            }

            keys.sort();
            Ok(keys)
        })
    }

    fn health(&self) -> StorageFuture<'_, ()> {
        Box::pin(async move {
            tokio::fs::create_dir_all(&self.root).await.map_err(|err| {
                Error::config(format!(
                    "the files directory {} is not usable: {err}",
                    self.root.display()
                ))
            })
        })
    }

    fn presign_put(&self, _key: &str, _expires_seconds: i64) -> Option<String> {
        // A directory is not an HTTP endpoint, so there is nothing to hand out.
        None
    }
}

/// The live file service.
///
/// Reconfigurable **without a restart**: a superuser saving storage settings swaps the
/// inner service, and a request already in flight finishes against the backend it started
/// with — the old one is dropped once the last holder lets go of it. That is what makes
/// "change storage from the Studio" honest instead of "takes effect after a restart".
pub struct Files {
    inner: arc_swap::ArcSwap<Inner>,
}

/// The parts that change when the configuration does.
#[derive(Clone)]
struct Inner {
    storage: Arc<dyn Storage>,
    limits: UploadLimits,
    public_base_url: Option<String>,
    image_service: Option<crate::image_service::ImageService>,
    /// `local` or `s3`, as `_files.storage_backend` records it: a key is only unique
    /// within one backend, so the reaper has to know which store it owns.
    backend: &'static str,
    /// The configuration this service was built from, so the settings API can report the
    /// configuration the server is *running* with rather than re-reading the environment.
    config: FilesConfig,
}

impl Inner {
    /// Build one from a configuration.
    ///
    /// A backend is built and validated here rather than on the first upload: a
    /// misconfigured S3 backend is refused loudly instead of quietly falling back to the
    /// local disk, which would put objects somewhere the operator is not looking.
    fn build(config: &FilesConfig) -> Result<Self> {
        let (storage, backend): (Arc<dyn Storage>, &'static str) = match config.adapter {
            StorageAdapter::Local => (Arc::new(LocalStorage::new(&config.directory)), "local"),
            StorageAdapter::S3 => (Arc::new(crate::s3::S3Storage::new(config)?), "s3"),
        };

        // Built up front, so a bad image service URL is a startup error rather than the
        // first thumbnail's problem.
        let image_service = match &config.image_service_url {
            Some(url) => Some(crate::image_service::ImageService::new(
                url.clone(),
                config.image_service_token.clone(),
            )?),
            None => None,
        };

        Ok(Self {
            storage,
            limits: UploadLimits::default(),
            public_base_url: config.public_base_url.clone(),
            image_service,
            backend,
            config: config.clone(),
        })
    }
}

impl std::fmt::Debug for Files {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.inner.load();
        f.debug_struct("Files")
            .field("backend", &inner.backend)
            .field("limits", &inner.limits)
            .field("public_base_url", &inner.public_base_url)
            // The service's URL, never its token.
            .field(
                "image_service",
                &inner.image_service.as_ref().map(|service| service.url()),
            )
            .finish_non_exhaustive()
    }
}

impl Files {
    /// Build the service from the configuration.
    pub fn from_config(config: &FilesConfig) -> Result<Self> {
        Ok(Self {
            inner: arc_swap::ArcSwap::from_pointee(Inner::build(config)?),
        })
    }

    /// Build a service from a new configuration and make it current.
    ///
    /// Built first and swapped second, so a configuration that cannot be built leaves the
    /// running service exactly as it was — a bad save must not take a working deployment
    /// down with it.
    pub fn reconfigure(&self, config: &FilesConfig) -> Result<()> {
        let inner = Inner::build(config)?;
        self.inner.store(Arc::new(inner));
        Ok(())
    }

    /// The external image service, when one is configured.
    ///
    /// `None` means resizing happens in this process — or, in a build without the
    /// `local-image-codecs` feature, that no thumbnail can be produced at all.
    pub fn image_service(&self) -> Option<crate::image_service::ImageService> {
        self.inner.load().image_service.clone()
    }

    /// The backend's name, as `_files.storage_backend` stores it.
    pub fn backend_name(&self) -> &'static str {
        self.inner.load().backend
    }

    /// The configuration this service is running with.
    pub fn config(&self) -> FilesConfig {
        self.inner.load().config.clone()
    }

    /// The backend in use **now**.
    ///
    /// A handle rather than a borrow: the service can be reconfigured at any moment, and a
    /// caller holding a borrow of it would hold the swap off. The backend it received
    /// stays alive until the caller drops it, so a request finishes where it started.
    pub fn storage(&self) -> Arc<dyn Storage> {
        Arc::clone(&self.inner.load().storage)
    }

    /// The upload rules.
    pub fn limits(&self) -> UploadLimits {
        self.inner.load().limits.clone()
    }

    /// Replace the upload rules, e.g. from a field's options.
    pub fn with_limits(self, limits: UploadLimits) -> Self {
        self.inner.rcu(|inner| {
            let mut next = Inner::clone(inner);
            next.limits = limits.clone();
            Arc::new(next)
        });
        self
    }

    /// Validate an upload and store it, returning what it turned out to be.
    pub async fn store(&self, key: &str, declared_type: &str, body: Bytes) -> Result<ImageFormat> {
        // Read the current backend and rules out of the guard, then let it go: the upload
        // is written where it started even if the settings change while it is in flight.
        let (storage, limits) = {
            let inner = self.inner.load();
            (Arc::clone(&inner.storage), inner.limits.clone())
        };

        let format = validation::validate(&body, declared_type, &limits)?;
        storage.put(key, format.content_type(), body).await?;
        Ok(format)
    }

    /// The URL a client can fetch a stored object from.
    ///
    /// With a public base URL that is the CDN; otherwise the server's own file
    /// route.
    pub fn url(&self, collection: &str, record_id: &str, filename: &str) -> String {
        let path = format!("api/files/{collection}/{record_id}/{filename}");
        match &self.inner.load().public_base_url {
            Some(base) => format!("{}/{}", base.trim_end_matches('/'), path),
            None => format!("/{path}"),
        }
    }

    /// The URL a client fetches a file from when all it holds is the file's id.
    ///
    /// The id is what a client stores in a file field, so this is the shape the API
    /// hands out — and the one an editor's markdown embeds, which is how a reference
    /// to an editor upload is found again. The public base URL applies here too, the
    /// same rule as [`Self::url`].
    pub fn file_url(&self, id: &str) -> String {
        let path = format!("api/files/{id}");
        match &self.inner.load().public_base_url {
            Some(base) => format!("{}/{}", base.trim_end_matches('/'), path),
            None => format!("/{path}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::FilesConfig;

    fn png(width: u32, height: u32) -> Bytes {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(&13u32.to_be_bytes());
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
        Bytes::from(bytes)
    }

    fn config(adapter: StorageAdapter) -> FilesConfig {
        FilesConfig {
            adapter,
            directory: "/tmp/lazypock-files-test".into(),
            public_base_url: None,
            endpoint: None,
            bucket: None,
            region: "auto".into(),
            access_key: None,
            secret_key: None,
            image_service_url: None,
            image_service_token: None,
            presign_ttl_seconds: FilesConfig::DEFAULT_PRESIGN_TTL_SECONDS,
        }
    }

    fn service(adapter: StorageAdapter) -> Result<Files> {
        Files::from_config(&config(adapter))
    }

    #[tokio::test]
    async fn the_local_backend_round_trips_an_object() {
        let directory =
            std::env::temp_dir().join(format!("lazypock-files-{}", lazypock_core::id::new_id()));
        let storage = LocalStorage::new(&directory);

        let key = "posts/abc123/photo.png";
        assert!(storage.get(key).await.expect("get").is_none());

        storage
            .put(key, "image/png", png(10, 10))
            .await
            .expect("put");

        let stored = storage.get(key).await.expect("get").expect("present");
        assert_eq!(stored.body, png(10, 10));
        assert_eq!(stored.content_type, "image/png");

        storage.delete(key).await.expect("delete");
        assert!(storage.get(key).await.expect("get").is_none());
        // Deleting twice is fine, so a retried outbox entry succeeds.
        storage.delete(key).await.expect("delete again");

        let _ = tokio::fs::remove_dir_all(&directory).await;
    }

    #[tokio::test]
    async fn the_local_backend_refuses_a_traversing_key() {
        let storage = LocalStorage::new("/tmp/lazypock-files-traversal");
        for key in [
            "../escape",
            "a/../../escape",
            "/absolute",
            "a//b",
            ".",
            "..",
            "",
        ] {
            assert!(
                storage.get(key).await.is_err()
                    || storage.put(key, "image/png", png(1, 1)).await.is_err(),
                "{key:?} should be refused"
            );
        }
    }

    #[tokio::test]
    async fn the_service_validates_before_storing() {
        let directory = std::env::temp_dir().join(format!(
            "lazypock-files-svc-{}",
            lazypock_core::id::new_id()
        ));
        let files = Files::from_config(&FilesConfig {
            directory: directory.display().to_string(),
            ..config(StorageAdapter::Local)
        })
        .expect("service");

        // A refused upload leaves nothing behind.
        assert!(files
            .store("a/b/evil.svg", "image/png", Bytes::from_static(b"<svg/>"))
            .await
            .is_err());
        assert!(files
            .storage()
            .get("a/b/evil.svg")
            .await
            .expect("get")
            .is_none());

        assert_eq!(
            files
                .store("a/b/photo.png", "image/png", png(10, 10))
                .await
                .expect("stored"),
            ImageFormat::Png
        );

        let _ = tokio::fs::remove_dir_all(&directory).await;
    }

    #[test]
    fn a_misconfigured_backend_is_refused_rather_than_fallen_back_to() {
        // An S3 adapter with no endpoint is a configuration error at boot — not a
        // silent fall back to the local disk, which would put objects somewhere
        // the operator is not looking.
        let err = service(StorageAdapter::S3).expect_err("refused");
        assert!(matches!(err, Error::Config(_)), "{err:?}");
        assert!(
            err.message().contains("LAZYPOCK_S3_ENDPOINT"),
            "the message must name the missing setting: {err}"
        );
        assert!(service(StorageAdapter::Local).is_ok());
    }

    #[test]
    fn urls_prefer_the_public_base_when_one_is_configured() {
        let files = service(StorageAdapter::Local).expect("service");
        assert_eq!(
            files.url("posts", "abc", "p.png"),
            "/api/files/posts/abc/p.png"
        );
        assert_eq!(files.file_url("f1"), "/api/files/f1");

        // A CDN base applies to both shapes of URL, so a client that is handed either one
        // is not quietly sent to the wrong host.
        let with_cdn = Files::from_config(&FilesConfig {
            public_base_url: Some("https://cdn.example.com".into()),
            ..config(StorageAdapter::Local)
        })
        .expect("service");

        assert_eq!(
            with_cdn.url("posts", "abc", "p.png"),
            "https://cdn.example.com/api/files/posts/abc/p.png"
        );
        assert_eq!(
            with_cdn.file_url("f1"),
            "https://cdn.example.com/api/files/f1"
        );
    }

    #[tokio::test]
    async fn reconfiguring_swaps_the_backend_without_a_restart() {
        let first =
            std::env::temp_dir().join(format!("lazypock-files-a-{}", lazypock_core::id::new_id()));
        let second =
            std::env::temp_dir().join(format!("lazypock-files-b-{}", lazypock_core::id::new_id()));

        let files = Files::from_config(&FilesConfig {
            directory: first.display().to_string(),
            ..config(StorageAdapter::Local)
        })
        .expect("service");

        files
            .store("posts/p1/a.png", "image/png", png(4, 4))
            .await
            .expect("stored");

        // What a settings save does: build the new service and swap it in.
        files
            .reconfigure(&FilesConfig {
                directory: second.display().to_string(),
                ..config(StorageAdapter::Local)
            })
            .expect("reconfigured");

        assert_eq!(files.config().directory, second.display().to_string());
        assert!(
            files.storage().list("").await.expect("listing").is_empty(),
            "the new directory is empty"
        );
        assert!(
            first.join("posts/p1/a.png").exists(),
            "the object stays where it was written"
        );

        // A configuration that cannot be built leaves the running service alone.
        let broken = FilesConfig {
            adapter: StorageAdapter::S3,
            ..config(StorageAdapter::S3)
        };
        assert!(files.reconfigure(&broken).is_err());
        assert_eq!(files.config().directory, second.display().to_string());

        let _ = std::fs::remove_dir_all(&first);
        let _ = std::fs::remove_dir_all(&second);
    }
}
