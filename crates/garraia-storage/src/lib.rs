//! `garraia-storage` — object storage abstraction for GarraIA.
//!
//! Plan 0037 (GAR-394 slice 1) ships the trait surface + a `LocalFs`
//! baseline implementation. S3 / MinIO / presigned URLs land in a follow-up
//! slice. See `plans/0037-gar-394-storage-skeleton.md` for the full scope
//! and non-goals.
//!
//! # Example
//!
//! ```no_run
//! # use garraia_storage::{LocalFs, ObjectStore, PutOptions};
//! # use bytes::Bytes;
//! # async fn go() -> Result<(), Box<dyn std::error::Error>> {
//! let store = LocalFs::new("/tmp/garraia-objects")?;
//! let meta = store
//!     .put("group-abc/file-123/v1", Bytes::from_static(b"hello"), PutOptions::default())
//!     .await?;
//! assert_eq!(meta.size_bytes, 5);
//! # Ok(()) }
//! ```

pub mod error;
pub mod local_fs;
pub mod object_store;
pub mod path_sanitize;

pub use error::{Result, StorageError};
pub use local_fs::LocalFs;
pub use object_store::{GetResult, ObjectMetadata, ObjectStore, PutOptions};
pub use path_sanitize::{SanitizeError, sanitise_key};
