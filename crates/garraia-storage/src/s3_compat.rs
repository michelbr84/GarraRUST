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
use aws_sdk_s3::config::http::HttpResponse;
use aws_sdk_s3::config::{
    Builder as S3ConfigBuilder, RequestChecksumCalculation, SharedCredentialsProvider,
};
use aws_sdk_s3::error::{ProvideErrorMetadata, SdkError};
use aws_sdk_s3::operation::head_object::HeadObjectError;
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{
    ChecksumAlgorithm, CompletedMultipartUpload, CompletedPart, ServerSideEncryption,
};
use base64::Engine as _;
use bytes::Bytes;
use sha2::{Digest, Sha256};
use tokio::io::AsyncReadExt;
use tracing::{debug, warn};
use url::Url;

use crate::error::{Result, StorageError};
use crate::hash_util::sha256_hex;
use crate::object_store::{
    AsyncByteReader, GetResult, ObjectMetadata, ObjectStore, PutOptions, check_mime_allowlist,
    check_presign_ttl, maybe_compute_integrity_hmac,
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
        // Checksum de request PINADO (#1229).
        //
        // O SDK le `AWS_REQUEST_CHECKSUM_CALCULATION` (e o
        // `request_checksum_calculation` do profile) do ambiente, e
        // `WHEN_REQUIRED` desliga o checksum que ele carimba sozinho. Este
        // `.request_checksum_calculation(...)` e aplicado DEPOIS do
        // `S3ConfigBuilder::from(&shared)` — que copia o valor vindo do
        // ambiente — entao o ambiente deixa de ter voto.
        //
        // Nota de escopo, para nao virar falsa sensacao de seguranca: os
        // checksums que importam aqui sao os EXPLICITOS. `put_object` e cada
        // `upload_part` mandam `checksum_sha256` como campo da operacao, e o
        // interceptor do SDK faz short-circuit quando o header ja existe,
        // antes mesmo de consultar esta preferencia. O pin cobre (a) as
        // operacoes onde nao nomeamos algoritmo nenhum e (b) a regressao
        // futura em que alguem remova o `.checksum_sha256(...)` e o ambiente
        // volte a decidir sozinho se o servidor verifica o que recebeu.
        builder = builder.request_checksum_calculation(RequestChecksumCalculation::WhenSupported);
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
        // Um HEAD nao tem corpo, entao o backend nao tem onde escrever o
        // codigo de erro: o proprio status 404 e a resposta. O SDK modela
        // isso como `HeadObjectError::NotFound`, mas nem todo S3-compativel
        // produz a forma que o parser reconhece — sem o fallback por status,
        // um 404 viraria `Backend(...)` e `exists` devolveria erro no lugar
        // de `false`.
        if matches!(err.as_service_error(), Some(HeadObjectError::NotFound(_)))
            || is_http_not_found(&err)
        {
            return StorageError::NotFound {
                key: key.to_owned(),
            };
        }
        backend_error("s3 head_object", &err)
    }
}

/// Descreve um `SdkError` sem jogar fora o que o servidor disse.
///
/// O `Display` de `SdkError` rende literalmente `"service error"`: o codigo
/// S3, a mensagem e o status HTTP moram no erro de servico, e o mapeamento
/// anterior os descartava. Foi por isso que a primeira execucao real contra
/// o MinIO reportou apenas `Backend("s3 put_object: service error")` e nao
/// disse que o servidor havia respondido `NotImplemented` (#1230).
///
/// Redaction: sai daqui apenas o que o SERVIDOR devolveu (codigo, mensagem,
/// status). Nunca a URI assinada nem os headers da requisicao, que carregam
/// `Authorization` / `X-Amz-Signature`.
fn describe_sdk_error<E>(op: &str, err: &SdkError<E, HttpResponse>) -> String
where
    E: ProvideErrorMetadata,
{
    let status = err.raw_response().map(|r| r.status().as_u16());
    match err {
        SdkError::ServiceError(_) => {
            let meta = err.meta();
            let code = meta.code().unwrap_or("Unknown");
            let message = meta.message().unwrap_or("<sem mensagem>");
            match status {
                Some(s) => format!("{op}: {code} (HTTP {s}): {message}"),
                None => format!("{op}: {code}: {message}"),
            }
        }
        SdkError::TimeoutError(_) => format!("{op}: timeout na requisicao"),
        SdkError::DispatchFailure(_) => {
            format!("{op}: falha ao despachar a requisicao (conexao/DNS/TLS)")
        }
        SdkError::ResponseError(_) => match status {
            Some(s) => format!("{op}: resposta ilegivel do backend (HTTP {s})"),
            None => format!("{op}: resposta ilegivel do backend"),
        },
        SdkError::ConstructionFailure(_) => format!("{op}: falha ao montar a requisicao"),
        _ => format!("{op}: erro nao classificado do SDK"),
    }
}

