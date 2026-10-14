//! lazypock file storage.
//!
//! Uploads are validated before they are stored — by magic bytes, not by the
//! name or the declared content type — and then handed to a backend. The local
//! backend is the default and the S3-compatible one is being built behind the
//! same [`Storage`] trait.
//!
//! Object deletion goes through `_file_outbox` rather than happening in the
//! transaction that removed the record: an object deleted before the transaction
//! committed would be gone even if the transaction rolled back.

pub mod config;
pub mod keys;
pub mod outbox;
pub mod presets;
pub mod s3;
pub mod storage;
pub mod thumbnail;
pub mod validation;

pub use config::{FilesConfig, StorageAdapter};
pub use keys::{safe_filename, storage_key};
pub use presets::ImagePreset;
pub use storage::{Files, LocalStorage, Storage, StoredObject};
pub use thumbnail::ResizeMode;
pub use validation::{sniff, ImageFormat, UploadLimits};
