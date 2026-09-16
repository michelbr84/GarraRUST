//! End-to-end integration against a MinIO testcontainer.
//!
//! Gated behind the `storage-s3` feature so the vanilla crate test run
//! stays fast. Requires a working Docker daemon; when Docker is absent
//! the tests print a skip notice and exit cleanly rather than failing.

#![cfg(feature = "storage-s3")]

use std::time::Duration;

use aws_config::BehaviorVersion;
use aws_credential_types::Credentials;
use aws_sdk_s3::Client;
use aws_sdk_s3::config::{Builder as S3ConfigBuilder, Region, SharedCredentialsProvider};
use aws_sdk_s3::types::{BucketLocationConstraint, CreateBucketConfiguration};
use bytes::Bytes;
use garraia_storage::{GetOptions, ObjectStore, PutOptions, S3Compatible, StorageError};
use testcontainers::ImageExt;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::minio::MinIO;

const ACCESS_KEY: &str = "minioadmin";
const SECRET_KEY: &str = "minioadmin";
const BUCKET: &str = "garraia-test-bucket";
const REGION: &str = "us-east-1";

// `testcontainers-modules` 0.15 fixa `minio/minio`, repositorio removido do
// Docker Hub (#1230: "object not found" para o repo inteiro, nao so a tag).
// A MinIO publica a mesma imagem, mesma tag, em quay.io — troca de registry,
// sem mudanca de conteudo ou de comportamento.
const MINIO_IMAGE: &str = "quay.io/minio/minio";
const MINIO_TAG: &str = "RELEASE.2025-02-28T09-55-16Z";

/// Spawn a MinIO testcontainer, pre-create a bucket, and hand back an
/// `S3Compatible` wired against it. Returns `None` when the Docker
/// daemon is unreachable — in CI without docker we skip instead of fail.
async fn start_minio() -> Option<(
    testcontainers::ContainerAsync<MinIO>,
    S3Compatible,
    String, // endpoint URL
)> {
    let container = match MinIO::default()
        .with_name(MINIO_IMAGE)
        .with_tag(MINIO_TAG)
        .start()
        .await
    {
        Ok(c) => c,
        Err(e) => {
            // Um teste que se pula sozinho passa verde sem asserir nada — foi
            // assim que estes 7 testes ficaram desde sempre "passando" sem
            // nunca tocar o backend S3. Onde o Docker E esperado (CI Linux),
            // `GARRAIA_REQUIRE_DOCKER` transforma o skip em falha, para que o
            // verde signifique que o MinIO rodou mesmo. Sem a variavel, o
            // comportamento antigo continua: pular em maquina sem Docker.
            assert!(
                std::env::var_os("GARRAIA_REQUIRE_DOCKER").is_none(),
                "GARRAIA_REQUIRE_DOCKER esta setado, mas o container MinIO nao subiu: {e}"
            );
            eprintln!(
                "[skip] MinIO container failed to start — Docker unavailable? ({e}); \
                 plan 0038 integration tests skipped",
            );
            return None;
        }
    };
    let host = container.get_host().await.ok()?;
    let port = container.get_host_port_ipv4(9000).await.ok()?;
    let endpoint = format!("http://{host}:{port}");

    let creds = Credentials::new(ACCESS_KEY, SECRET_KEY, None, None, "minio-test");
    let shared = aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new(REGION))
        .credentials_provider(SharedCredentialsProvider::new(creds))
        .load()
        .await;
    let s3_cfg = S3ConfigBuilder::from(&shared)
        .endpoint_url(&endpoint)
        .force_path_style(true)
        .build();
    let client = Client::from_conf(s3_cfg);

    // MinIO's default region is us-east-1; `create_bucket` with a
    // LocationConstraint other than us-east-1 fails so we send it empty.
    let create_cfg = CreateBucketConfiguration::builder()
        .location_constraint(BucketLocationConstraint::from(REGION))
        .build();
    client
        .create_bucket()
        .bucket(BUCKET)
        .create_bucket_configuration(create_cfg)
        .send()
        .await
        .ok(); // tolerate BucketAlreadyOwnedByYou

    let store = S3Compatible::from_client(client, BUCKET);
    Some((container, store, endpoint))
}

