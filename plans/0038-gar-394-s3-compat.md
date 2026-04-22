# Plan 0038 — GAR-394 slice 2: `S3Compatible` backend + SSE-S3 + MIME allowlist + HMAC integrity + real presigned URLs

**Status:** Em execução
**Autor:** Claude Opus 4.7 (sessão autônoma, ClaudeMaxPower + Superpowers)
**Data:** 2026-04-21 (America/New_York)
**Issues:** [GAR-394](https://linear.app/chatgpt25/issue/GAR-394) (parcial — slice 2 de N; slice 1 mergeado em `aa48b07`)
**Branch:** `feat/0038-gar-394-s3-compat`
**Supersedes:** — (complementa plan 0037)
**Unblocks:** GAR-395 (tus), `message_attachments` + `task_attachments` handlers no gateway, `/v1/files/*` endpoints

---

## 1. Goal

Adicionar um backend real S3-compatível ao `garraia-storage` e fechar os 4 requisitos de segurança críticos da ADR 0004:

1. **`S3Compatible`** via `aws-sdk-s3` — put/get/head/delete/exists + presigned URLs reais (TTL range [30s, 900s]) + MinIO por endpoint override.
2. **SSE-S3 obrigatório** — todo `put` envia `ServerSideEncryption::Aes256`; o bucket é assumido configurado para bloquear uploads sem SSE.
3. **MIME allow-list** (ADR 0004 §Security 3) — `put` rejeita content-types fora da lista salvo opt-in explícito `allow_unsafe_mime=true`. Aplicada ao `S3Compatible` **e** ao `LocalFs` (anti-drift).
4. **HMAC integrity** (ADR 0004 §Security 4) — quando o caller provê `hmac_secret` + `version_id` em `PutOptions`, o backend computa `HMAC-SHA256({object_key}:{version_id}:{sha256_hex})` e devolve em `ObjectMetadata.integrity_hmac`. `get_with(GetOptions{ expected_hmac, hmac_secret, version_id })` verifica constant-time antes de retornar bytes. Aplicada aos dois backends.

Feature flag `storage-s3` gateia a dependência `aws-sdk-s3` (mantém compile default leve).

## 2. Non-goals

- **Não** implementa streaming `AsyncRead`/`AsyncWrite` — `get`/`put` continuam in-memory (slice 4+).
- **Não** implementa `list`/`copy`/`set_versioning` — slice futuro.
- **Não** implementa `MinioBackend` dedicado — endpoint override do S3 já cobre (ADR 0004 §Caveats).
- **Não** liga `StorageConfig` ao `garraia-config` — slice 3 (gateway wiring). `S3Compatible::from_env()` lê vars diretas neste slice.
- **Não** toca em nenhum handler HTTP em `garraia-gateway`; zero blast radius no gateway.
- **Não** implementa retry/backoff custom — confia no default do `aws-config`.
- **Não** adiciona SSE-KMS (slice 2b+ quando produto pedir).

## 3. Scope

**Novos arquivos:**

- `crates/garraia-storage/src/s3_compat.rs` — `S3Compatible` + `S3Config` (feature `storage-s3`).
- `crates/garraia-storage/src/mime_allowlist.rs` — `is_mime_allowed(ct) -> bool` + `DEFAULT_ALLOWED` const.
- `crates/garraia-storage/src/integrity.rs` — `compute_hmac(...)` + `verify_hmac(...)` constant-time.
- `crates/garraia-storage/tests/s3_integration.rs` — MinIO testcontainer end-to-end (gated `#[cfg(feature = "storage-s3")]`).
- `plans/0038-gar-394-s3-compat.md` (este arquivo).

**Arquivos modificados:**

- `crates/garraia-storage/Cargo.toml` — novas deps (aws-sdk-s3, aws-config, hmac, subtle, base64, bytes streaming, testcontainers + modules feature `minio`); feature `storage-s3`.
- `crates/garraia-storage/src/lib.rs` — re-exports novos módulos (conditional para `s3_compat`).
- `crates/garraia-storage/src/error.rs` — novos variants: `DisallowedMime`, `TtlOutOfRange`, `SseConfigurationMissing`.
- `crates/garraia-storage/src/object_store.rs` — `PutOptions` ganha `allow_unsafe_mime`, `version_id`, `hmac_secret`; `ObjectMetadata` ganha `integrity_hmac`; novo `GetOptions` + método `get_with` (com default que chama `get`).
- `crates/garraia-storage/src/local_fs.rs` — `put` valida MIME allow-list + computa HMAC se material provido; `get_with` verifica HMAC opcional.
- `plans/README.md` — entrada 0038.
- `CLAUDE.md` — atualiza linha do `garraia-storage` para citar slice 2 entregue.

**Trait surface após slice 2:**

```rust
#[async_trait]
pub trait ObjectStore: Send + Sync + 'static {
    async fn put(&self, key: &str, bytes: Bytes, opts: PutOptions) -> Result<ObjectMetadata>;
    async fn get(&self, key: &str) -> Result<GetResult>;
    async fn get_with(&self, key: &str, opts: GetOptions) -> Result<GetResult> {
        // default calls `get` + runs HMAC verify if opts requests it
        …
    }
    async fn head(&self, key: &str) -> Result<ObjectMetadata>;
    async fn delete(&self, key: &str) -> Result<()>;
    async fn exists(&self, key: &str) -> Result<bool>;
    async fn presign_put(&self, key: &str, ttl: Duration) -> Result<Url>;
    async fn presign_get(&self, key: &str, ttl: Duration) -> Result<Url>;
}
```

## 4. Acceptance criteria

1. `cargo check -p garraia-storage` verde (sem feature).
2. `cargo check -p garraia-storage --features storage-s3` verde.
3. `cargo clippy -p garraia-storage --all-targets --all-features -- -D warnings` verde.
4. `cargo fmt --check` verde.
5. `cargo test -p garraia-storage` verde (unit tests LocalFs + MIME + HMAC).
6. `cargo test -p garraia-storage --features storage-s3` verde (integration MinIO testcontainer quando docker disponível; skip-with-note caso contrário).
7. MIME allowlist: 11 content-types listados em ADR 0004 §Security 3 aceitos; 1 fora da lista (`application/x-msdownload`) rejeitado; `allow_unsafe_mime=true` libera.
8. HMAC: `put(.., hmac_secret, version_id)` devolve `integrity_hmac` em `ObjectMetadata`. Duas `put` do mesmo blob com `version_id` distinto → HMACs distintos. `get_with(.., expected_hmac)` verifica constant-time e retorna `StorageError::IntegrityMismatch` em divergência.
9. SSE-S3: MinIO testcontainer com bucket SSE enforcement; upload sem SSE rejeitado em put (check via HEAD `ServerSideEncryption`).
10. Presigned URLs: `ttl < 30s` → `TtlOutOfRange`; `ttl > 900s` → `TtlOutOfRange`; `ttl = 300s` → URL válida (roundtrip HTTP `PUT → GET` no testcontainer).
11. `@code-reviewer` APPROVE.
12. `@security-auditor` APPROVE ≥ 8.5/10.
13. CI 9/9 green.
14. Linear GAR-394 comentada com status slice 2 + link PR (issue permanece aberta — ainda faltam slices 3+ para gateway wiring).

## 5. Design rationale

### 5.1 Feature flag `storage-s3`

`aws-sdk-s3` puxa ~40 crates transitivas (hyper-rustls, h2, etc.). Manter default off preserva o baseline enxuto do slice 1. Consumer (`garraia-gateway` slice 3) habilita via `features = ["storage-s3"]`.

### 5.2 MIME allowlist em ambos os backends

A lista da ADR 0004 §3 é uma política de conteúdo do produto — não é característica de backend. Se ficasse só no `S3Compatible`, um dev que rodasse `LocalFs` em prod poderia aceitar SVG+HTML polyglot. Pôr a validação no trait layer (via `PutOptions::allow_unsafe_mime` default false) garante fail-closed consistente. A lista canônica vive em `mime_allowlist.rs` como constante tipada.

### 5.3 HMAC via `PutOptions`/`GetOptions` em vez de wrapper backend

Uma alternativa era `HmacIntegrityStore<S: ObjectStore>` envolvendo outro store. Porém:
- Duplica o trait surface sem add feature nova.
- Exige que o caller conheça a chave HMAC em **todas** as chamadas, mesmo para `head`/`exists` que não precisam.
- Fica redundante com `LocalFs` que também precisa da lógica.

Optei por `PutOptions::hmac_secret: Option<Vec<u8>>` + `version_id: Option<String>`. Quando ambos presentes, o backend computa `HMAC-SHA256({key}:{version}:{sha256_hex})` e devolve em `ObjectMetadata.integrity_hmac`. O caller (gateway) persiste em `file_versions.integrity_hmac` (migration 003) e compara no `get_with` recuperando a chave do servidor.

### 5.4 TTL range [30s, 900s]

- 900s = 15 min = teto ADR 0004 §Security 1.
- 30s = piso ADR 0004 §Security 10 (evita falha silenciosa de token expirado antes do upload começar em mobile flaky).

### 5.5 SSE-S3 obrigatório, não opcional

ADR 0004 §Security 2 é categórico. Hardcodar `ServerSideEncryption::Aes256` em todo put elimina a chance de drift — se um dev desligar, o teste integration rejeita. Escape hatch para SSE-KMS vira slice 2b quando produto pedir (hoje ninguém pede).

### 5.6 `get_with` com default, `get` preservado

Backcompat: todo código que usa `store.get(key)` continua funcionando. `get_with` é opt-in e só adiciona custo quando o caller pede HMAC verify. Backends novos podem override `get_with` para implementação mais eficiente (S3 pode ler HMAC do response metadata num futuro slice).

### 5.7 `base64 = "0.22"`

S3 SDK espera `checksum_sha256` em base64 standard (com padding). `hex` → `base64::Engine` via `GeneralPurpose::STANDARD`. Dep já estava transitivamente presente.

## 6. Testing strategy

- **Unit tests** em `mime_allowlist`: table-driven com 15+ entradas (permitidos/rejeitados).
- **Unit tests** em `integrity`: HMAC determinístico, verify constant-time, tampering detection.
- **Unit tests** em `local_fs`: put/get com HMAC material → metadata.integrity_hmac ≠ None; put sem material → None; get_with expected_hmac divergente → IntegrityMismatch; MIME disallowed rejeitado; allow_unsafe_mime libera.
- **Integration test** `tests/s3_integration.rs` (feature `storage-s3`, skip se docker indisponível):
  - `testcontainers-modules::minio` container ephemeral.
  - put → head → get → delete roundtrip.
  - SSE header presente em head_object response.
  - presign_put URL → HTTP PUT → presign_get URL → HTTP GET → bytes idênticos.
  - presign com ttl=0s → TtlOutOfRange; ttl=30min → TtlOutOfRange.
  - MIME disallowed → DisallowedMime.
  - HMAC material → metadata.integrity_hmac preenchido; verify com chave trocada → IntegrityMismatch.

## 7. Security review triggers

- **SEC-H constant-time HMAC verify**: `subtle::ConstantTimeEq` (não `==` byte-a-byte). Evita timing oracle em adversário que observe latency.
- **SEC-H presigned URL TTL enforcement**: rejeição deve ser **pre-call** (antes de emitir a URL); um URL com TTL absurdo não pode vazar pelo SDK.
- **SEC-H SSE enforcement**: em `S3Compatible::put`, `ServerSideEncryption` é hardcoded, não configurável. Qualquer mudança futura passa por review explícito.
- **SEC-M MIME bypass via `allow_unsafe_mime`**: ADR 0004 §3 obriga audit log. Slice 2 não liga com audit_events (gateway slice 3 faz isso), mas o WARN em `tracing` fica registrado em `put` com `target: "garraia_storage::mime"`.
- **SEC-M HMAC secret lifetime**: `PutOptions::hmac_secret: Vec<u8>` é dropado ao fim do método; não é persistido. Documentar no rustdoc que caller deve usar `zeroize` se relevante.
- **SEC-M aws-sdk credentials**: o slice não toca em creds — confia em `aws-config::from_env()` (IAM role > env vars > profile). Documentar em rustdoc de `S3Config::from_env`.
- **SEC-L log redaction**: `PutOptions::hmac_secret` + `version_id` não podem vazar em `tracing::Debug`. Custom `Debug` impl em `PutOptions` redigindo esses campos.
- **SEC-L TtlOutOfRange error message**: incluir o TTL solicitado + limites permitidos; não é sensitive.

## 8. Rollback plan

Reversível via `git revert <merge-commit>`. Nenhum schema mudou; nenhum handler existente mudou. O crate reverte para o estado do slice 1 (só `LocalFs`). Consumers que habilitarem `storage-s3` já neste mesmo slice perdem o backend mas não quebram compile (feature flag default off).

## 9. Risk assessment

| Risco | Severidade | Mitigação |
|---|---|---|
| aws-sdk-s3 version bump futuro quebra API | MEDIUM | Pin minor version (`aws-sdk-s3 = "~1.50"`); CI cargo-audit (plan 0026) detecta CVE. |
| MIME allowlist falso-positivo (legit MIME bloqueado) | MEDIUM | `allow_unsafe_mime=true` escape hatch documentado + config override via `storage.allowed_mime_types` slice 3. |
| HMAC secret hard-coded em código dev | HIGH | Rustdoc de `PutOptions::hmac_secret` cita `GARRAIA_STORAGE_HMAC_SECRET` obrigatório em prod; testes usam chave ephemeral. |
| SSE-S3 bucket não configurado → silent accept pela AWS mesmo com header ausente | MEDIUM | Integration test verifica `ServerSideEncryption::Aes256` no head_object; docs recomendam bucket policy `Condition: StringEquals: s3:x-amz-server-side-encryption: AES256`. |
| MinIO testcontainer instável em CI | LOW | Test gated `storage-s3` feature; CI roda sem. Slice 3 (gateway wiring) adiciona ao CI matrix. |
| aws-sdk async runtime conflita com tokio | LOW | aws-sdk usa tokio default; sem custom executor. |

## 10. Open questions

Nenhuma bloqueante. Abertas para follow-up:

- **Q1**: Incluir integration test no CI workflow (precisa habilitar `storage-s3` feature no matrix)? → Proposta: slice 3 (gateway wiring) adiciona; por ora, integration test roda local.
- **Q2**: `StorageConfig` vai para `garraia-config` ou fica em `garraia-storage::S3Config`? → Decisão: fica em `garraia-storage` por ora; slice 3 migra para `garraia-config` ao fazer wiring.

## 11. Future work

- **Slice 2b**: SSE-KMS opt-in via `S3Config::kms_key_id` + docs em `docs/storage.md`.
- **Slice 3 (GAR-394 wiring)**: `garraia-config::StorageConfig` + `garraia-gateway` usage em endpoints `/v1/files/*`.
- **Slice 4 (GAR-395)**: tus.io resumable em cima do trait.
- **Slice 5**: streaming `AsyncRead`/`AsyncWrite` (breaking change de `put`/`get` signatures).
- **Slice 6**: `list(prefix)` + `copy(src, dst)` + `set_versioning(bool)`.

## 12. Definition of done

- [x] Plan mergeado (este arquivo).
- [ ] Módulo `mime_allowlist` + 11 allowed + 4 rejected tests.
- [ ] Módulo `integrity` + HMAC compute/verify tests.
- [ ] `PutOptions` + `GetOptions` + `ObjectMetadata` estendidos.
- [ ] `LocalFs::put` enforça MIME allowlist + computa HMAC.
- [ ] `LocalFs::get_with` verifica HMAC opcional.
- [ ] `S3Compatible` impl completa atrás de feature `storage-s3`.
- [ ] Integration test MinIO testcontainer passando (local).
- [ ] `cargo check`/`clippy`/`fmt`/`test` verdes sem e com feature.
- [ ] `@code-reviewer` APPROVE.
- [ ] `@security-auditor` APPROVE ≥ 8.5/10.
- [ ] PR aberto.
- [ ] CI 9/9 green.
- [ ] PR merged.
- [ ] Linear GAR-394 comentada (slice 2 done, issue continua aberta).
- [ ] `CLAUDE.md` + `plans/README.md` atualizados.
- [ ] `.garra-estado.md` atualizado ao fim da sessão.
