//! Where uploads go, from the environment.
//!
//! Configuration is environment-only on purpose: the storage secret is a
//! credential, and keeping it out of the database means it cannot leak through a
//! backup, a replica or the SQL console. The same reasoning is why the
//! access-token signing key is tracked as a gap rather than stored here.

use lazypock_core::error::{Error, Result};
use serde_json::Value;

use crate::secrets::Secrets;

/// Which backend stores objects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageAdapter {
    /// The local filesystem, under `LAZYPOCK_FILES_DIR`.
    Local,
    /// An S3-compatible bucket.
    S3,
}

impl StorageAdapter {
    /// Parse the configured adapter, refusing anything unknown.
    pub fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "local" | "fs" => Ok(Self::Local),
            "s3" | "r2" => Ok(Self::S3),
            other => Err(Error::config(format!(
                "LAZYPOCK_FILES_ADAPTER must be `local` or `s3`, got {other:?}"
            ))),
        }
    }
}

/// Everything the file layer needs to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesConfig {
    /// The chosen backend.
    pub adapter: StorageAdapter,
    /// Root directory for the local backend.
    pub directory: String,
    /// Public base URL, when objects are served through a CDN.
    pub public_base_url: Option<String>,
    /// S3 endpoint, e.g. `https://<account>.r2.cloudflarestorage.com`.
    pub endpoint: Option<String>,
    /// S3 bucket.
    pub bucket: Option<String>,
    /// S3 region; `auto` is what R2 wants.
    pub region: String,
    /// S3 access key id.
    pub access_key: Option<String>,
    /// S3 secret.
    pub secret_key: Option<String>,
    /// An external image service to resize in, instead of this process.
    ///
    /// Empty (the default) means "resize here": the `image` codecs run inside the
    /// API process. Setting it moves that work — and its peak memory — to the
    /// service; see [`crate::image_service`] for the contract.
    pub image_service_url: Option<String>,
    /// Shared secret sent to the image service as a bearer token.
    pub image_service_token: Option<String>,
    /// How long a presigned URL stays valid, in seconds.
    ///
    /// Short by default: the URL authorizes a `PUT` to one key, so the window in which a
    /// leaked one is useful should be small. Fifteen minutes is enough for a slow
    /// connection to start an upload.
    pub presign_ttl_seconds: i64,
    /// A namespace every object key is written under, `/`-separated.
    ///
    /// Empty means "the root of the store". Setting it lets several deployments —
    /// or several tenants — share one bucket without sharing a key space, which is
    /// what makes the orphan sweeper safe: it lists its own prefix instead of the
    /// whole bucket.
    pub prefix: String,
    /// Refuse the local adapter outright.
    ///
    /// A deployment whose files must live in object storage (a multi-tenant host, a
    /// container with no persistent disk) cannot rely on the environment alone:
    /// a saved Studio setting overrides it. This makes the requirement a boot
    /// failure instead of a suggestion.
    pub force_remote: bool,
}

impl FilesConfig {
    /// Default local directory.
    pub const DEFAULT_DIRECTORY: &'static str = "data/files";
    /// Default region, which is what Cloudflare R2 expects.
    pub const DEFAULT_REGION: &'static str = "auto";
    /// Default lifetime of a presigned URL.
    pub const DEFAULT_PRESIGN_TTL_SECONDS: i64 = 15 * 60;

    /// A configuration with every value at its default.
    ///
    /// Exists so a caller that only cares about one or two fields — a test, a
    /// probe — does not have to restate the rest, and so adding a field does not
    /// break every literal in the tree.
    pub fn default_config() -> Self {
        Self {
            adapter: StorageAdapter::Local,
            directory: Self::DEFAULT_DIRECTORY.to_string(),
            public_base_url: None,
            endpoint: None,
            bucket: None,
            region: Self::DEFAULT_REGION.to_string(),
            access_key: None,
            secret_key: None,
            image_service_url: None,
            image_service_token: None,
            presign_ttl_seconds: Self::DEFAULT_PRESIGN_TTL_SECONDS,
            prefix: String::new(),
            force_remote: false,
        }
    }

