//! S3-compatible backend (AWS S3, MinIO via endpoint override, Cloudflare
//! R2, Backblaze B2, etc.).
//!
//! Gated behind the `storage-s3` feature so the baseline crate stays
//! lightweight. The backend enforces three contract-level invariants
//! mandated by ADR 0004:
//!
//! 1. **SSE-S3** — every `put` requests `ServerSideEncryption::Aes256`.
//!    Buckets SHOULD additionally enforce `Condition: StringEquals:
//!    s3:x-amz-server-side-encryption = AES256` so uploads that somehow
//!    bypass this client fail at the server.
//! 2. **MIME allow-list** (shared with `LocalFs`) — opt-out explicit via
//!    `PutOptions::allow_unsafe_mime`.
//! 3. **Presigned URL TTL range** — `[30s, 900s]` enforced pre-call.
//!
//! **Slice 3+ follow-ups (ADR 0004 §Security gaps, plan 0038 audit):**
//! - `audit_events` emission (`file.uploaded`, `file.deleted`,
//!   `file.presign_get_issued`) belongs in the gateway handler, not
//!   this crate.
//! - Cross-tenant isolation (handler validates `file.group_id =
//!   caller.group_id` before `put`/`get`/presign) — gateway slice 3.
//! - Short-lived IAM credentials (prefer role over static keys) —
//!   deploy docs slice 3.
//! - Bucket-level default encryption + `Deny` policy on non-SSE
//!   uploads — deploy docs slice 3.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use aws_config::{BehaviorVersion, Region};
use aws_credential_types::Credentials;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Builder as S3ConfigBuilder, SharedCredentialsProvider};
use aws_sdk_s3::error::SdkError;
use aws_sdk_s3::operation::head_object::HeadObjectError;
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::ServerSideEncryption;
use base64::Engine as _;
use bytes::Bytes;
use sha2::{Digest, Sha256};
use tracing::{debug, warn};
use url::Url;

use crate::error::{Result, StorageError};
use crate::hash_util::sha256_hex;
use crate::object_store::{
    GetResult, ObjectMetadata, ObjectStore, PutOptions, check_mime_allowlist, check_presign_ttl,
    maybe_compute_integrity_hmac,
};
use crate::path_sanitize::sanitise_key;

/// Static configuration for an S3-compatible backend.
#[derive(Clone)]
pub struct S3Config {
    pub bucket: String,
    pub region: String,
    /// Optional endpoint override — set to MinIO / R2 / B2 URLs. When
    /// `None` the SDK uses the real AWS endpoint for the region.
    pub endpoint_url: Option<String>,
    /// When `true`, the SDK uses path-style requests (`https://host/bucket/key`)
    /// instead of virtual-host-style. Required for MinIO without custom DNS.
    pub force_path_style: bool,
    /// Static credentials. `None` lets `aws-config` discover IAM role /
    /// env vars / profile chain.
    pub credentials: Option<Credentials>,
}