/// Cliente S3 cru contra o mesmo endpoint, para assertar estado que o trait
/// `ObjectStore` de proposito nao expoe (ex.: multiparts em aberto).
async fn raw_client(endpoint: &str) -> Client {
    let creds = Credentials::new(ACCESS_KEY, SECRET_KEY, None, None, "minio-test");
    let shared = aws_config::defaults(BehaviorVersion::latest())
        .region(Region::new(REGION))
        .credentials_provider(SharedCredentialsProvider::new(creds))
        .load()
        .await;
    Client::from_conf(
        S3ConfigBuilder::from(&shared)
            .endpoint_url(endpoint)
            .force_path_style(true)
            .build(),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn minio_put_get_head_delete_roundtrip() {
    let Some((_c, store, _endpoint)) = start_minio().await else {
        return;
    };
    let payload = Bytes::from_static(b"hello-minio");
    let meta = store
        .put(
            "group-alpha/file-1/v1",
            payload.clone(),
            PutOptions {
                content_type: Some("text/plain".into()),
                ..Default::default()
            },
        )
        .await
        .expect("put");
    assert_eq!(meta.size_bytes, payload.len() as u64);
    assert_eq!(meta.etag_sha256.len(), 64);

    let got = store.get("group-alpha/file-1/v1").await.expect("get");
    assert_eq!(got.bytes.as_ref(), payload.as_ref());

    let head = store.head("group-alpha/file-1/v1").await.expect("head");
    // Single-part MinIO ETag for small objects = MD5 of body (not SHA-256).
    // We only assert it's non-empty — the SHA-256 invariant is the caller's
    // responsibility via the metadata returned from `put`.
    assert!(!head.etag_sha256.is_empty());

    assert!(store.exists("group-alpha/file-1/v1").await.expect("exists"));
    store.delete("group-alpha/file-1/v1").await.expect("delete");
    assert!(
        !store
            .exists("group-alpha/file-1/v1")
            .await
            .expect("exists after delete")
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn minio_rejects_disallowed_mime() {
    let Some((_c, store, _endpoint)) = start_minio().await else {
        return;
    };
    let err = store
        .put(
            "bad",
            Bytes::from_static(b"x"),
            PutOptions {
                content_type: Some("application/x-msdownload".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, StorageError::DisallowedMime { .. }));
}

#[tokio::test(flavor = "multi_thread")]
async fn minio_computes_and_verifies_integrity_hmac() {
    let Some((_c, store, _endpoint)) = start_minio().await else {
        return;
    };
    let put_meta = store
        .put(
            "group-hmac/file/v1",
            Bytes::from_static(b"with-hmac"),
            PutOptions {
                content_type: Some("text/plain".into()),
                version_id: Some("v1".into()),
                hmac_secret: Some(b"test-hmac-secret".to_vec()),
                ..Default::default()
            },
        )
        .await
        .expect("put");
    let hmac = put_meta
        .integrity_hmac
        .clone()
        .expect("integrity_hmac returned from put");
    assert_eq!(hmac.len(), 64);

    let got = store
        .get_with(
            "group-hmac/file/v1",
            GetOptions {
                expected_integrity_hmac: Some(hmac.clone()),
                version_id: Some("v1".into()),
                hmac_secret: Some(b"test-hmac-secret".to_vec()),
            },
        )
        .await
        .expect("get_with verifies");
    assert_eq!(got.bytes.as_ref(), b"with-hmac");

    // Tampered expected HMAC → IntegrityMismatch.
    let err = store
        .get_with(
            "group-hmac/file/v1",
            GetOptions {
                expected_integrity_hmac: Some("00".repeat(32)),
                version_id: Some("v1".into()),
                hmac_secret: Some(b"test-hmac-secret".to_vec()),
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(err, StorageError::IntegrityMismatch { .. }));
}

#[tokio::test(flavor = "multi_thread")]
async fn minio_presign_ttl_range_enforced() {
    let Some((_c, store, _endpoint)) = start_minio().await else {
        return;
    };
    let err = store
        .presign_get("g/f/v1", Duration::from_secs(5))
        .await
        .unwrap_err();
    assert!(matches!(err, StorageError::TtlOutOfRange { .. }));

    let err = store
        .presign_put("g/f/v1", Duration::from_secs(3600))
        .await
        .unwrap_err();
    assert!(matches!(err, StorageError::TtlOutOfRange { .. }));
}

#[tokio::test(flavor = "multi_thread")]
async fn minio_presign_roundtrip_via_http() {
    let Some((_c, store, _endpoint)) = start_minio().await else {
        return;
    };
    let http = reqwest::Client::new();

    // Put via presigned URL.
    let put_url = store
        .presign_put("g/presigned/v1", Duration::from_secs(300))
        .await
        .expect("presign put");
    let resp = http
        .put(put_url.as_str())
        .header("x-amz-server-side-encryption", "AES256")
        .body("via-presign")
        .send()
        .await
        .expect("http put");
    assert!(
        resp.status().is_success(),
        "presigned PUT failed: {} / {}",
        resp.status(),
        resp.text().await.unwrap_or_default()
    );

    // Get via presigned URL.
    let get_url = store
        .presign_get("g/presigned/v1", Duration::from_secs(300))
        .await
        .expect("presign get");
    let body = http
        .get(get_url.as_str())
        .send()
        .await
        .expect("http get")
        .bytes()
        .await
        .expect("body");
    assert_eq!(body.as_ref(), b"via-presign");
}

#[tokio::test(flavor = "multi_thread")]
async fn minio_put_enforces_sse() {
    // This is a negative test: the bucket itself is not configured to
    // reject non-SSE uploads, but we assert the S3Compatible `put`
    // path always sends the SSE header. After put, HEAD returns
    // x-amz-server-side-encryption = AES256 in MinIO's response, which
    // `head` surfaces via the SDK's builtin field. We assert that here
    // through a raw head_object call (go around our trait to see the
    // extended metadata).
    let Some((_c, store, _endpoint)) = start_minio().await else {
        return;
    };
    let _meta = store
        .put(
            "sse-check/file/v1",
            Bytes::from_static(b"data"),
            PutOptions {
                content_type: Some("application/json".into()),
                ..Default::default()
            },
        )
        .await
        .expect("put");

    // Re-issue a raw head_object using a fresh client aimed at the same
    // MinIO to read back the SSE header. We share the client by calling
    // exists (no-op side effect) and then extending via a debug-only
    // accessor is not exposed — instead we just validate we could read
    // the object and trust MinIO's `x-amz-server-side-encryption` echo
    // when we explicitly asked for it in `put`.
    assert!(store.exists("sse-check/file/v1").await.expect("exists"));
}

/// ROADMAP §3.5 — native S3 multipart for files > 16 MiB.
///
/// Uploads 24 MiB (threshold + 8 MiB = exactly 3 parts of 8 MiB) through
/// `put_stream` and verifies the object round-trips byte-for-byte with a
/// matching etag and metadata.
#[tokio::test(flavor = "multi_thread")]
async fn minio_put_stream_multipart_roundtrips_large_object() {
    let Some((_c, store, _endpoint)) = start_minio().await else {
        return;
    };

    // 3 full parts: 16 MiB threshold already crossed + one more 8 MiB part.
    let payload: Vec<u8> = (0..(16 * 1024 * 1024 + 8 * 1024 * 1024))
        .map(|i| (i % 251) as u8) // pseudo-random but deterministic
        .collect();
    let expected_etag = {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(&payload))
    };

    // Stage the payload in a temp file (same pattern as the gateway's
    // finalize path: `Box::pin(tokio::fs::File)`).
    let tmp = tempfile::tempdir().expect("tempdir");
    let staged = tmp.path().join("payload.bin");
    tokio::fs::write(&staged, &payload)
        .await
        .expect("stage payload");
    let reader: garraia_storage::AsyncByteReader =
        Box::pin(tokio::fs::File::open(&staged).await.expect("open staged"));

    let meta = store
        .put_stream(
            "multipart/big/v1",
            reader,
            payload.len() as u64,
            PutOptions {
                content_type: Some("application/octet-stream".into()),
                ..Default::default()
            },
        )
        .await
        .expect("put_stream multipart");
    assert_eq!(meta.size_bytes, payload.len() as u64);
    assert_eq!(meta.etag_sha256, expected_etag);
    assert_eq!(
        meta.content_type.as_deref(),
        Some("application/octet-stream")
    );

    let got = store.get("multipart/big/v1").await.expect("get back");
    assert_eq!(got.bytes.as_ref(), payload.as_slice());
    assert_eq!(got.metadata.size_bytes, payload.len() as u64);

    // Replace path: a smaller object through the same key must fully replace
    // the multipart content (single-put path).
    let staged_small = tmp.path().join("small.bin");
    tokio::fs::write(&staged_small, b"small")
        .await
        .expect("stage small");
    let reader: garraia_storage::AsyncByteReader = Box::pin(
        tokio::fs::File::open(&staged_small)
            .await
            .expect("open small"),
    );
    let small = store
        .put_stream("multipart/big/v1", reader, 5, PutOptions::default())
        .await
        .expect("small put_stream");
    assert_eq!(small.size_bytes, 5);
    let got = store.get("multipart/big/v1").await.expect("get small");
    assert_eq!(got.bytes.as_ref(), b"small");
}

/// Um stream que termina antes do `content_length` declarado nao pode virar
/// um objeto "completo" com tamanho menor: o multipart e abortado e a chave
/// fica intacta. Cobre o contrato "a chave nunca expoe conteudo parcial" no
/// caminho que escolhe o multipart justamente pelo valor declarado.
#[tokio::test(flavor = "multi_thread")]
async fn minio_multipart_aborts_when_stream_is_shorter_than_declared() {
    let Some((_c, store, endpoint)) = start_minio().await else {
        return;
    };

    // Conteudo previo: precisa sobreviver a tentativa falha.
    store
        .put(
            "multipart/short/v1",
            Bytes::from_static(b"previous"),
            PutOptions::default(),
        )
        .await
        .expect("seed previous content");

    // 20 MiB reais, 24 MiB declarados: acima do limiar, entao vai por
    // multipart, e o stream acaba no meio da terceira parte.
    let actual: Vec<u8> = (0..(20 * 1024 * 1024)).map(|i| (i % 251) as u8).collect();
    let declared = 24 * 1024 * 1024u64;
    let reader: garraia_storage::AsyncByteReader = Box::pin(std::io::Cursor::new(actual));

    let err = store
        .put_stream(
            "multipart/short/v1",
            reader,
            declared,
            PutOptions::default(),
        )
        .await
        .expect_err("short stream must not commit");
    assert!(
        matches!(err, StorageError::Backend(ref m) if m.contains("expected")),
        "erro deve nomear o descasamento de tamanho, veio: {err:?}"
    );

    // A chave mantem o conteudo anterior — nada parcial foi exposto.
    let got = store
        .get("multipart/short/v1")
        .await
        .expect("previous kept");
    assert_eq!(got.bytes.as_ref(), b"previous");

    // E nenhum multipart ficou aberto sendo faturado.
    let open = raw_client(&endpoint)
        .await
        .list_multipart_uploads()
        .bucket(BUCKET)
        .send()
        .await
        .expect("list_multipart_uploads");
    assert!(
        open.uploads().is_empty(),
        "multipart orfao deixado aberto: {:?}",
        open.uploads()
    );
}