/// Acucar sobre [`describe_sdk_error`] para o caso comum.
fn backend_error<E>(op: &str, err: &SdkError<E, HttpResponse>) -> StorageError
where
    E: ProvideErrorMetadata,
{
    StorageError::Backend(describe_sdk_error(op, err))
}

/// `true` quando o backend respondeu 404, independente de ter conseguido
/// classificar o erro.
fn is_http_not_found<E>(err: &SdkError<E, HttpResponse>) -> bool {
    err.raw_response().map(|r| r.status().as_u16()) == Some(404)
}

fn sha256_base64(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    base64::engine::general_purpose::STANDARD.encode(hasher.finalize())
}

/// Threshold above which `put_stream` switches to native S3 multipart
/// (ROADMAP §3.5: "arquivos > 16 MiB").
pub(crate) const MULTIPART_THRESHOLD_BYTES: u64 = 16 * 1024 * 1024;
/// Part size for multipart uploads — must be ≥ the S3 minimum of 5 MiB.
const MULTIPART_PART_SIZE: usize = 8 * 1024 * 1024;

/// Aborta um multipart em voo a menos que explicitamente desarmado.
///
/// Os caminhos de erro do `put_stream_multipart` ja abortam com `await`, que
/// e o modo confiavel. Este guard cobre o que nenhum `?` alcanca: o
/// cancelamento da future. `finalize_upload` roda dentro de um handler Axum e,
/// quando o cliente derruba a conexao, o hyper dropa a future no meio — sem
/// isto o `upload_id` e as partes ja enviadas ficariam orfaos e faturados,
/// invisiveis no `ListObjects`.
///
/// O abort do `Drop` e best-effort e destacado (um `Drop` nao pode `await`):
/// nao sobrevive a `kill -9` nem ao shutdown do runtime. O backstop que
/// sobrevive e a regra de lifecycle `AbortIncompleteMultipartUpload` no
/// bucket — requisito de deploy, nao de codigo. Ver ADR 0004.
struct MultipartGuard {
    client: Arc<Client>,
    bucket: Arc<str>,
    key: String,
    upload_id: Option<String>,
}

impl MultipartGuard {
    /// Desarma: o caminho normal ja tratou este upload (completou ou abortou).
    fn disarm(&mut self) {
        self.upload_id = None;
    }
}

impl Drop for MultipartGuard {
    fn drop(&mut self) {
        let Some(upload_id) = self.upload_id.take() else {
            return;
        };
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            warn!(
                target: "garraia_storage::s3",
                key = %self.key,
                "multipart abort ignorado: sem runtime tokio; \
                 depende da regra de lifecycle do bucket"
            );
            return;
        };
        let client = self.client.clone();
        let bucket = self.bucket.clone();
        let key = self.key.clone();
        handle.spawn(async move {
            let abort = client
                .abort_multipart_upload()
                .bucket(bucket.as_ref())
                .key(&key)
                .upload_id(&upload_id)
                .send()
                .await;
            match abort {
                Ok(_) => debug!(
                    target: "garraia_storage::s3",
                    key = %key,
                    "multipart abortado apos cancelamento da future"
                ),
                Err(e) => warn!(
                    target: "garraia_storage::s3",
                    key = %key,
                    "multipart abort apos cancelamento falhou: {}",
                    describe_sdk_error("s3 abort_multipart_upload", &e)
                ),
            }
        });
    }
}

impl S3Compatible {
    /// Best-effort abort of an in-flight multipart upload. Failure to abort is
    /// logged and swallowed: the caller is already returning the original
    /// error, which must not be masked by the cleanup.
    async fn abort_upload(&self, key: &str, upload_id: &str) {
        let abort = self
            .client
            .abort_multipart_upload()
            .bucket(self.bucket.as_ref())
            .key(key)
            .upload_id(upload_id)
            .send()
            .await;
        if let Err(ae) = abort {
            warn!(
                target: "garraia_storage::s3",
                key = %key,
                "multipart abort failed: {}",
                describe_sdk_error("s3 abort_multipart_upload", &ae)
            );
        }
    }

