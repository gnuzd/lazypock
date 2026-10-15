//! lazypock file storage.
//!
//! Uploads are validated before they are stored — by magic bytes, not by the
//! name or the declared content type — and then handed to a backend. The local
//! backend is the default and the S3-compatible one is being built behind the
//! same [`Storage`] trait.
//!
//! Object deletion goes through a queue rather than happening in the transaction that
//! removed the reference: an object deleted before the transaction committed would be
//! gone even if the transaction rolled back. Two queues exist and [`reaper`] drains
//! both — `_file_outbox` for a key that is known directly, and `_file_deletions`, which
//! the `_files_deleted` trigger fills whenever a media-library row goes.
//!
//! Image resizing runs in this process by default. Setting
//! `LAZYPOCK_IMAGE_SERVICE_URL` moves it to an external service instead, and a build
//! without the `local-image-codecs` feature cannot resize locally at all — see
//! [`image_service`] and [`thumbnail`].

pub mod config;
pub mod image_service;
pub mod keys;
pub mod outbox;
pub mod presets;
pub mod reaper;
pub mod s3;
pub mod storage;
pub mod store;
pub mod sweep;
pub mod thumbnail;
pub mod validation;

pub use config::{FilesConfig, StorageAdapter};
pub use image_service::ImageService;
pub use keys::{safe_filename, storage_key};
pub use presets::ImagePreset;
pub use reaper::Reaper;
pub use storage::{Files, LocalStorage, Storage, StoredObject};
pub use store::{FileRef, FileRow, NewFile, Origin, Status};
pub use sweep::{SweepOptions, SweepReport};
pub use thumbnail::{OutputFormat, ResizeMode};
pub use validation::{sniff, ImageFormat, UploadLimits};
