//! Where uploads go, from the environment.
//!
//! Configuration is environment-only on purpose: the storage secret is a
//! credential, and keeping it out of the database means it cannot leak through a
//! backup, a replica or the SQL console. The same reasoning is why the
//! access-token signing key is tracked as a gap rather than stored here.

use lazypock_core::error::{Error, Result};

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
}

impl FilesConfig {
    /// Default local directory.
    pub const DEFAULT_DIRECTORY: &'static str = "data/files";
    /// Default region, which is what Cloudflare R2 expects.
    pub const DEFAULT_REGION: &'static str = "auto";

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
        };

        if let Some(url) = &config.public_base_url {
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                return Err(Error::config(format!(
                    "LAZYPOCK_FILES_PUBLIC_URL must start with http:// or https://, got {url:?}"
                )));
            }
        }

        // An incomplete S3 configuration is refused here rather than failing on
        // the first upload.
        if config.adapter == StorageAdapter::S3 {
            for (key, value) in [
                ("LAZYPOCK_S3_ENDPOINT", &config.endpoint),
                ("LAZYPOCK_S3_BUCKET", &config.bucket),
                ("LAZYPOCK_S3_ACCESS_KEY", &config.access_key),
                ("LAZYPOCK_S3_SECRET_KEY", &config.secret_key),
            ] {
                if value.is_none() {
                    return Err(Error::config(format!(
                        "{key} is required when LAZYPOCK_FILES_ADAPTER=s3"
                    )));
                }
            }
        }

        Ok(config)
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
        ]
    }

    fn adapter_str(&self) -> &'static str {
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