    /// Multipart path: streams the reader in 8 MiB parts. On any failure the
    /// multipart upload is aborted so the key never exposes partial content.
    async fn put_stream_multipart(
        &self,
        key: &str,
        mut reader: AsyncByteReader,
        opts: PutOptions,
        content_length: u64,
    ) -> Result<ObjectMetadata> {
        // 1) Create the multipart upload (SSE-S3 mandated by ADR 0004).
        //
        // O algoritmo de checksum e declarado aqui DE PROPOSITO. Sem essa
        // declaracao o SDK ainda assim carimba um CRC32 default em cada
        // `upload_part` (`RequestChecksumCalculation::WhenSupported`), e o
        // `complete` mandava so os ETags — um multipart onde criacao, partes
        // e conclusao discordam sobre o checksum. Declarando SHA-256 dos tres
        // lados, o servidor verifica cada parte contra o hash que nos mesmos
        // calculamos, no mesmo algoritmo que o `put` de uma parte so ja usa.
        let mut create = self
            .client
            .create_multipart_upload()
            .bucket(self.bucket.as_ref())
            .key(key)
            .server_side_encryption(ServerSideEncryption::Aes256)
            .checksum_algorithm(ChecksumAlgorithm::Sha256);
        if let Some(ct) = opts.content_type.clone() {
            create = create.content_type(ct);
        }
        if let Some(cc) = opts.cache_control.clone() {
            create = create.cache_control(cc);
        }
        let created = create
            .send()
            .await
            .map_err(|e| backend_error("s3 create_multipart_upload", &e))?;
        let upload_id = created
            .upload_id()
            .ok_or_else(|| {
                StorageError::Backend("s3 create_multipart_upload: no upload_id".into())
            })?
            .to_owned();

        // Armado aqui: a partir deste ponto existe um upload em voo que
        // precisa ser abortado mesmo se esta future for cancelada.
        let mut guard = MultipartGuard {
            client: self.client.clone(),
            bucket: self.bucket.clone(),
            key: key.to_owned(),
            upload_id: Some(upload_id.clone()),
        };

        // 2) Stream parts.
        let mut hasher = Sha256::new();
        let mut total: u64 = 0;
        let mut parts: Vec<CompletedPart> = Vec::new();
        let mut part_number: i32 = 0;
        let result: Result<()> = loop {
            let chunk = match read_chunk(&mut reader, MULTIPART_PART_SIZE).await {
                Ok(c) => c,
                Err(e) => break Err(StorageError::Io(e)),
            };
            if chunk.is_empty() {
                break Ok(());
            }
            part_number += 1;
            hasher.update(&chunk);
            total += chunk.len() as u64;

            let part_checksum = sha256_base64(&chunk);
            let upload = self
                .client
                .upload_part()
                .bucket(self.bucket.as_ref())
                .key(key)
                .upload_id(upload_id.as_str())
                .part_number(part_number)
                .checksum_sha256(part_checksum.clone())
                .body(ByteStream::from(chunk));
            match upload.send().await {
                Ok(out) => {
                    // Sem ETag o `complete` falharia com erro opaco de parte
                    // invalida; falhar aqui nomeia a causa real.
                    let Some(etag) = out.e_tag().map(|t| t.to_owned()) else {
                        break Err(StorageError::Backend(format!(
                            "s3 upload_part {part_number}: no e_tag in response"
                        )));
                    };
                    parts.push(
                        CompletedPart::builder()
                            .part_number(part_number)
                            .e_tag(etag)
                            .checksum_sha256(part_checksum)
                            .build(),
                    );
                }
                Err(e) => {
                    break Err(backend_error(&format!("s3 upload_part {part_number}"), &e));
                }
            }
        };

        // 3) Complete or abort — abort MUST run on every failure path so the
        //    key never exposes partial content and no orphan upload is billed.
        //    Ordem: abortar e so entao desarmar, para que o cancelamento da
        //    future durante o proprio abort ainda caia no `Drop` do guard.
        if let Err(e) = result {
            self.abort_upload(key, &upload_id).await;
            guard.disarm();
            return Err(e);
        }
        // An empty stream would make `complete_multipart_upload` fail with the
        // upload still open; reject it here so the abort is not skipped.
        if parts.is_empty() {
            self.abort_upload(key, &upload_id).await;
            guard.disarm();
            return Err(StorageError::Backend(
                "s3 multipart: stream yielded no bytes".into(),
            ));
        }
        // A reader that ends early would otherwise be committed as a complete
        // object, with `size_bytes` silently below the declared length.
        if total != content_length {
            self.abort_upload(key, &upload_id).await;
            guard.disarm();
            return Err(StorageError::Backend(format!(
                "s3 multipart: stream yielded {total} bytes, expected {content_length}"
            )));
        }

        let completed = match self
            .client
            .complete_multipart_upload()
            .bucket(self.bucket.as_ref())
            .key(key)
            .upload_id(upload_id.as_str())
            .multipart_upload(
                CompletedMultipartUpload::builder()
                    .set_parts(Some(parts))
                    .build(),
            )
            .send()
            .await
        {
            Ok(c) => c,
            Err(e) => {
                self.abort_upload(key, &upload_id).await;
                guard.disarm();
                return Err(backend_error("s3 complete_multipart_upload", &e));
            }
        };
        // Objeto materializado: nao ha mais nada a abortar.
        guard.disarm();
        debug!(
            target: "garraia_storage::s3",
            bucket = %self.bucket,
            key = %key,
            size = total,
            parts = part_number,
            "put_stream (multipart, etag={:?})",
            completed.e_tag()
        );

        let etag = sha256_hex_from_hasher(hasher);
        let integrity_hmac = maybe_compute_integrity_hmac(&opts, key, &etag);
        Ok(ObjectMetadata {
            key: key.to_owned(),
            size_bytes: total,
            etag_sha256: etag,
            content_type: opts.content_type,
            integrity_hmac,
        })
    }
}

