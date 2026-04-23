//! `garraia-storage` — object storage abstraction for GarraIA.
//!
//! - Plan 0037 (GAR-394 slice 1) shipped the trait surface + `LocalFs`
//!   baseline.
//! - Plan 0038 (GAR-394 slice 2) adds `S3Compatible` (feature
//!   `storage-s3`), real presigned URLs (TTL range 30s–900s), SSE-S3
//!   enforcement, a shared MIME allow-list, and HMAC-SHA256 integrity
//!   signing anchored in ADR 0004 §Security.
//!
//! # Example
//!
//! ```no_run
//! # use garraia_storage::{LocalFs, ObjectStore, PutOptions};
//! # use bytes::Bytes;
//! # async fn go() -> Result<(), Box<dyn std::error::Error>> {
//! let store = LocalFs::new("/tmp/garraia-objects")?;
//! let meta = store
//!     .put(
//!         "group-abc/file-123/v1",
//!         Bytes::from_static(b"hello"),
//!         PutOptions {
//!             content_type: Some("text/plain".into()),
//!             ..Default::default()
//!         },
//!     )
//!     .await?;
//! assert_eq!(meta.size_bytes, 5);
//! # Ok(()) }
//! ```

pub mod error;
mod hash_util;
pub mod integrity;
pub mod local_fs;
pub mod mime_allowlist;
pub mod object_store;
pub mod path_sanitize;

#[cfg(feature = "storage-s3")]
pub mod s3_compat;

pub use error::{Result, StorageError};
pub use local_fs::LocalFs;
pub use object_store::{
    AsyncByteReader, GetOptions, GetResult, ObjectMetadata, ObjectStore, PRESIGN_TTL_MAX,
    PRESIGN_TTL_MIN, PutOptions,
};
pub use path_sanitize::{SanitizeError, sanitise_key};

#[cfg(feature = "storage-s3")]
pub use s3_compat::{S3Compatible, S3Config};
