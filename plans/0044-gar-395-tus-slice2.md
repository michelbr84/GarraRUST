# Plan 0044 — GAR-395 TUS 1.0 server slice 2 (PATCH + ObjectStore commit)

**Status:** Em execução
**Autor:** Claude Opus 4.7 (sessão autônoma, ClaudeMaxPower + Superpowers, Lote A-1)
**Data:** 2026-04-22 (America/New_York)
**Issues:** [GAR-395](https://linear.app/chatgpt25/issue/GAR-395)
**Branch:** `feat/0044-gar-395-tus-slice2`
**Worktree:** `.worktrees/0044-tus-slice2`
**Pré-requisitos:** [plan 0041](0041-gar-395-tus-slice1.md) merged (Creation + HEAD shipped, migration 014 aplicada), ADR 0004 aceito, `garraia-storage` com `LocalFs` + `S3Compatible` (plans 0037/0038) shipped, migration 003 (`files`/`file_versions`) shipped (plan 0033).
**Unblocks:** slice 3 (DELETE termination + expiration worker + streaming `put`) e `/v1/files/*` endpoints (listagem, download).

---

## 1. Goal

Entregar o **segundo slice do servidor tus 1.0** seguindo [ADR 0004 §Storage](../docs/adr/0004-object-storage.md) e GAR-395, levando o contrato do slice 1 até o **commit efetivo do blob**:

1. `PATCH /v1/uploads/{id}` — **tus 1.0 Core**. Aceita bytes via `Content-Type: application/offset+octet-stream`, valida `Upload-Offset`, persiste incrementalmente em arquivo de staging, atualiza `tus_uploads.upload_offset`. Quando `upload_offset + bytes == upload_length`, dispara commit: `ObjectStore::put(final_object_key, bytes, opts)` + inserção atômica em `files` + `file_versions` (migração 003) + audit `upload.completed` + `tus_uploads.status = 'completed'`.
2. `OPTIONS /v1/uploads` — **tus discovery**. Resposta 204 com `Tus-Version: 1.0.0`, `Tus-Resumable: 1.0.0`, `Tus-Extension: creation,creation-with-upload`, `Tus-Max-Size: 5368709120`.
3. Wiring de `ObjectStore` em `AppState` (construção via `garraia-config::StorageConfig`) expondo o backend configurado para `rest_v1::uploads`.
4. Audit event `upload.completed` (atômico com o commit) — alinhado com ADR 0005 §audit e LGPD art. 18.

**O que NÃO entra neste slice:**
- `DELETE /v1/uploads/{id}` (tus Termination extension) — slice 3.
- Worker periódico que marca `status='expired'` após 24h — slice 3.
- Streaming `ObjectStore::put` via `AsyncRead` (hoje o trait recebe `Bytes`; commit final lê o arquivo de staging para memória) — fica para slice 3 ou refactor dedicado do trait.
- Checksum extension (`Upload-Checksum` header) — v2.
- Concatenation extension — v2.
- UI mobile de retomada — separado (GAR-mobile).
- Exposição de `GET /v1/files/*` (list/download) — fora de escopo (separate issue Fase 3.5).
- Mudanças em `garraia-storage` além de consumo do trait `ObjectStore` público.

## 2. Non-goals

- Zero mudanças em `garraia-workspace` schema (migrations 003 + 014 já existem e bastam).
- Zero quebra de compat em `LocalFs` / `S3Compatible` — ambos funcionam via trait.
- Não expor um "staging object_key" público; staging é FS-only e interno ao gateway.
- Não alterar `tus_uploads` schema — já tem `upload_offset`, `status`, `object_key`, `filename`, `mime_type`.
- Não mudar rate-limit do `POST /v1/uploads` (já em `members_manage` preset); `PATCH` herda layer idêntico (revisto em §5.7).
- Não forçar MIME allow-list no PATCH (será aplicada no commit via `garraia-storage` MIME allow-list do slice 2 do plan 0038, fail-closed).
- Não introduzir nova dependência Rust no workspace; reusa `axum::body::Body`, `bytes`, `tokio::fs`, `garraia-storage`.

## 3. Scope

**Arquivos modificados:**

- `crates/garraia-gateway/src/rest_v1/uploads.rs` — novo `patch_upload` + `options_uploads` handlers; helpers de validação `Content-Type`, `Upload-Offset`, `upload_length`; função de commit `finalize_upload`.
- `crates/garraia-gateway/src/rest_v1/mod.rs` — registra `PATCH /v1/uploads/{id}` + `OPTIONS /v1/uploads` (Mode 1 real, Modes 2/3 stub 503).
- `crates/garraia-gateway/src/rest_v1/openapi.rs` — entries OpenAPI para os 2 endpoints + schemas.
- `crates/garraia-gateway/src/state.rs` — campo `pub storage: Option<Arc<dyn ObjectStore>>` em `AppState`, getter idempotente, PII-safe Debug.
- `crates/garraia-gateway/src/bootstrap.rs` — lê `StorageConfig` do `AppConfig`, instancia backend (`LocalFs` ou `S3Compatible`), wire em `AppState`.
- `crates/garraia-gateway/src/rest_v1/problem.rs` — novos variantes `RestError::Conflict` (409) e `RestError::UnsupportedMediaType` (415) caso ainda não existam.
- `crates/garraia-config/src/model.rs` — nova `StorageConfig { backend, local_fs, s3, staging_dir, max_patch_bytes }` com `Default`.
- `crates/garraia-config/src/loader.rs` — parse do novo bloco `[storage]`.
- `crates/garraia-config/src/check.rs` — validação (backend válido, paths legíveis, staging_dir writable, s3 endpoint bem-formado quando aplicável, `max_patch_bytes` em faixa).
- `crates/garraia-gateway/Cargo.toml` — feature-gate `storage-s3` passa pelo wiring (no-op se ausente; `LocalFs` continua default).
- `crates/garraia-auth/src/audit_workspace.rs` — novo variant `WorkspaceAuditAction::UploadCompleted`.
- `plans/0044-gar-395-tus-slice2.md` (este arquivo).
- `plans/README.md` — entrada 0044.
- `CLAUDE.md` — menção de slice 2 em `garraia-gateway` e `garraia-storage`.

**Arquivos novos:**

- `crates/garraia-gateway/tests/rest_v1_uploads_patch.rs` — integration tests (testcontainer pgvector + tempdir staging + LocalFs ObjectStore).

Zero dependência Rust nova. `axum::body::to_bytes` com limite, `tokio::fs::OpenOptions::append(true)` e `tokio::io::copy_buf` já disponíveis.

## 4. Acceptance criteria

1. `cargo check --workspace --exclude garraia-desktop` verde.
2. `cargo fmt --check --all` verde.
3. `cargo clippy --workspace --all-targets -- -D warnings` verde.
4. `cargo test -p garraia-gateway --lib` verde (unit tests novos).
5. `cargo test -p garraia-gateway --test 'rest_v1_uploads_patch'` verde (integration tests).
6. `PATCH /v1/uploads/{id}` com `Tus-Resumable: 1.0.0` + `Content-Type: application/offset+octet-stream` + `Upload-Offset: 0` + body válido + JWT + X-Group-Id match → **204 No Content** com `Upload-Offset` atualizado no response header e `Tus-Resumable: 1.0.0`.
7. `PATCH` com `Upload-Offset` divergente do `tus_uploads.upload_offset` atual → **409 Conflict** (tus §3.2 "If the offsets do not match, the Server MUST respond with the 409 Conflict status").
8. `PATCH` com `Content-Type` diferente de `application/offset+octet-stream` → **415 Unsupported Media Type**.
9. `PATCH` sem `Tus-Resumable` → **412 Precondition Failed** (herda middleware do slice 1).
10. `PATCH` com body que faria `upload_offset + len > upload_length` → **413 Payload Too Large**.
11. `PATCH` em upload `status='completed'` → **410 Gone**.
12. `PATCH` em upload `status='aborted'` ou `expired'` → **410 Gone**.
13. `PATCH` cross-tenant (user em group_A, upload em group_B) → **404 Not Found** (nunca 403 — ADR 0004 §7, mesmo padrão do slice 1).
14. `PATCH` com bytes que completam o upload → **204** + `Upload-Offset: <upload_length>`; na mesma transação:
    - `ObjectStore::put(final_key, bytes, opts)` executado;
    - `files` row inserido (group_id, folder_id=NULL, name, current_version=1, total_versions=1, size_bytes, mime_type, created_by, created_by_label);
    - `file_versions` row inserido (file_id, group_id, version=1, object_key, etag, checksum_sha256, integrity_hmac, size_bytes, mime_type, created_by, created_by_label);
    - `audit_events` row `action='upload.completed'` com `resource_id = files.id`;
    - `tus_uploads.status = 'completed'`.
15. Se `ObjectStore::put` falhar, **zero** mudanças em `files`/`file_versions`/`audit_events` (rollback) e `tus_uploads.status` permanece `in_progress` (o cliente pode retry) → **502 Bad Gateway** ao cliente.
16. `HEAD /v1/uploads/{id}` pós-PATCH parcial retorna o `Upload-Offset` real (não mais 0).
17. `HEAD /v1/uploads/{id}` pós-commit retorna `Upload-Offset == Upload-Length` e `Tus-Resumable: 1.0.0`; resource permanece addressable até expiration.
18. `OPTIONS /v1/uploads` → **204** com `Tus-Version: 1.0.0`, `Tus-Resumable: 1.0.0`, `Tus-Extension: creation,creation-with-upload`, `Tus-Max-Size: 5368709120`.
19. `ObjectStore::put` é chamado com `PutOptions::hmac_secret` derivado de `GARRAIA_UPLOAD_HMAC_SECRET` (fail-closed quando ausente; aviso no log se falta e storage backend é S3).
20. Ao menos 1 cenário integration verifica: upload multi-chunk (3 PATCHes de 1 KiB cada) → 204 final + files row + file_versions row + object persistido no LocalFs testcontainer.
21. `@code-reviewer` APPROVE.
22. `@security-auditor` APPROVE ≥ 8.0/10.
23. CI 9/9 green.
24. Linear GAR-395 comentada com link do PR + nota de slice 2/3.
25. `plans/README.md` + `CLAUDE.md` atualizados.

## 5. Design rationale

### 5.1 Memória vs streaming no commit

`ObjectStore::put` aceita `Bytes` — não há variante streaming hoje (trait v1 shipped em plan 0037). Slice 2 grava bytes do PATCH direto no staging file (streaming via `axum::body::Body::into_data_stream()` + `tokio::fs::File::write_all_buf`), mas no commit final lê o arquivo inteiro para memória via `tokio::fs::read(staging_path).await?` e chama `put`. Isso limita efetivamente o tamanho que o gateway aguenta commit — documentado como **cap operacional `max_patch_bytes` (default 100 MiB)** em `StorageConfig`. Clientes que precisem de uploads maiores esperam slice 3, onde o trait `ObjectStore::put_stream` será adicionado (non-goal deste slice para evitar churn em 3 backends).

Não aplicamos hard-cap global em `MAX_UPLOAD_LENGTH` (permanece 5 GiB no schema e no Creation). Aplicamos cap **apenas em slice 2** via `StorageConfig::max_patch_bytes`: se o `Upload-Length` do Creation exceder o cap, o PATCH falha com **413** no commit. Slice 3 remove o cap quando streaming put chegar.

### 5.2 Staging em FS local (independente do backend)

Todo backend (LocalFs, S3Compatible, MinIO) reutiliza o mesmo staging FS local. Motivo:

- S3-style multipart upload exige protocolo distinto por backend — slice 2 não quer divergir.
- Staging local permite `append` barato + resume com `Upload-Offset` check simples.
- Ao commitar, basta `tokio::fs::read` + `ObjectStore::put`.
- Staging path é `{staging_dir}/{upload_id}.staging` (UUID v7 → sort determinístico).

`staging_dir` vem de `StorageConfig::staging_dir` (default `{data_dir}/uploads-staging/`). Criado no boot do gateway (fail-closed se não writable).

**Limpeza:** staging file é removido em (a) commit OK, (b) `DELETE` termination (slice 3), (c) expiration worker (slice 3). Crashes do gateway deixam staging órfão; operator pode limpar manualmente até slice 3 shippar.

### 5.3 Transação DB mais commit de blob: two-phase consideration

Ordem proposta no commit final:

```
BEGIN;
  SET LOCAL app.current_user_id = ...;
  SET LOCAL app.current_group_id = ...;
  SELECT id, upload_length, upload_offset, status, object_key, filename, mime_type, created_by
    FROM tus_uploads WHERE id = $1 FOR UPDATE;
  -- assert status == 'in_progress' AND upload_offset == upload_length
  UPDATE tus_uploads SET status='completed', updated_at=NOW() WHERE id = $1;
  INSERT INTO files (...) RETURNING id;
  INSERT INTO file_versions (file_id, group_id, version=1, object_key, etag, checksum_sha256, integrity_hmac, size_bytes, mime_type, created_by, created_by_label) VALUES (...);
  INSERT INTO audit_events (...action='upload.completed'...);
  -- ObjectStore::put EXECUTA **antes** do COMMIT abaixo — ver §5.3.1.
COMMIT;
```

**§5.3.1 Two-phase commit: blob-first, row-second.**

`ObjectStore::put` é chamado **antes** do `COMMIT` da transação Postgres. Se o `put` falhar:
1. `ROLLBACK` deixa `tus_uploads.status='in_progress'` (retry possível pelo cliente);
2. Nenhum row em `files`/`file_versions`/`audit_events`;
3. Staging file permanece até nova tentativa ou expiration (slice 3).

Se o `put` tiver sucesso mas o `COMMIT` falhar (catastrófico — DB down):
1. Blob persistido mas zero referência em `files`/`file_versions`;
2. Slice 3 purge detecta via `SELECT object_key FROM file_versions WHERE object_key IS NOT NULL` e staging cleanup;
3. Alternativa futura: idempotency key no blob + reconciliação separada.

Tradeoff aceito: janela pequena onde o blob existe sem metadata é melhor do que metadata sem blob (que dá 404 ao cliente). A ordem inversa (`INSERT` antes de `put`) seria pior: metadata órfã precisa ser cleanup manual e fica exposta em listagens.

Code comment explícito no `finalize_upload` documenta a ordem e o motivo.

### 5.4 `files.folder_id = NULL` no commit inicial

Upload via tus entra em root do grupo (`folder_id = NULL` — root-level file per migration 003 `folders` CHECK). Slice futuro pode aceitar `Upload-Metadata` com `folder_id=<uuid>` para colocar em pasta específica — não neste slice.

### 5.5 `files.name` e `mime_type` derivados do `Upload-Metadata`

Slice 1 já persiste `filename` + `mime_type` em `tus_uploads` (quando o cliente envia no header `Upload-Metadata`). Commit reusa:

- `files.name ← tus_uploads.filename` (fallback `"upload-<upload_id>.bin"` se NULL).
- `files.mime_type ← tus_uploads.mime_type` (fallback `"application/octet-stream"` se NULL).
- `files.size_bytes ← tus_uploads.upload_length`.
- `files.created_by ← tus_uploads.created_by`.
- `files.created_by_label` ← lookup `users.display_name` na mesma tx.

**MIME allow-list:** antes do `put`, validamos `mime_type` contra `garraia-storage::mime_allowlist` (já disponível no plan 0038). Rejeita → **415 Unsupported Media Type** + rollback.

### 5.6 HMAC integrity obrigatório

`file_versions.integrity_hmac` é `NOT NULL` em migration 003 (regex `^[0-9a-f]{64}$`). O HMAC é calculado sobre `{object_key}:{version}:{checksum_sha256}` usando `GARRAIA_UPLOAD_HMAC_SECRET` (≥32 bytes). Fail-closed: sem secret configurado, o commit aborta com **500 Internal Server Error** e mensagem de config para o operator.

`PutOptions::hmac_secret` do `garraia-storage` plan 0038 é o path seguro. Para `LocalFs`, o HMAC também é computado (security-audit plan 0038 §Security 4).

### 5.7 Rate-limit: PATCH herda o bucket `members_manage`

Temporário até slice 3 introduzir bucket dedicado. Justificativa: `PATCH` é byte-intensive mas a proteção contra DoS do `tus_uploads` (ledger row) já está no `POST /v1/uploads`. O `PATCH` em si é limitado pelo `Upload-Length` acordado no Creation; um cliente abusivo que loopasse `PATCH` com `Upload-Offset` forjado pega 409 rápido — custo baixo para o gateway.

Slice 3 adiciona bucket `upload_bytes_limiter` por grupo (e.g., "100 MiB/min") quando o expiration worker estiver em produção.

### 5.8 `OPTIONS /v1/uploads` discovery

Handler **não autenticado** (tus §5.2 "The Server MAY respond to `OPTIONS` requests without requiring authentication"). Retorna headers fixos para clientes que descobrem capabilities antes de autenticar. Sem body (status 204). Rate-limit: herda `members_manage` leve para evitar probing massivo.

### 5.9 `PATCH` e `SET LOCAL` pair

Pattern do slice 1 preservado: `tx.begin()` + `SET LOCAL app.current_user_id/current_group_id` + query + `tx.commit()`. O `SELECT ... FOR UPDATE` serializa com outros PATCHes concorrentes no mesmo upload (qualquer request concorrente espera o lock; segunda acumulação vê o novo `upload_offset`).

### 5.10 `tus_uploads.status` state machine

```
  in_progress ──PATCH (offset match)──▶ in_progress
  in_progress ──PATCH (offset == length)──▶ completed
  in_progress ──DELETE (slice 3)──────▶ aborted
  in_progress ──expiration_worker (slice 3)──▶ expired
  completed   ──PATCH──▶ 410 Gone
  aborted     ──PATCH──▶ 410 Gone
  expired     ──PATCH──▶ 410 Gone
```

Atualização atômica: `UPDATE tus_uploads SET status='completed', updated_at=NOW() WHERE id=$1 AND status='in_progress'` + `rows_affected` check evita race entre 2 PATCHes que tentam completar.

### 5.11 `files.size_bytes` cap versus `max_patch_bytes`

Migration 003 `files.size_bytes` tem CHECK `<= 5368709120` (5 GiB). Cap operacional `max_patch_bytes` é menor (default 100 MiB) mas **não substitui** o CHECK do schema. Se operator configurar `max_patch_bytes = 200 MiB`, o schema ainda aceita; se chegar perto de 5 GiB, o fail é **antes** do `put` via `max_patch_bytes`.

## 6. Security review triggers

- **SEC-H blob-first commit ordering**: §5.3.1 documenta o tradeoff; audit verificador lê o code comment e assina.
- **SEC-H cross-tenant PATCH**: integration test com user em group_A + upload em group_B → 404 (não 403). Regression guard.
- **SEC-H HMAC fail-closed**: sem `GARRAIA_UPLOAD_HMAC_SECRET` configurado, commit aborta; não cai silencioso. Unit test cobrindo missing secret → 500.
- **SEC-M Content-Type validation**: rejeita qualquer body cujo `Content-Type ≠ application/offset+octet-stream`. Unit test.
- **SEC-M Upload-Offset race**: `SELECT ... FOR UPDATE` + `UPDATE WHERE status='in_progress' AND upload_offset=$expected` + `rows_affected` check. Integration test com 2 PATCHes concorrentes ao mesmo upload_id — só um OK (204), o outro 409.
- **SEC-M MIME allow-list**: commit rejeita `mime_type` fora da allow-list de `garraia-storage` (415 + rollback). Integration test.
- **SEC-L staging cleanup on error**: `put` falhou → staging file permanece (retry-friendly), mas logged WARN com upload_id; CR-L nit para slice 3 purgar órfãos.
- **SEC-L audit event PII**: `audit_events.metadata` **não** inclui `filename`/`mime_type` (PII potencial) — inclui só `upload_id`, `file_id`, `size_bytes`, `object_key_hash` (SHA-256 do object_key para debug ownership sem expor path).
- **SEC-L max_patch_bytes**: default 100 MiB prevê blowup de memória; doc `StorageConfig` avisa trade-off e aponta slice 3 para streaming.
- **SEC-L `OPTIONS` public**: sem JWT nem X-Group-Id; doc code comment confirma tus §5.2.
- **SEC-L staging path traversal**: `staging_dir / format!("{upload_id}.staging")` — `upload_id` é UUID (36 char `[0-9a-f-]`), impossível traversal.

## 7. Testing strategy

### 7.1 Unit

- `validate_content_type("application/offset+octet-stream")` OK; qualquer outro → 415.
- `validate_upload_offset(headers, expected=42)` OK se header `Upload-Offset: 42`; 409 em divergência.
- `max_patch_bytes_check(upload_length=200MiB, cap=100MiB)` → 413.
- `finalize_upload` computa SHA-256 + HMAC corretos (fixture de 1 KiB conhecido).
- `file_row_from_tus_upload(upload, fallback_name)` produz `files` row com defaults esperados quando `filename` é NULL.
- `object_key_reuse`: confirma que o `object_key` alocado no Creation (slice 1) é o mesmo usado no `put` + em `file_versions.object_key`.

### 7.2 Integration (testcontainer pgvector + LocalFs tempdir)

- `patch_happy_path_single_chunk_commits_files_and_file_versions`
- `patch_multi_chunk_3x1kib_commits_ok`
- `patch_offset_mismatch_returns_409`
- `patch_wrong_content_type_returns_415`
- `patch_missing_tus_resumable_returns_412`
- `patch_body_exceeds_upload_length_returns_413`
- `patch_completed_upload_returns_410`
- `patch_cross_group_returns_404`
- `patch_without_jwt_returns_401`
- `patch_concurrent_two_patches_only_one_wins_409`
- `commit_rolls_back_files_on_put_failure` (injeta `MockObjectStore` que retorna erro no `put`)
- `commit_rejects_disallowed_mime_type_returns_415`
- `options_uploads_returns_204_with_tus_headers` (não autenticado)

### 7.3 End-to-end sanity (manual — documentado no PR)

Tus CLI client (`tus-py-client`) faz upload 2 MiB → retomar após kill simulado → completar. Documenta no PR description apenas (não parte do CI).

## 8. Rollback plan

Puramente aditivo. Migration-free. Reversão: `git revert <commit>`. Dados persistidos (`files`, `file_versions`, `audit_events`) permanecem — operator pode optar por deletar via SQL se quiser limpar blobs criados durante rollout:

```sql
-- Apenas o que slice 2 criou. audit_events persiste by-design (LGPD art. 37).
DELETE FROM file_versions
WHERE file_id IN (
    SELECT fv.file_id FROM file_versions fv
    JOIN tus_uploads tu ON tu.object_key = fv.object_key
    WHERE tu.status = 'completed'
);
DELETE FROM files WHERE id IN (...);  -- mesmo subset
```

## 9. Risk assessment

| Risco | Severidade | Mitigação |
|---|---|---|
| `ObjectStore::put` OK + Postgres COMMIT falha → blob órfão | BAIXO | Slice 3 adiciona reconciliation worker; janela pequena (`put` é latência, `COMMIT` é ms); doc explícito. |
| `max_patch_bytes=100MiB` default impede uploads legítimos de `files` grandes | MÉDIO | Config override; default doc em `.env.example`; slice 3 remove via streaming. |
| `staging_dir` enche o disco (uploads órfãos acumulando) | MÉDIO | WARN log no boot se `>80%` full; slice 3 purge. Operator monitora. |
| `SELECT ... FOR UPDATE` em PATCH concorrente bloqueia indefinidamente | BAIXO | `statement_timeout` do pool + retry-friendly 409 no caller; unit test cobre. |
| Cliente envia body enorme em um único PATCH excedendo `max_patch_bytes` | BAIXO | Axum `axum::extract::DefaultBodyLimit::max(max_patch_bytes)` + streaming check. 413 early. |
| MIME type spoofing (cliente envia `text/plain` mas body é binário) | MÉDIO | MIME allow-list via lista finita; filetype sniffing (`infer` crate) fica para slice 3 — hoje confiamos no `Upload-Metadata`. Documentado. |
| HMAC secret rotation durante upload ativo | BAIXO | Secret é lido 1x no boot; rotation requer restart. Doc. |
| `Content-Length` header mentiroso no PATCH | BAIXO | `tokio::fs::write` efetivo conta bytes; `rows_affected` no UPDATE garante invariante; integration test cobre. |
| Tus client envia PATCH após `DELETE` (slice 3) | BAIXO | Slice 2 trata `status IN ('aborted','expired')` como 410. Slice 3 apenas adiciona o caminho de transição. |

## 10. Open questions

- **Q1**: Emitir audit `upload.patched` em cada PATCH parcial? → **Não**; 1 upload pode gerar milhares de PATCHes (chunking), polui audit log. `upload.completed` é o sinal útil. Slice 3 pode emitir `upload.aborted` (termination).
- **Q2**: `folder_id` do `files` criado suporta `Upload-Metadata` folder? → **Não neste slice**; default NULL (root). Slice futuro aceita.
- **Q3**: `file_versions.etag` vem de onde em `LocalFs`? → Plan 0037 define `LocalFs` etag = `hex(sha256(body))[..32]`. Usamos esse valor direto.
- **Q4**: Qual backend fica default em dev? → `LocalFs` (plan 0037) via `.env.example` `GARRAIA_STORAGE_BACKEND=local` + `GARRAIA_STORAGE_LOCAL_ROOT=${HOME}/.local/share/garraia/objects`.

## 11. Future work (slice 3+)

- **Slice 3 (Termination + Expiration + Streaming)**: `DELETE /v1/uploads/{id}` + worker `status='expired'` + trait `ObjectStore::put_stream(AsyncRead)` elimina `max_patch_bytes`.
- **Tus Checksum extension**: client envia `Upload-Checksum: sha256 <base64>` por chunk; servidor valida.
- **File type sniffing**: `infer` crate detecta MIME real antes do commit; rejeita spoof.
- **Multi-backend staging**: S3 multipart upload direto quando backend é S3 (elimina staging FS).
- **Audit `upload.patched` com throttling**: 1 audit por X chunks.

## 12. Work breakdown

| Task | Arquivo | Estimativa |
|---|---|---|
| T1 | `StorageConfig` em `garraia-config::model` + `loader` + `check` + `StorageBackend` enum + unit tests | 45 min |
| T2 | Wire `ObjectStore` em `AppState` + `bootstrap` (feature-gate s3) | 40 min |
| T3 | `WorkspaceAuditAction::UploadCompleted` + Display impl + test | 15 min |
| T4 | `patch_upload` handler + helpers (`validate_content_type`, `validate_upload_offset`, staging append stream) | 90 min |
| T5 | `finalize_upload` (PSQL tx + object_store.put + files/file_versions + audit) | 70 min |
| T6 | `options_uploads` handler | 15 min |
| T7 | Wire em `rest_v1::router` (Mode 1 real, Modes 2/3 stub) + OpenAPI | 30 min |
| T8 | Unit tests (6 cenários §7.1) | 45 min |
| T9 | Integration tests (13 cenários §7.2) | 120 min |
| T10 | CLAUDE.md + plans/README.md + `.env.example` | 20 min |
| T11 | `@code-reviewer` + `@security-auditor` pass + fix findings | 90 min |

Total estimado: ~9–10h. Executado em worktree isolado, paralelo com A-2 (plan 0045).

## 13. Definition of done

- [ ] Todos os `Acceptance criteria` §4 verdes.
- [ ] `@code-reviewer` APPROVE.
- [ ] `@security-auditor` APPROVE ≥ 8.0/10.
- [ ] CI 9/9 green.
- [ ] PR aberto com link para este plan.
- [ ] PR merged em `main`.
- [ ] Linear GAR-395 atualizada (comentário slice 2/3 done).
- [ ] `plans/README.md` entrada 0044 marcada `✅`.
- [ ] `CLAUDE.md` atualizado (menção rest_v1::uploads ganha PATCH + OPTIONS).
- [ ] `.garra-estado.md` atualizado ao fim da sessão autônoma (Lote A).