impl std::fmt::Debug for S3Config {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Config")
            .field("bucket", &self.bucket)
            .field("region", &self.region)
            .field("endpoint_url", &self.endpoint_url)
            .field("force_path_style", &self.force_path_style)
            .field(
                "credentials",
                &self.credentials.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

impl S3Config {
    /// Build from `GARRAIA_STORAGE_S3_*` env vars. Minimal required vars:
    /// `GARRAIA_STORAGE_S3_BUCKET`, `GARRAIA_STORAGE_S3_REGION`.
    /// Optional: `GARRAIA_STORAGE_S3_ENDPOINT`,
    /// `GARRAIA_STORAGE_S3_FORCE_PATH_STYLE` (`true`/`false`),
    /// `GARRAIA_STORAGE_S3_ACCESS_KEY_ID` +
    /// `GARRAIA_STORAGE_S3_SECRET_ACCESS_KEY`.
    pub fn from_env() -> Result<Self> {
        let bucket = std::env::var("GARRAIA_STORAGE_S3_BUCKET").map_err(|_| {
            StorageError::Backend(
                "GARRAIA_STORAGE_S3_BUCKET env var not set; S3 backend requires an explicit bucket"
                    .into(),
            )
        })?;
        let region =
            std::env::var("GARRAIA_STORAGE_S3_REGION").unwrap_or_else(|_| "us-east-1".to_string());
        let endpoint_url = std::env::var("GARRAIA_STORAGE_S3_ENDPOINT").ok();
        let force_path_style = std::env::var("GARRAIA_STORAGE_S3_FORCE_PATH_STYLE")
            .ok()
            .map(|s| s.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let credentials = match (
            std::env::var("GARRAIA_STORAGE_S3_ACCESS_KEY_ID").ok(),
            std::env::var("GARRAIA_STORAGE_S3_SECRET_ACCESS_KEY").ok(),
        ) {
            (Some(id), Some(secret)) => Some(Credentials::new(
                id,
                secret,
                None,
                None,
                "garraia-storage-env",
            )),
            _ => None,
        };
        Ok(Self {
            bucket,
            region,
            endpoint_url,
            force_path_style,
            credentials,
        })
    }
}

/// S3-compatible backend.
#[derive(Clone)]
pub struct S3Compatible {
    client: Arc<Client>,
    bucket: Arc<str>,
}

impl std::fmt::Debug for S3Compatible {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("S3Compatible")
            .field("bucket", &self.bucket)
            .finish()
    }
}

impl S3Compatible {
    /// Build a client from a fully-specified [`S3Config`].
    pub async fn new(cfg: S3Config) -> Result<Self> {
        let mut loader =
            aws_config::defaults(BehaviorVersion::latest()).region(Region::new(cfg.region.clone()));
        if let Some(creds) = cfg.credentials.clone() {
            loader = loader.credentials_provider(SharedCredentialsProvider::new(creds));
        }
        let shared = loader.load().await;

        let mut builder = S3ConfigBuilder::from(&shared);
        if let Some(endpoint) = cfg.endpoint_url.clone() {
            builder = builder.endpoint_url(endpoint);
        }
        if cfg.force_path_style {
            builder = builder.force_path_style(true);
        }
        let client = Client::from_conf(builder.build());
        Ok(Self {
            client: Arc::new(client),
            bucket: Arc::from(cfg.bucket),
        })
    }

    /// Escape hatch for tests and for slice 3 wiring: inject a pre-built
    /// client so we can thread integration-specific config (e.g.
    /// testcontainer credentials + endpoint) without going through
    /// env vars.
    pub fn from_client(client: Client, bucket: impl Into<String>) -> Self {
        Self {
            client: Arc::new(client),
            bucket: Arc::from(bucket.into()),
        }
    }

    fn map_head_error(&self, key: &str, err: SdkError<HeadObjectError>) -> StorageError {
        match err.into_service_error() {
            HeadObjectError::NotFound(_) => StorageError::NotFound {
                key: key.to_owned(),
            },
            other => StorageError::Backend(format!("s3 head: {other}")),
        }
    }
}

fn sha256_base64(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    base64::engine::general_purpose::STANDARD.encode(hasher.finalize())
}

#[async_trait]
impl ObjectStore for S3Compatible {
    async fn put(&self, key: &str, bytes: Bytes, opts: PutOptions) -> Result<ObjectMetadata> {
        check_mime_allowlist(&opts)?;
        let key = sanitise_key(key)?;
        let size_bytes = bytes.len() as u64;
        let etag = sha256_hex(&bytes);
        let checksum_b64 = sha256_base64(&bytes);

        let mut req = self
            .client
            .put_object()
            .bucket(self.bucket.as_ref())
            .key(key)
            .server_side_encryption(ServerSideEncryption::Aes256)
            .checksum_sha256(checksum_b64)
            .body(ByteStream::from(bytes));
        if let Some(ct) = opts.content_type.clone() {
            req = req.content_type(ct);
        }
        if let Some(cc) = opts.cache_control.clone() {
            req = req.cache_control(cc);
        }

        req.send()
            .await
            .map_err(|e| StorageError::Backend(format!("s3 put_object: {e}")))?;

        let integrity_hmac = maybe_compute_integrity_hmac(&opts, key, &etag);
        debug!(
            target: "garraia_storage::s3",
            bucket = %self.bucket,
            key = %key,
            size = size_bytes,
            "put"
        );
        Ok(ObjectMetadata {
            key: key.to_owned(),
            size_bytes,
            etag_sha256: etag,
            content_type: opts.content_type,
            integrity_hmac,
        })
    }

    async fn get(&self, key: &str) -> Result<GetResult> {
        let key = sanitise_key(key)?;
        let resp = self
            .client
            .get_object()
            .bucket(self.bucket.as_ref())
            .key(key)
            .send()
            .await
            .map_err(|e| match e.into_service_error() {
                aws_sdk_s3::operation::get_object::GetObjectError::NoSuchKey(_) => {
                    StorageError::NotFound {
                        key: key.to_owned(),
                    }
                }
                other => StorageError::Backend(format!("s3 get_object: {other}")),
            })?;

        let content_type = resp.content_type().map(|s| s.to_owned());
        let body = resp
            .body
            .collect()
            .await
            .map_err(|e| StorageError::Backend(format!("s3 body collect: {e}")))?
            .into_bytes();
        let size_bytes = body.len() as u64;
        let etag = sha256_hex(&body);
        Ok(GetResult {
            bytes: body,
            metadata: ObjectMetadata {
                key: key.to_owned(),
                size_bytes,
                etag_sha256: etag,
                content_type,
                integrity_hmac: None,
            },
        })
    }

    async fn head(&self, key: &str) -> Result<ObjectMetadata> {
        let key = sanitise_key(key)?;
        let resp = self
            .client
            .head_object()
            .bucket(self.bucket.as_ref())
            .key(key)
            .send()
            .await
            .map_err(|e| self.map_head_error(key, e))?;

        // S3 does not return the raw body on HEAD, so `etag_sha256` below
        // comes from the object's own ETag header. For single-part uploads
        // the ETag matches the MD5 of the body (not SHA-256) — this
        // differs from LocalFs. Callers that require SHA-256 from HEAD
        // should use `get` instead (plan 0038 §2 non-goal).
        let size_bytes = resp.content_length().unwrap_or(0) as u64;
        let etag = resp.e_tag().unwrap_or("").trim_matches('"').to_owned();
        let content_type = resp.content_type().map(|s| s.to_owned());
        Ok(ObjectMetadata {
            key: key.to_owned(),
            size_bytes,
            etag_sha256: etag,
            content_type,
            integrity_hmac: None,
        })
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let key = sanitise_key(key)?;
        self.client
            .delete_object()
            .bucket(self.bucket.as_ref())
            .key(key)
            .send()
            .await
            .map_err(|e| StorageError::Backend(format!("s3 delete_object: {e}")))?;
        Ok(())
    }

    async fn exists(&self, key: &str) -> Result<bool> {
        let key = sanitise_key(key)?;
        match self
            .client
            .head_object()
            .bucket(self.bucket.as_ref())
            .key(key)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(e) => match e.into_service_error() {
                HeadObjectError::NotFound(_) => Ok(false),
                other => Err(StorageError::Backend(format!("s3 exists: {other}"))),
            },
        }
    }

    /// Issue a presigned PUT URL. The URL is scoped to the bucket + key
    /// + TTL; the caller PUTs bytes directly against it without holding
    /// AWS credentials.
    ///
    /// **Security note (plan 0038 audit SEC-H):** the
    /// `x-amz-server-side-encryption` header requested here is included
    /// in the signed query string the SDK produces, but the exact
    /// contract depends on SDK/provider behaviour. MinIO, R2 and AWS all
    /// accept client PUTs *without* that header when the bucket has no
    /// default encryption configured — the SDK-side signing does not
    /// prevent omission. The production deployment MUST therefore
    /// enable bucket-level default encryption OR enforce via bucket
    /// policy (`"Deny": "PutObject" when "x-amz-server-side-encryption"
    /// is absent`). The storage layer does NOT attempt to detect or
    /// configure bucket policy — that is a deploy-time concern
    /// documented in `docs/storage.md` (slice 3).
    async fn presign_put(&self, key: &str, ttl: Duration) -> Result<Url> {
        check_presign_ttl(ttl)?;
        let key = sanitise_key(key)?;
        let cfg = PresigningConfig::expires_in(ttl)
            .map_err(|e| StorageError::Backend(format!("presign config: {e}")))?;
        let req = self
            .client
            .put_object()
            .bucket(self.bucket.as_ref())
            .key(key)
            .server_side_encryption(ServerSideEncryption::Aes256)
            .presigned(cfg)
            .await
            .map_err(|e| StorageError::Backend(format!("s3 presign_put: {e}")))?;
        let uri = req.uri().to_string();
        Url::parse(&uri).map_err(|e| {
            warn!(target: "garraia_storage::s3", "presign_put returned non-parseable URL: {e}");
            StorageError::Backend(format!("presigned URL unparseable: {e}"))
        })
    }

    async fn presign_get(&self, key: &str, ttl: Duration) -> Result<Url> {
        check_presign_ttl(ttl)?;
        let key = sanitise_key(key)?;
        let cfg = PresigningConfig::expires_in(ttl)
            .map_err(|e| StorageError::Backend(format!("presign config: {e}")))?;
        let req = self
            .client
            .get_object()
            .bucket(self.bucket.as_ref())
            .key(key)
            .presigned(cfg)
            .await
            .map_err(|e| StorageError::Backend(format!("s3 presign_get: {e}")))?;
        let uri = req.uri().to_string();
        Url::parse(&uri).map_err(|e| {
            warn!(target: "garraia_storage::s3", "presign_get returned non-parseable URL: {e}");
            StorageError::Backend(format!("presigned URL unparseable: {e}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // NOTE: we avoid writing a `from_env_requires_bucket` test that
    // mutates process-wide env state because `cargo test` runs tests in
    // parallel inside the same process and `std::env::remove_var` is
    // `unsafe` + racy (code review NIT). The happy-path validation
    // happens in the MinIO integration test instead — it asserts that a
    // fully-specified client can put/get bytes.

    #[test]
    fn s3_config_debug_redacts_credentials() {
        let cfg = S3Config {
            bucket: "b".into(),
            region: "us-east-1".into(),
            endpoint_url: None,
            force_path_style: false,
            credentials: Some(Credentials::new("AKID", "SUPER_SECRET", None, None, "test")),
        };
        let rendered = format!("{cfg:?}");
        assert!(!rendered.contains("SUPER_SECRET"));
        assert!(!rendered.contains("AKID"));
        assert!(rendered.contains("<redacted>"));
    }
}
