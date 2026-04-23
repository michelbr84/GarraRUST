# Plan 0047 — GAR-395 TUS slice 3 (Termination + expiration worker + streaming put)

**Status:** 🟡 Em execução (Lote B — implementação primeiro, plano narrow por decisão do usuário 2026-04-23)
**Autor:** Claude Opus 4.7 (sessão autônoma, ClaudeMaxPower + Superpowers)
**Data:** 2026-04-23 (America/New_York)
**Issue:** [GAR-395](https://linear.app/chatgpt25/issue/GAR-395) — slice 3 de 3
**Branch:** `plan/0047-tus-slice3`
**Pré-requisitos:** plan 0049 (clippy strict) merged (5716976); slice 1 (0041) + slice 2 (0044) in main.
**Escopo narrow por decisão do usuário:** "Pode deixar MIME sniff, bucket bytes rate-limit e HMAC zeroize para commits seguintes do mesmo slice, mas quero o slice já em andamento agora com código real."

## 1. Goal

Fechar GAR-395 entregando os 3 deferidos do PR #59 (plan 0044): **DELETE termination**, **expiration worker**, **streaming `put_stream`** — na mesma branch, com base de testes cobrindo os fluxos novos.

## 2. Non-goals deste primeiro commit

- MIME sniffing (`infer` crate) — próximo commit do mesmo slice.
- Bucket bytes rate-limiter — próximo commit.
- HMAC secret `Zeroize<Vec<u8>>` — próximo commit.
- Substituição do `to_bytes` buffered em `patch_upload` por streaming — próximo commit (mudança de fluxo no PATCH handler + reescrita do staging append).

## 3. Scope (arquivos tocados neste commit)

**Novos:**
- `crates/garraia-gateway/src/uploads_worker.rs` — worker + `UploadsExpirationWorkerConfig` + `TickReport` + `run_expiration_tick` + `spawn_uploads_expiration_worker` + 3 unit tests puros.
- `crates/garraia-gateway/src/uploads_worker_util.rs` — `sha256_hex_of` helper compartilhado + 3 unit tests (known-vector NIST, idempotência, divergência).

**Modificados:**
- `crates/garraia-auth/src/audit_workspace.rs` — 2 novas variantes `UploadTerminated` + `UploadExpired` com mappings `upload.terminated` / `upload.expired`.
- `crates/garraia-storage/src/object_store.rs` — trait `put_stream(&self, key, reader: AsyncByteReader, content_length, opts)` com default impl buffered + novo `pub type AsyncByteReader = Pin<Box<dyn AsyncRead + Send>>`.
- `crates/garraia-storage/src/local_fs.rs` — override `put_stream` com temp-sibling + atomic rename + hash streaming (sem spike de memória) + 3 unit tests (happy, zero-byte vector NIST, overwrite atomicidade).
- `crates/garraia-storage/src/lib.rs` — re-export `AsyncByteReader`.
- `crates/garraia-gateway/src/lib.rs` — `pub mod uploads_worker; pub mod uploads_worker_util;`.
- `crates/garraia-gateway/src/rest_v1/uploads.rs` — novo handler `delete_upload` (tus 1.0 Termination extension); constante `TUS_EXTENSION_HEADER` bumped de `"creation"` para `"creation,termination"`.
- `crates/garraia-gateway/src/rest_v1/mod.rs` — rota DELETE em `/v1/uploads/{id}` em todos os 3 modes (full / auth-only / no-auth).
- `crates/garraia-gateway/src/server.rs` — spawn do worker após `build_storage_wiring` quando AppPool presente.

## 4. Acceptance criteria (deste commit)

1. `cargo fmt --check --all` → exit 0.
2. `cargo clippy --workspace --exclude garraia-desktop --all-targets -- -D warnings` → exit 0 (régua nova do 0049).
3. `cargo test -p garraia-storage --lib` → 62/62 green (inclui 3 novos tests de `put_stream`).
4. `cargo test -p garraia-auth --lib audit_workspace` → 3/3 green (existentes continuam válidos com 2 variantes novas).
5. `cargo test -p garraia-gateway --lib uploads_worker` → 6/6 green (3 worker + 3 util).
6. DELETE handler semânticas:
   - 204 em `in_progress` → `aborted` + audit `upload.terminated` na mesma tx + staging best-effort cleanup.
   - 410 em `completed`/`aborted`/`expired` (idempotência de termination).
   - 404 quando row não existe OU cross-group (anti-enumeration per ADR 0004 §7).
   - 412 sem `Tus-Resumable: 1.0.0`.
7. Worker guarded por `pg_try_advisory_lock(hashtext('tus_uploads_expiration'))` — múltiplas réplicas não double-purge.

## 5. Non-scope (fica pros commits seguintes deste slice)

- MIME sniffing (SEC-L plan 0044 follow-up).
- Bucket bytes rate-limiter `upload_bytes_limiter` (SEC-M1 plan 0044).
- Zeroize HMAC secret.
- Integration tests full-stack (testcontainer pg + handlers DELETE + worker tick end-to-end) — demandam reorganização do test harness de `garraia-gateway`; vão em commit 2.
- Substituição do `to_bytes` buffered em `patch_upload` por streaming real (o `put_stream` já existe e é roundtripável; integração no fluxo do PATCH precisa reescrever o staging append + `finalize_upload` para evitar segundo spike).

## 6. Rollback plan

- Reversível por `git revert` — nenhuma migration nova.
- Worker parado ao reverter (spawn deixa de rodar); DB fica em estado pré-slice (rows `in_progress` não viram `expired` automaticamente, mas `expires_at` já estava sendo escrito desde slice 1 — re-habilitar é só re-mergear).
- DELETE handler sumido → clientes tus re-tentam com 404/405 padrão Axum; não há dado novo no schema.

## 7. Follow-ups conhecidos (mesmo slice, próximos commits da branch)

1. `rest_v1::uploads::patch_upload` passa a consumir via `put_stream` (elimina cap `max_patch_bytes` para uploads > 100 MiB).
2. `infer`-based MIME sniff feature-gated (`storage-mime-sniff`).
3. `upload_bytes_limiter` — rate-limit por bytes, não por request.
4. `Zeroize<Vec<u8>>` para HMAC material.
5. Integration tests testcontainer pg + MinIO.

## 8. Referências

- Plan 0041 (slice 1) — `POST /v1/uploads` + `HEAD /v1/uploads/{id}`.
- Plan 0044 (slice 2) — `PATCH /v1/uploads/{id}` + `OPTIONS` + ObjectStore commit.
- ADR 0004 — Object storage (§7 404-vs-403 anti-enumeration).
- CLAUDE.md regra #9 — forward-only migrations (este slice NÃO adiciona migration).