/// Read up to `len` bytes, looping until the buffer is full or EOF.
async fn read_chunk(
    reader: &mut crate::object_store::AsyncByteReader,
    len: usize,
) -> std::io::Result<Bytes> {
    let mut buf = vec![0u8; len];
    let mut filled = 0usize;
    while filled < len {
        let n = reader.read(&mut buf[filled..]).await?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    buf.truncate(filled);
    Ok(Bytes::from(buf))
}

/// Finalize a `Sha256` hasher into the hex etag used by this backend.
fn sha256_hex_from_hasher(hasher: Sha256) -> String {
    hex::encode(hasher.finalize())
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
            .map_err(|e| backend_error("s3 put_object", &e))?;

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

    /// Streaming upload with native S3 multipart for large payloads
    /// (ROADMAP §3.5: files > 16 MiB must not be buffered in memory).
    ///
    /// - `content_length <= MULTIPART_THRESHOLD_BYTES` → buffered single
    ///   `put_object` (same path as [`Self::put`]).
    /// - Larger → `create_multipart_upload` + 8 MiB parts +
    ///   `complete_multipart_upload`; any mid-stream failure aborts the
    ///   multipart upload so the target key never shows partial content.
    async fn put_stream(
        &self,
        key: &str,
        mut reader: AsyncByteReader,
        content_length: u64,
        opts: PutOptions,
    ) -> Result<ObjectMetadata> {
        check_mime_allowlist(&opts)?;
        let key = sanitise_key(key)?;

        if content_length <= MULTIPART_THRESHOLD_BYTES {
            let mut buf = Vec::new();
            reader
                .read_to_end(&mut buf)
                .await
                .map_err(StorageError::Io)?;
            return self.put(key, Bytes::from(buf), opts).await;
        }

        self.put_stream_multipart(key, reader, opts, content_length)
            .await
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
            .map_err(|e| {
                if matches!(
                    e.as_service_error(),
                    Some(aws_sdk_s3::operation::get_object::GetObjectError::NoSuchKey(_))
                ) || is_http_not_found(&e)
                {
                    return StorageError::NotFound {
                        key: key.to_owned(),
                    };
                }
                backend_error("s3 get_object", &e)
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
            .map_err(|e| backend_error("s3 delete_object", &e))?;
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
            // Mesma classificacao do `head`: um 404 e "nao existe", nao erro.
            Err(e) => match self.map_head_error(key, e) {
                StorageError::NotFound { .. } => Ok(false),
                other => Err(other),
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
            .map_err(|e| backend_error("s3 presign_put", &e))?;
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
            .map_err(|e| backend_error("s3 presign_get", &e))?;
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

    use aws_sdk_s3::error::ErrorMetadata;
    use aws_sdk_s3::operation::put_object::PutObjectError;
    use aws_sdk_s3::primitives::SdkBody;
    use aws_smithy_runtime_api::http::StatusCode;

    fn service_error<E>(err: E, status: u16) -> SdkError<E, HttpResponse> {
        let raw = HttpResponse::new(StatusCode::try_from(status).unwrap(), SdkBody::empty());
        SdkError::service_error(err, raw)
    }

    /// Regressao de #1230: o primeiro run real contra o MinIO so disse
    /// `s3 put_object: service error`, escondendo que o servidor havia
    /// respondido `NotImplemented` por falta de KMS. O codigo, a mensagem e o
    /// status do servidor precisam sobreviver ao mapeamento.
    #[test]
    fn service_error_carries_code_message_and_status() {
        let meta = ErrorMetadata::builder()
            .code("NotImplemented")
            .message("Server side encryption specified but KMS is not configured")
            .build();
        let err = service_error(PutObjectError::generic(meta), 501);
        let rendered = describe_sdk_error("s3 put_object", &err);
        assert!(rendered.contains("s3 put_object"), "{rendered}");
        assert!(rendered.contains("NotImplemented"), "{rendered}");
        assert!(rendered.contains("HTTP 501"), "{rendered}");
        assert!(rendered.contains("KMS is not configured"), "{rendered}");
    }

    #[test]
    fn service_error_without_metadata_still_names_the_operation() {
        let err = service_error(
            PutObjectError::generic(ErrorMetadata::builder().build()),
            500,
        );
        let rendered = describe_sdk_error("s3 put_object", &err);
        assert!(rendered.contains("s3 put_object"), "{rendered}");
        assert!(rendered.contains("HTTP 500"), "{rendered}");
    }

    /// Um 404 sem codigo de erro (HEAD nao tem corpo onde escreve-lo) tem de
    /// virar `NotFound`, senao `exists` devolve erro em vez de `false`.
    #[test]
    fn head_404_without_error_code_maps_to_not_found() {
        let store = offline_store();
        let err = service_error(
            HeadObjectError::generic(ErrorMetadata::builder().build()),
            404,
        );
        assert!(matches!(
            store.map_head_error("k/v1", err),
            StorageError::NotFound { .. }
        ));
    }

    #[test]
    fn head_500_is_a_backend_error_not_not_found() {
        let store = offline_store();
        let err = service_error(
            HeadObjectError::generic(ErrorMetadata::builder().code("InternalError").build()),
            500,
        );
        let mapped = store.map_head_error("k/v1", err);
        match mapped {
            StorageError::Backend(msg) => {
                assert!(msg.contains("InternalError"), "{msg}");
                assert!(msg.contains("s3 head_object"), "{msg}");
            }
            other => panic!("esperado Backend, veio {other:?}"),
        }
    }

    /// Regressao de #1229: o ambiente NAO pode desligar o checksum de
    /// request.
    ///
    /// `AWS_REQUEST_CHECKSUM_CALCULATION=WHEN_REQUIRED` e uma variavel que o
    /// SDK honra sozinho — uma linha no deploy bastaria para o cliente parar
    /// de carimbar checksum e o servidor parar de verificar o que recebeu.
    /// `S3Compatible::new` fixa `WhenSupported` DEPOIS de herdar a config do
    /// ambiente, entao o valor do ambiente e engolido.
    ///
    /// O teste roda `#[serial]` porque mexe em env var, que e estado global
    /// do processo, e restaura o valor anterior antes de sair.
    #[tokio::test]
    #[serial_test::serial]
    async fn env_cannot_downgrade_request_checksum_calculation() {
        const VAR: &str = "AWS_REQUEST_CHECKSUM_CALCULATION";
        let previous = std::env::var(VAR).ok();
        // SAFETY: edicao 2024 exige `unsafe` para mexer no env do processo.
        // `#[serial]` garante que nenhum outro teste deste binario roda em
        // paralelo, e o valor anterior e restaurado no fim.
        unsafe { std::env::set_var(VAR, "WHEN_REQUIRED") };

        let store = S3Compatible::new(S3Config {
            bucket: "bucket-de-teste".into(),
            region: "us-east-1".into(),
            endpoint_url: Some("http://127.0.0.1:1".into()),
            force_path_style: true,
            credentials: Some(Credentials::new("AKID", "SECRET", None, None, "test")),
        })
        .await
        .expect("construir o cliente nao faz I/O de rede");

        let effective = store
            .client
            .config()
            .request_checksum_calculation()
            .cloned();

        // SAFETY: mesmo racional do `set_var` acima.
        unsafe {
            match previous {
                Some(v) => std::env::set_var(VAR, v),
                None => std::env::remove_var(VAR),
            }
        }

        assert_eq!(
            effective,
            Some(RequestChecksumCalculation::WhenSupported),
            "o ambiente conseguiu rebaixar o checksum de request: {effective:?}"
        );
    }

    /// Cliente que nunca sai para a rede: so precisamos de um `S3Compatible`
    /// para alcancar `map_head_error`.
    fn offline_store() -> S3Compatible {
        let conf = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new("us-east-1"))
            .credentials_provider(Credentials::new("AKID", "SECRET", None, None, "test"))
            .endpoint_url("http://127.0.0.1:1")
            .force_path_style(true)
            .build();
        S3Compatible::from_client(Client::from_conf(conf), "bucket-de-teste")
    }

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