    /// Normalize and check a key prefix.
    ///
    /// Leading and trailing slashes are dropped (a prefix is a path, and the
    /// separator is added when a key is built), every segment must be non-empty and
    /// not `.`/`..`, and only key-safe characters are allowed — the same reasoning
    /// as [`crate::keys::safe_filename`], because this value ends up in object keys.
    fn normalize_prefix(raw: &str) -> Result<String> {
        let trimmed = raw.trim().trim_matches('/');
        if trimmed.is_empty() {
            return Ok(String::new());
        }

        let mut segments = Vec::new();
        for segment in trimmed.split('/') {
            if segment.is_empty() || segment == "." || segment == ".." {
                return Err(Error::config(format!(
                    "LAZYPOCK_FILES_PREFIX must be a plain path prefix, got {raw:?}"
                )));
            }
            if !segment.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
            }) {
                return Err(Error::config(format!(
                    "LAZYPOCK_FILES_PREFIX may only contain letters, digits, `-`, `_`, `.` and \
                     `/`, got {raw:?}"
                )));
            }
            segments.push(segment);
        }

        Ok(segments.join("/"))
    }

    /// Read the configuration from the environment.
    pub fn from_env() -> Result<Self> {
        Self::from_source(|key| std::env::var(key).ok())
    }

    /// Read the configuration from an arbitrary source.
    pub fn from_source<F>(get: F) -> Result<Self>
    where
        F: Fn(&str) -> Option<String>,
    {
        let optional = |key: &str| {
            get(key)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };

        let adapter = match optional("LAZYPOCK_FILES_ADAPTER") {
            Some(raw) => StorageAdapter::parse(&raw)?,
            None => StorageAdapter::Local,
        };

        let config = Self {
            adapter,
            directory: optional("LAZYPOCK_FILES_DIR")
                .unwrap_or_else(|| Self::DEFAULT_DIRECTORY.to_string()),
            public_base_url: optional("LAZYPOCK_FILES_PUBLIC_URL"),
            endpoint: optional("LAZYPOCK_S3_ENDPOINT"),
            bucket: optional("LAZYPOCK_S3_BUCKET"),
            region: optional("LAZYPOCK_S3_REGION")
                .unwrap_or_else(|| Self::DEFAULT_REGION.to_string()),
            access_key: optional("LAZYPOCK_S3_ACCESS_KEY"),
            secret_key: optional("LAZYPOCK_S3_SECRET_KEY"),
            image_service_url: optional("LAZYPOCK_IMAGE_SERVICE_URL"),
            image_service_token: optional("LAZYPOCK_IMAGE_SERVICE_TOKEN"),
            presign_ttl_seconds: match optional("LAZYPOCK_PRESIGN_TTL") {
                Some(raw) => raw.parse::<i64>().map_err(|_| {
                    Error::config(format!(
                        "LAZYPOCK_PRESIGN_TTL must be a whole number of seconds, got {raw:?}"
                    ))
                })?,
                None => Self::DEFAULT_PRESIGN_TTL_SECONDS,
            },
            prefix: Self::normalize_prefix(
                optional("LAZYPOCK_FILES_PREFIX")
                    .as_deref()
                    .unwrap_or_default(),
            )?,
            force_remote: match optional("LAZYPOCK_FILES_FORCE_REMOTE") {
                None => false,
                Some(raw) => match raw.to_ascii_lowercase().as_str() {
                    "1" | "on" | "true" | "yes" => true,
                    "0" | "off" | "false" | "no" => false,
                    other => {
                        return Err(Error::config(format!(
                            "LAZYPOCK_FILES_FORCE_REMOTE must be on or off, got {other:?}"
                        )))
                    }
                },
            },
        };

        config.validate()?;

        Ok(config)
    }

    /// The fields the Studio's Files Storage page can save, and the environment variable
    /// each of them falls back to.
    ///
    /// One table, so the view, the merge and the page's "set by LAZYPOCK_…" hints cannot
    /// disagree about which variable owns which field.
    pub const STORAGE_FIELDS: [(&'static str, &'static str); 7] = [
        ("backend", "LAZYPOCK_FILES_ADAPTER"),
        ("endpoint", "LAZYPOCK_S3_ENDPOINT"),
        ("bucket", "LAZYPOCK_S3_BUCKET"),
        ("region", "LAZYPOCK_S3_REGION"),
        ("access_key_id", "LAZYPOCK_S3_ACCESS_KEY"),
        ("secret_access_key", "LAZYPOCK_S3_SECRET_KEY"),
        ("public_base_url", "LAZYPOCK_FILES_PUBLIC_URL"),
    ];

    /// Refuse a configuration that cannot work, at boot and again at save time.
    ///
    /// An incomplete S3 configuration is named here rather than failing on the first
    /// upload.
    pub fn validate(&self) -> Result<()> {
        if let Some(url) = &self.public_base_url {
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(Error::config(format!(
                    "LAZYPOCK_FILES_PUBLIC_URL must start with http:// or https://, got {url:?}"
                )));
            }
        }

        if let Some(url) = &self.image_service_url {
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(Error::config(format!(
                    "LAZYPOCK_IMAGE_SERVICE_URL must start with http:// or https://, got {url:?}"
                )));
            }
        } else if self.image_service_token.is_some() {
            return Err(Error::config(
                "LAZYPOCK_IMAGE_SERVICE_TOKEN is set but LAZYPOCK_IMAGE_SERVICE_URL is not: \
                 the token would never be sent",
            ));
        }

        if !(1..=7 * 24 * 60 * 60).contains(&self.presign_ttl_seconds) {
            return Err(Error::config(format!(
                "a presigned URL lifetime must be between 1 second and 7 days, got {}",
                self.presign_ttl_seconds
            )));
        }

        // Stored already normalized, so a literal that is not is a caller's mistake
        // rather than something to silently repair here.
        if Self::normalize_prefix(&self.prefix)? != self.prefix {
            return Err(Error::config(format!(
                "LAZYPOCK_FILES_PREFIX must be stored normalized (no leading, trailing or \
                 repeated slashes), got {:?}",
                self.prefix
            )));
        }

        if self.force_remote && self.adapter == StorageAdapter::Local {
            return Err(Error::config(
                "LAZYPOCK_FILES_FORCE_REMOTE is on, so the local storage adapter is refused: \
                 set LAZYPOCK_FILES_ADAPTER=s3 with the LAZYPOCK_S3_* credentials, or turn the \
                 requirement off",
            ));
        }

        if self.adapter == StorageAdapter::S3 {
            for (key, value) in [
                ("LAZYPOCK_S3_ENDPOINT", &self.endpoint),
                ("LAZYPOCK_S3_BUCKET", &self.bucket),
                ("LAZYPOCK_S3_ACCESS_KEY", &self.access_key),
                ("LAZYPOCK_S3_SECRET_KEY", &self.secret_key),
            ] {
                if value.is_none() {
                    return Err(Error::config(format!(
                        "{key} is required when LAZYPOCK_FILES_ADAPTER=s3"
                    )));
                }
            }
        }

        Ok(())
    }

    /// Apply a saved `storage` setting over this configuration.
    ///
    /// The environment supplies the defaults and **a saved setting wins**, which is what
    /// lets a superuser change storage from the Studio without a rebuild while a
    /// deployment configured by environment keeps behaving exactly as it did. A field the
    /// setting does not carry is left alone.
    ///
    /// A stored secret needs `secrets` to read it. Without a key that is an **error**
    /// rather than a fallback to the environment's value: silently using a different
    /// credential than the one that was saved hides a rotated key behind a working-looking
    /// deployment.
    pub fn apply_storage_setting(
        &mut self,
        setting: &Value,
        secrets: Option<&Secrets>,
    ) -> Result<()> {
        let saved = |field: &str| {
            setting
                .get(field)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        };

        if let Some(backend) = saved("backend") {
            self.adapter = StorageAdapter::parse(&backend)?;
        }
        if let Some(endpoint) = saved("endpoint") {
            self.endpoint = Some(endpoint);
        }
        if let Some(bucket) = saved("bucket") {
            self.bucket = Some(bucket);
        }
        if let Some(region) = saved("region") {
            self.region = region;
        }
        if let Some(access_key) = saved("access_key_id") {
            self.access_key = Some(access_key);
        }
        if let Some(public_base_url) = saved("public_base_url") {
            self.public_base_url = Some(public_base_url);
        }
        if let Some(ttl) = setting
            .get("presign_ttl")
            .and_then(Value::as_i64)
            .filter(|ttl| *ttl > 0)
        {
            self.presign_ttl_seconds = ttl;
        }

        if let Some(stored) = saved("secret_access_key") {
            let secrets = secrets.ok_or_else(|| {
                Error::config(format!(
                    "a storage secret is saved in the database but {} is not set, so it cannot \
                     be read: set it to the key the secret was saved with",
                    crate::secrets::KEY_VARIABLE
                ))
            })?;
            self.secret_key = Some(secrets.decrypt("secret_access_key", &stored)?);
        }

        self.validate()
    }

    /// The fields whose value still comes from the environment.
    ///
    /// What the Studio's page locks: a field the environment set and the setting does not
    /// override is not editable, because saving it would look like it worked and change
    /// nothing. Saving a field moves it out of this list.
    pub fn storage_env_fields(setting: &Value) -> Vec<&'static str> {
        Self::STORAGE_FIELDS
            .iter()
            .filter(|(field, variable)| {
                let overridden = setting
                    .get(*field)
                    .and_then(Value::as_str)
                    .is_some_and(|value| !value.trim().is_empty());
                let set_in_env =
                    std::env::var(variable).is_ok_and(|value| !value.trim().is_empty());
                !overridden && set_in_env
            })
            .map(|(field, _)| *field)
            .collect()
    }

    /// A summary with the secret redacted, for logs and the CLI.
    pub fn summary(&self) -> Vec<(&'static str, String)> {
        vec![
            ("files_adapter", self.adapter_str().to_string()),
            ("files_directory", self.directory.clone()),
            ("files_public_url", self.public_url_or_unset()),
            ("s3_endpoint", self.or_unset(&self.endpoint)),
            ("s3_bucket", self.or_unset(&self.bucket)),
            ("s3_region", self.region.clone()),
            ("s3_access_key", self.or_unset(&self.access_key)),
            // Never the secret itself.
            (
                "s3_secret_key",
                if self.secret_key.is_some() {
                    "(set)".to_string()
                } else {
                    "(unset)".to_string()
                },
            ),
            // The URL, never the token: it is a credential too.
            ("image_service", self.or_unset(&self.image_service_url)),
            (
                "image_service_token",
                if self.image_service_token.is_some() {
                    "(set)".to_string()
                } else {
                    "(unset)".to_string()
                },
            ),
        ]
    }

    /// The backend's name, as `_files.storage_backend` and the settings API use it.
    pub fn adapter_str(&self) -> &'static str {
        match self.adapter {
            StorageAdapter::Local => "local",
            StorageAdapter::S3 => "s3",
        }
    }

    fn public_url_or_unset(&self) -> String {
        self.or_unset(&self.public_base_url)
    }

    fn or_unset(&self, value: &Option<String>) -> String {
        value.clone().unwrap_or_else(|| "(unset)".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(pairs: &[(&str, &str)]) -> Result<FilesConfig> {
        FilesConfig::from_source(|key| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_string())
        })
    }

    #[test]
    fn the_default_is_local_storage() {
        let config = config(&[]).expect("defaults");
        assert_eq!(config.adapter, StorageAdapter::Local);
        assert_eq!(config.directory, FilesConfig::DEFAULT_DIRECTORY);
        assert_eq!(config.region, FilesConfig::DEFAULT_REGION);
        assert!(config.public_base_url.is_none());
        // Resizing happens in this process until a service is configured.
        assert!(config.image_service_url.is_none());
        // No namespace and no forced backend: a single-deployment node behaves
        // exactly as it did before either variable existed.
        assert!(config.prefix.is_empty());
        assert!(!config.force_remote);
    }

    #[test]
    fn a_prefix_is_normalized_and_kept() {
        let config = config(&[("LAZYPOCK_FILES_PREFIX", "  /tenants/abc/  ")]).expect("prefix");
        assert_eq!(config.prefix, "tenants/abc");
    }

    #[test]
    fn a_prefix_that_is_not_a_plain_path_is_refused() {
        for bad in ["../escape", "a//b", "a/./b", "a b", "a/../b"] {
            let err = config(&[("LAZYPOCK_FILES_PREFIX", bad)]).expect_err("refused");
            assert!(
                err.message().contains("LAZYPOCK_FILES_PREFIX"),
                "{bad:?}: {err}"
            );
        }
    }

    #[test]
    fn forcing_remote_storage_refuses_the_local_adapter() {
        // The default adapter is local, so asking for remote-only already refuses.
        let err = config(&[("LAZYPOCK_FILES_FORCE_REMOTE", "on")]).expect_err("refused");
        assert!(err.message().contains("FORCE_REMOTE"), "{err}");

        // Naming it explicitly does not get around the requirement.
        let err = config(&[
            ("LAZYPOCK_FILES_FORCE_REMOTE", "on"),
            ("LAZYPOCK_FILES_ADAPTER", "local"),
        ])
        .expect_err("refused");
        assert!(err.message().contains("FORCE_REMOTE"), "{err}");
    }

    #[test]
    fn forcing_remote_storage_accepts_a_complete_s3_configuration() {
        let config = config(&[
            ("LAZYPOCK_FILES_FORCE_REMOTE", "on"),
            ("LAZYPOCK_FILES_ADAPTER", "s3"),
            (
                "LAZYPOCK_S3_ENDPOINT",
                "https://account.r2.cloudflarestorage.com",
            ),
            ("LAZYPOCK_S3_BUCKET", "media"),
            ("LAZYPOCK_S3_ACCESS_KEY", "AKIDEXAMPLE"),
            ("LAZYPOCK_S3_SECRET_KEY", "secret"),
        ])
        .expect("s3 with forcing");
        assert!(config.force_remote);
    }

    #[test]
    fn a_force_remote_value_that_is_not_a_switch_is_refused() {
        let err = config(&[("LAZYPOCK_FILES_FORCE_REMOTE", "maybe")]).expect_err("refused");
        assert!(err.message().contains("FORCE_REMOTE"), "{err}");
    }

    #[test]
    fn the_image_service_url_must_be_http() {
        let err = config(&[("LAZYPOCK_IMAGE_SERVICE_URL", "ftp://images.example.com")])
            .expect_err("refused");
        assert!(
            err.message().contains("LAZYPOCK_IMAGE_SERVICE_URL"),
            "{err}"
        );
    }

    #[test]
    fn a_token_without_a_service_is_refused() {
        // Otherwise it reads as "the service is configured and authenticated" while
        // every thumbnail is still produced here.
        let err = config(&[("LAZYPOCK_IMAGE_SERVICE_TOKEN", "s3cret")]).expect_err("refused");
        assert!(
            err.message().contains("LAZYPOCK_IMAGE_SERVICE_URL"),
            "{err}"
        );
    }

    #[test]
    fn the_image_service_is_summarised_without_its_token() {
        let config = config(&[
            (
                "LAZYPOCK_IMAGE_SERVICE_URL",
                "https://images.example.com/resize",
            ),
            ("LAZYPOCK_IMAGE_SERVICE_TOKEN", "s3cret"),
        ])
        .expect("configured");

        let summary = config.summary();
        let value = |key: &str| {
            summary
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| value.clone())
                .unwrap_or_default()
        };
        assert_eq!(value("image_service"), "https://images.example.com/resize");
        assert_eq!(value("image_service_token"), "(set)");
        assert!(
            !summary.iter().any(|(_, value)| value.contains("s3cret")),
            "{summary:?}"
        );
    }

    #[test]
    fn an_unknown_adapter_is_refused() {
        let err = config(&[("LAZYPOCK_FILES_ADAPTER", "gcs")]).expect_err("refused");
        assert!(err.message().contains("LAZYPOCK_FILES_ADAPTER"), "{err}");
    }

    #[test]
    fn an_incomplete_s3_configuration_is_refused_at_boot() {
        // Named in the error, so the operator knows what to add.
        for key in [
            "LAZYPOCK_S3_ENDPOINT",
            "LAZYPOCK_S3_BUCKET",
            "LAZYPOCK_S3_ACCESS_KEY",
            "LAZYPOCK_S3_SECRET_KEY",
        ] {
            let mut pairs = vec![
                ("LAZYPOCK_FILES_ADAPTER", "s3"),
                (
                    "LAZYPOCK_S3_ENDPOINT",
                    "https://example.r2.cloudflarestorage.com",
                ),
                ("LAZYPOCK_S3_BUCKET", "lazypock"),
                ("LAZYPOCK_S3_ACCESS_KEY", "key"),
                ("LAZYPOCK_S3_SECRET_KEY", "secret"),
            ];
            pairs.retain(|(name, _)| *name != key);
            let err = config(&pairs).unwrap_err();
            assert!(err.message().contains(key), "{key}: {err}");
        }

        // All four together are accepted.
        let complete = config(&[
            ("LAZYPOCK_FILES_ADAPTER", "s3"),
            (
                "LAZYPOCK_S3_ENDPOINT",
                "https://example.r2.cloudflarestorage.com",
            ),
            ("LAZYPOCK_S3_BUCKET", "lazypock"),
            ("LAZYPOCK_S3_ACCESS_KEY", "key"),
            ("LAZYPOCK_S3_SECRET_KEY", "secret"),
        ]);
        assert!(complete.is_ok());
    }

    #[test]
    fn a_bad_public_url_is_refused() {
        let err = config(&[("LAZYPOCK_FILES_PUBLIC_URL", "cdn.example.com")]).unwrap_err();
        assert!(err.message().contains("http"), "{err}");
    }

    #[test]
    fn the_summary_never_carries_the_secret() {
        let config = config(&[
            ("LAZYPOCK_FILES_ADAPTER", "s3"),
            (
                "LAZYPOCK_S3_ENDPOINT",
                "https://example.r2.cloudflarestorage.com",
            ),
            ("LAZYPOCK_S3_BUCKET", "lazypock"),
            ("LAZYPOCK_S3_ACCESS_KEY", "AKIAEXAMPLE"),
            ("LAZYPOCK_S3_SECRET_KEY", "super-secret-value"),
        ])
        .unwrap();

        let rendered = format!("{:?}", config.summary());
        assert!(!rendered.contains("super-secret-value"), "{rendered}");
        assert!(rendered.contains("(set)"), "{rendered}");
        assert!(
            rendered.contains("AKIAEXAMPLE"),
            "the access key is not secret"
        );
    }
}
