# Plan 0041 — GAR-395 tus 1.0 server slice 1 (Creation + HEAD resume)

**Status:** Em execução
**Autor:** Claude Opus 4.7 (sessão autônoma, ClaudeMaxPower + Superpowers)
**Data:** 2026-04-22 (America/New_York)
**Issues:** [GAR-395](https://linear.app/chatgpt25/issue/GAR-395)
**Branch:** `feat/0041-gar-395-tus-slice1`
**Pré-requisitos:** ADR 0004 (storage) aceito, `garraia-storage` shipped (plans 0037/0038), migration 003 (`files`) shipped (plan 0033).
**Unblocks:** slice 2 (PATCH append + ObjectStore commit) e Fase 3.5 upload flow end-to-end (attachment em chats, anexos em tasks).

---

## 1. Goal

Entregar o **primeiro slice do servidor tus 1.0** conforme [ADR 0004 §Storage](../docs/adr/0004-object-storage.md) e GAR-395, cobrindo **apenas os dois endpoints que reservam/probam o recurso** — sem ainda transportar bytes:

1. `POST /v1/uploads` — **tus 1.0 Creation extension**: cria um recurso `tus_uploads` row, responde `201 Created` com `Location: /v1/uploads/{id}`, `Tus-Resumable: 1.0.0`, `Tus-Version: 1.0.0`. Valida `Upload-Length` (bytes totais), opcional `Upload-Metadata` (base64 key/value pairs).
2. `HEAD /v1/uploads/{id}` — **tus 1.0 Resume probe**: retorna `Upload-Offset` (0 no slice 1 — bytes ainda não são aceitos), `Upload-Length`, opcional `Upload-Metadata`, `Cache-Control: no-store`, `Tus-Resumable: 1.0.0`.

**O que NÃO entra neste slice:**
- `PATCH /v1/uploads/{id}` (tus Core — upload de bytes) — fica para slice 2.
- Commit via `ObjectStore::put` quando o upload completar — slice 2.
- Expiration worker (24h cleanup) — slice 3.
- Termination extension (`DELETE /v1/uploads/{id}`) — slice 3.
- Checksum / Concatenation / Length-Deferred extensions — v2.
- Expor TUS OPTIONS endpoint (`Tus-Version`/`Tus-Extension` discovery) — slice 2.

O objetivo é estabelecer o **contrato mínimo** + o **modelo persistente** + a **isolamento multi-tenant** para que o upload real em slice 2 seja uma adição cirúrgica.

## 2. Non-goals

- Zero mudanças em `garraia-storage` (já shipped nos plans 0037/0038).
- Zero wiring de `ObjectStore` em `AppState` — o upload real só usa o store em slice 2. Reservar `object_key` no row de `tus_uploads` é suficiente aqui.
- Não expor endpoint OPTIONS (discovery) — seria noise sem PATCH shipped.
- Não aceita `Upload-Defer-Length: 1` (Length-Deferred) — slice 1 exige `Upload-Length` concreto.
- Não integra com `files`/`file_versions` — upload completo vira uma `file_versions` row em slice 2/3, não aqui.
- Não entrega UI mobile/desktop — endpoints HTTP puros.

## 3. Scope

**Arquivos novos:**

- `crates/garraia-workspace/migrations/014_tus_uploads.sql` — tabela `tus_uploads` com FORCE RLS + índices + grants para `garraia_app`.
- `crates/garraia-gateway/src/rest_v1/uploads.rs` — handlers `create_upload` + `head_upload` + tipos de request/response + helper de metadata parsing.
- `plans/0041-gar-395-tus-slice1.md` (este arquivo).

**Arquivos modificados:**

- `crates/garraia-gateway/src/rest_v1/mod.rs` — registra as novas rotas em Mode 1 (real handlers) e Modes 2/3 (`unconfigured_handler` 503).
- `crates/garraia-gateway/src/rest_v1/openapi.rs` — entries OpenAPI para os dois endpoints + schemas.
- `crates/garraia-gateway/src/rest_v1/problem.rs` — novo variante `RestError::PreconditionFailed` (412) e `RestError::PayloadTooLarge` (413) se não existirem; novo variante `RestError::UnsupportedMediaType` (415) idem.
- `plans/README.md` — entrada 0041.
- `CLAUDE.md` — menção do crate garraia-gateway ganhando `rest_v1::uploads` (slice 1 only).

Zero dependência Rust nova (usa sqlx, axum, base64, uuid já pinados no workspace).

## 4. Acceptance criteria

1. `cargo check --workspace --exclude garraia-desktop` verde.
2. `cargo fmt --check --all` verde.
3. `cargo test -p garraia-gateway --lib` verde (unit tests de metadata parsing + header validation).
4. `cargo test -p garraia-gateway --test '*'` verde (integration tests Docker-gated).
5. `POST /v1/uploads` com `Tus-Resumable: 1.0.0` + `Upload-Length: N` + JWT válido + `X-Group-Id` → **201 Created** com `Location: /v1/uploads/{uuid}` + `Tus-Resumable: 1.0.0`.
6. `POST /v1/uploads` sem `Tus-Resumable` header → **412 Precondition Failed**.
7. `POST /v1/uploads` com `Tus-Resumable: 0.2.2` (old) → **412 Precondition Failed** com `Tus-Version: 1.0.0`.
8. `POST /v1/uploads` com `Upload-Length: -1` ou `> 5368709120` (5 GiB) → **413 Payload Too Large**.
9. `POST /v1/uploads` sem `Upload-Length` nem `Upload-Defer-Length: 1` → **400 Bad Request**. (Deferred length não é suportado no slice 1.)
10. `POST /v1/uploads` sem JWT → **401 Unauthorized**.
11. `POST /v1/uploads` com JWT mas sem `X-Group-Id` ou sem membership → **403 Forbidden**.
12. `HEAD /v1/uploads/{id}` de upload existente do mesmo grupo → **200 OK** com `Upload-Offset: 0`, `Upload-Length: N`, `Cache-Control: no-store`, `Tus-Resumable: 1.0.0`.
13. `HEAD /v1/uploads/{id}` de upload de outro grupo → **404 Not Found** (nunca 403, para não vazar existência; alinhado com ADR 0004 §7).
14. `HEAD /v1/uploads/{id}` de UUID inexistente → **404 Not Found**.
15. Row em `tus_uploads` fica visível apenas para membros do grupo (policy `tus_uploads_group_isolation` + FORCE RLS).
16. Audit: sem audit row neste slice — `upload.created` fica para slice 2 quando o commit for efetivo (evita audit de intenção).
17. `@code-reviewer` APPROVE.
18. `@security-auditor` APPROVE ≥ 8.0/10.
19. CI 9/9 green.
20. Linear GAR-395 comentada com link para o PR + nota de slice 1/3.

## 5. Design rationale

### 5.1 Tabela `tus_uploads` vs reusar `files`

Opção considerada: usar `files` + `file_versions` como backing store do upload em progresso (marca `status='uploading'` até commit). Rejeitado porque:

- `files.size_bytes` é `NOT NULL` e reflete o total final; mixar rows parciais poluiria o índice.
- `file_versions.checksum_sha256` é obrigatório — não temos até todos os bytes chegarem.
- Soft-delete/rename semânticos divergem: `files.deleted_at` é para "user apagou o arquivo visível"; upload abortado é outro conceito.

Tabela dedicada `tus_uploads` mantém o modelo semântico limpo. Na slice 2 ela serve como o buffer/ledger durante o upload; na slice 3 (commit), um `file_versions` + `files` row são criados a partir do `tus_uploads` completo.

### 5.2 `object_key` allocado na criação

Cada `tus_uploads` row reserva um `object_key` determinístico:

```
{group_id}/uploads/{upload_id}/v1
```

Esse key é o mesmo formato do ADR 0004 §Key schema (`{group_id}/{folder_path}/{file_uuid}/v{N}`) com segmento fixo `uploads` no lugar de folder_path. O prefixo `uploads/` deixa explícito que o blob é transient até commit (quando vira `{group_id}/<folder>/<file_uuid>/v1`).

Em slice 1 o key é apenas persistido — nenhum byte vai ao ObjectStore ainda. Slice 2 usa esse key no PATCH para escrever no storage backend.

### 5.3 `Tus-Resumable` header é pré-requisito

RFC tus 1.0 §2.2.1: toda request/response do protocolo DEVE carregar `Tus-Resumable`. Missing/mismatch → 412. Simples precondition middleware.

### 5.4 `Upload-Length` cap em 5 GiB = bate com `files.size_bytes`

Migration 003 já impõe `files.size_bytes <= 5368709120` (5 GiB). Aplicamos o mesmo cap no `tus_uploads` para evitar reservar recursos que não poderão virar `files.version` depois.

### 5.5 Slice 1 = 0-byte reservation (Upload-Offset sempre 0)

O HEAD retorna `Upload-Offset: 0` fixo porque slice 1 não aceita PATCH. Um cliente que tente resumir imediatamente vê que 0 bytes foram gravados — semântica idempotente: ele pode começar do zero quando slice 2 vir ao ar. Clientes que precisarem de upload real hoje devem esperar slice 2 (no momento do PR este será o caveat documentado no GAR-395).

### 5.6 Error codes: 412 vs 415 vs 413

- `Tus-Resumable` missing/mismatch → **412 Precondition Failed** + `Tus-Version: 1.0.0` header.
- `Content-Type: application/offset+octet-stream` mismatch em PATCH (não relevante slice 1, mas reservo a porta) → 415.
- `Upload-Length` fora do range [1, 5 GiB] → **413 Payload Too Large**.
- `Upload-Length` ausente E `Upload-Defer-Length: 1` ausente → **400 Bad Request**.

### 5.7 RLS policy mirrors `files`

Policy `tus_uploads_group_isolation` faz exatamente o que `files_group_isolation` (migration 003) faz: `USING` + `WITH CHECK` idênticos contra `app.current_group_id`. Cross-tenant DELETE/UPDATE são filtered + blocked pelo WITH CHECK.

### 5.8 Rate-limit

`POST /v1/uploads` herda o `rate_limit_layer_authenticated` (per-user bucket via JWT `sub`). Limites iniciais: reusa `members_manage_limiter()` (20/min, burst 5) — conservador, opera como "slow-path" até o slice 2. Quando bytes começarem a fluir, limites dedicados podem ser necessários.

`HEAD /v1/uploads/{id}` NÃO é rate-limited neste slice — probe é barato e o cliente pode precisar fazer múltiplos em background.

## 6. Security review triggers

- **SEC-H isolation**: RLS `tus_uploads_group_isolation` + FORCE RLS + compound cross-group policy. Integration test com user em group_A + upload em group_B → 404.
- **SEC-M metadata parsing**: `Upload-Metadata` header é base64-encoded key-value pairs separados por vírgula. Parser DEVE rejeitar entries malformadas com 400 sem panic. Limite total 1 KB (cap defensivo contra DoS).
- **SEC-M object_key allocation**: key é construído com UUIDs do server-side — zero input do cliente entra no path. Zero traversal surface.
- **SEC-L expires_at arithmetic**: `created_at + INTERVAL '24 hours'` calculado server-side; cliente não influencia. Timezone: `timestamptz` (UTC).
- **SEC-L filename/mime_type em log**: `Upload-Metadata` pode conter PII; logs estruturados omitem via `skip`/`fields(skip = "upload_metadata")` no tracing span.
- **SEC-L Cache-Control**: HEAD response inclui `Cache-Control: no-store` para evitar vazamento via proxy cache (RFC tus §3.2).
- **SEC-L 404 vs 403 leak**: cross-tenant HEAD retorna 404 (não 403) — alinhado com ADR 0004 §7 e padrão NIST AC-6.
- **SEC-L Content-Length header**: HEAD response tus DEVE omitir `Content-Length: 0` conforme RFC 7230; Axum's default behavior é correto.
- **SEC-L tus-version advertising**: 412 response DEVE incluir `Tus-Version: 1.0.0` (os suportados).

## 7. Testing strategy

### 7.1 Unit

- `parse_upload_metadata("key1 dmFsdWUx,key2 dmFsdWUy")` → `HashMap{"key1"→"value1","key2"→"value2"}`.
- `parse_upload_metadata` rejeita chaves com espaço, valores sem base64 válido, sequências vazias.
- `parse_upload_metadata` aceita entries sem valor (`"key"` sozinho → value="").
- `parse_upload_length("42")` → 42; rejeita negativos, zero, > 5 GiB, não-numérico.
- `require_tus_resumable` OK em `1.0.0`, 412 em qualquer outro.
- `build_object_key(group_id, upload_id)` produz formato exato.

### 7.2 Integration (testcontainer pgvector/pg16 + REST harness do plan 0016)

- `post_uploads_happy_path_creates_row_and_returns_location`
- `post_uploads_without_tus_resumable_is_412`
- `post_uploads_wrong_tus_version_is_412_with_tus_version_header`
- `post_uploads_oversized_is_413`
- `post_uploads_missing_upload_length_is_400`
- `post_uploads_without_jwt_is_401`
- `post_uploads_without_group_membership_is_403`
- `head_uploads_happy_path_returns_offset_zero_and_length`
- `head_uploads_cross_group_is_404`
- `head_uploads_unknown_id_is_404`

## 8. Rollback plan

Pure additive: nova migration 014 (forward-only per CLAUDE.md regra 9; rollback usa `DROP TABLE tus_uploads`), novo módulo Rust, rotas novas. Revertível via `git revert` do commit + `DROP TABLE tus_uploads` no operador.

## 9. Risk assessment

| Risco | Severidade | Mitigação |
|---|---|---|
| Cliente mal-comportado cria milhões de `tus_uploads` rows sem nunca fazer PATCH | MÉDIO | Rate-limit per-user (20/min burst 5) + job de expiração em slice 3 apaga rows `expires_at < NOW() AND status='in_progress'`. |
| `Upload-Metadata` parsing panic em entrada malformada | BAIXO | Parser retorna `Result<HashMap,String>`; caller devolve 400 com mensagem genérica (zero eco da entrada do cliente). |
| Row órfão em `tus_uploads` após incident — nunca vira `files.version` | BAIXO | Slice 3 purge removes `status='expired'`; `ON DELETE CASCADE` no FK para `groups` garante cleanup se o grupo for deletado. |
| Schema 014 colide com futura migration paralela | BAIXO | Número 014 disponível (último é 013); sqlx migrations rodam em ordem lex. |
| Cross-tenant leak se o extractor de `X-Group-Id` falhar | ALTO | Herda a proteção já existente — membership lookup via `Principal` + RLS `tus_uploads_group_isolation`. Integration test `head_uploads_cross_group_is_404` é o guard. |

## 10. Open questions

- **Q1**: Deveria emitir audit `upload.resource_created` em slice 1? → **Não**; audit de intenção polui o log antes do byte efetivo. Slice 2 (commit) emite `upload.completed` — o sinal útil.
- **Q2**: Deveria expor `OPTIONS /v1/uploads` (tus discovery)? → **Não**; sem PATCH, discovery anuncia capabilities incompletas. Slice 2 adiciona.

## 11. Future work (slice 2+)

- **Slice 2 (PATCH)**: aceitar `PATCH /v1/uploads/{id}` com `Content-Type: application/offset+octet-stream`, append bytes, atualizar `upload_offset`. Quando `upload_offset == upload_length`, flush para `ObjectStore::put`, marca `status='completed'`, cria `files` + `file_versions` rows.
- **Slice 3 (Termination + Expiration)**: `DELETE /v1/uploads/{id}` para abortar; job periódico que marca `status='expired'` rows > 24h.
- **OPTIONS** endpoint com `Tus-Version`, `Tus-Extension: creation,creation-with-upload,termination,expiration`.
- **Upload-Defer-Length** extension para streaming (Length-Deferred).
- **Checksum extension** (SHA-256 fingerprint nos chunks).

## 12. Definition of done

- [ ] Migration 014 aplicada (smoke test em `migration_smoke.rs` valida row count incremental + RLS policy).
- [ ] Module `rest_v1/uploads.rs` implementado.
- [ ] Rotas registradas em Mode 1 + stub em Mode 2/3.
- [ ] OpenAPI inclui os 2 endpoints.
- [ ] Unit tests verdes.
- [ ] Integration tests verdes (10 cenários) quando Docker disponível; skip graceful caso contrário.
- [ ] `cargo check/clippy/fmt/test` verdes workspace-wide.
- [ ] `@code-reviewer` APPROVE.
- [ ] `@security-auditor` APPROVE ≥ 8.0/10.
- [ ] PR aberto.
- [ ] CI 9/9 green.
- [ ] PR merged.
- [ ] Linear GAR-395 comentada (slice 1/3 done).
- [ ] `CLAUDE.md` + `plans/README.md` atualizados.
- [ ] `.garra-estado.md` atualizado ao fim da sessão.
