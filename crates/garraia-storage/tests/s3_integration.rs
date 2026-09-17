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
use aws_sdk_s3::error::ProvideErrorMetadata;
use aws_sdk_s3::types::{
    BucketLocationConstraint, ChecksumAlgorithm, CreateBucketConfiguration, ObjectAttributes,
    ServerSideEncryption,
};
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

// `S3Compatible::put` sempre manda `server_side_encryption(Aes256)`
// (s3_compat.rs) — na AWS isso e zero-config (chave gerenciada pela AWS),
// mas o MinIO recusa QUALQUER SSE sem um KMS configurado ("Server side
// encryption specified but KMS is not configured"). Sem isto nenhum `put`
// chegava a rodar de verdade, porque o teste sempre pulava antes (#1230).
// Chave estatica fixa, so para o container efemero deste teste — nao e
// segredo de producao, e o modo "legacy single-key" do MinIO documentado em
// docs.min.io/enterprise/minio-kms/legacy-key-management.
const MINIO_KMS_SECRET_KEY: &str = "garraia-test-key:7neeKXXrCI7222jSKgyzjaTDD53W0OvdenD3gJ2DZgU=";

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
        .with_env_var("MINIO_KMS_SECRET_KEY", MINIO_KMS_SECRET_KEY)
        .start()
        .await
    {
        Ok(c) => c,
        Err(e) => {
            // Um teste que se pula sozinho passa verde sem asserir nada — foi
            // assim que estes testes ficaram desde sempre "passando" sem
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

/// ADR 0004: todo `put` deste backend pede SSE-S3. O teste antigo se
/// contentava com `exists()` e uma nota dizendo "confiamos que o MinIO
/// ecoa o header" — o que nao assere nada, ainda mais numa suite que nunca
/// subia o container. Agora lemos de volta o
/// `x-amz-server-side-encryption` pelo cliente cru: ou o objeto esta
/// cifrado do lado do servidor, ou o teste cai.
#[tokio::test(flavor = "multi_thread")]
async fn minio_put_enforces_sse() {
    let Some((_c, store, endpoint)) = start_minio().await else {
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

    assert!(store.exists("sse-check/file/v1").await.expect("exists"));

    let head = raw_client(&endpoint)
        .await
        .head_object()
        .bucket(BUCKET)
        .key("sse-check/file/v1")
        .send()
        .await
        .expect("raw head_object");
    assert_eq!(
        head.server_side_encryption(),
        Some(&ServerSideEncryption::Aes256),
        "objeto gravado sem SSE-S3: {:?}",
        head.server_side_encryption()
    );
}

/// ROADMAP §3.5 — native S3 multipart for files > 16 MiB.
///
/// Uploads 24 MiB (threshold + 8 MiB = exactly 3 parts of 8 MiB) through
/// `put_stream` and verifies the object round-trips byte-for-byte with a
/// matching etag and metadata.
#[tokio::test(flavor = "multi_thread")]
async fn minio_put_stream_multipart_roundtrips_large_object() {
    let Some((_c, store, endpoint)) = start_minio().await else {
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
                // Este teste cobre a mecanica do multipart, nao a politica de
                // MIME (essa e a `minio_rejects_disallowed_mime`) —
                // `application/octet-stream` nao esta no allow-list padrao
                // (ADR 0004 Security 3), entao precisa do opt-in explicito.
                allow_unsafe_mime: true,
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

    // #1229 — o servidor tem de ter VERIFICADO cada parte, nao so aceitado os
    // bytes. `GetObjectAttributes` e o unico jeito de ler de volta o que o
    // backend guardou por parte: se `put_stream_multipart` parar de declarar
    // SHA-256 em `create`/`upload_part`/`complete`, os `checksum_sha256`
    // abaixo somem e este teste cai.
    //
    // Se o MinIO desta tag nao popular esses campos, o teste TEM de falhar
    // com a resposta inteira no log — e informacao que so o CI consegue
    // produzir, e afrouxar a assercao aqui seria voltar ao verde vazio que a
    // #1230 acabou de desfazer.
    let attrs = raw_client(&endpoint)
        .await
        .get_object_attributes()
        .bucket(BUCKET)
        .key("multipart/big/v1")
        .object_attributes(ObjectAttributes::Checksum)
        .object_attributes(ObjectAttributes::ObjectParts)
        .object_attributes(ObjectAttributes::Etag)
        .send()
        .await
        .expect("get_object_attributes");

    let parts = attrs
        .object_parts()
        .unwrap_or_else(|| panic!("GetObjectAttributes sem ObjectParts: {attrs:?}"));
    assert_eq!(
        parts.total_parts_count(),
        Some(3),
        "esperadas 3 partes de 8 MiB: {attrs:?}"
    );
    assert_eq!(parts.parts().len(), 3, "partes listadas: {attrs:?}");
    for part in parts.parts() {
        let sum = part.checksum_sha256().unwrap_or_else(|| {
            panic!(
                "parte {:?} sem checksum_sha256 — o servidor nao verificou esta parte: {attrs:?}",
                part.part_number()
            )
        });
        assert!(
            !sum.is_empty(),
            "parte {:?} com checksum_sha256 vazio: {attrs:?}",
            part.part_number()
        );
    }
    let composite = attrs
        .checksum()
        .and_then(|c| c.checksum_sha256())
        .unwrap_or_else(|| panic!("objeto sem checksum SHA-256 agregado: {attrs:?}"));

    // O checksum composto do S3 (`<digest>-<n>`) e o ETag do multipart NAO
    // sao o `etag_sha256` que a crate devolve: esse e sempre o SHA-256 do
    // conteudo inteiro, calculado por nos, e ja foi conferido acima contra
    // `expected_etag`. Confundir os dois quebraria todo consumidor que trata
    // `etag_sha256` como hash do arquivo.
    assert_ne!(
        composite, meta.etag_sha256,
        "checksum composto do S3 vazou como etag_sha256: {attrs:?}"
    );
    if let Some(server_etag) = attrs.e_tag() {
        assert_ne!(
            server_etag.trim_matches('"'),
            meta.etag_sha256,
            "ETag do servidor vazou como etag_sha256: {attrs:?}"
        );
    }

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

/// #1229 — o checksum por parte tem de ser VERIFICADO pelo servidor, nao so
/// transportado.
///
/// Os testes positivos provam que um multipart honesto passa; nenhum deles
/// prova que um multipart desonesto seria barrado — e um servidor que
/// ignorasse o `x-amz-checksum-sha256` passaria em todos eles igual. Aqui
/// mandamos, pelo cliente cru, uma parte cujo checksum declarado nao bate com
/// os bytes enviados: se o `upload_part` for aceito, o checksum e decorativo e
/// a garantia de integridade do #1229 nao existe.
#[tokio::test(flavor = "multi_thread")]
async fn minio_rejects_upload_part_with_wrong_checksum() {
    let Some((_c, _store, endpoint)) = start_minio().await else {
        return;
    };
    let client = raw_client(&endpoint).await;
    let key = "multipart/checksum-errado/v1";

    let created = client
        .create_multipart_upload()
        .bucket(BUCKET)
        .key(key)
        .server_side_encryption(ServerSideEncryption::Aes256)
        .checksum_algorithm(ChecksumAlgorithm::Sha256)
        .send()
        .await
        .expect("create_multipart_upload");
    let upload_id = created
        .upload_id()
        .expect("upload_id ausente na criacao")
        .to_owned();

    // SHA-256 de 32 bytes zerados: base64 bem-formado, tamanho certo, e
    // categoricamente NAO e o digest do corpo abaixo.
    const CHECKSUM_ERRADO: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    let corpo = b"estes-bytes-nao-batem-com-o-checksum-declarado".to_vec();

    let resultado = client
        .upload_part()
        .bucket(BUCKET)
        .key(key)
        .upload_id(&upload_id)
        .part_number(1)
        .checksum_sha256(CHECKSUM_ERRADO)
        .body(aws_sdk_s3::primitives::ByteStream::from(corpo))
        .send()
        .await;

    // Aborta antes de asserir: um `panic!` aqui nao pode deixar um multipart
    // pendurado no bucket para o proximo teste tropecar.
    let _ = client
        .abort_multipart_upload()
        .bucket(BUCKET)
        .key(key)
        .upload_id(&upload_id)
        .send()
        .await;

    match resultado {
        Ok(out) => panic!(
            "MinIO ACEITOU uma parte com checksum errado — a verificacao por \
             parte do #1229 nao esta acontecendo. Resposta: {out:?}"
        ),
        Err(e) => {
            assert!(
                e.as_service_error().is_some(),
                "esperado erro de servico (o servidor recusando), veio erro de \
                 transporte/SDK: {e:?}"
            );
            // O codigo exato varia entre implementacoes S3 (BadDigest,
            // InvalidRequest, XAmzContentSHA256Mismatch...). O contrato que
            // este teste trava e "o servidor recusa"; o codigo vai para o log
            // para documentarmos o que o MinIO desta tag devolve.
            eprintln!(
                "upload_part com checksum errado recusado: code={:?} message={:?}",
                e.code(),
                e.message()
            );
        }
    }
}
