# Plan 0055 — GAR-WS-CHAT Slice 2: `POST` + `GET /v1/chats/{chat_id}/messages`

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Linear issue:** [GAR-507](https://linear.app/chatgpt25/issue/GAR-507) — "REST /v1 chats slice 2: POST + GET /v1/chats/{chat_id}/messages" (In Progress, High). Labels: `epic:ws-chat`, `epic:ws-api`. Project: "Fase 3 — Group Workspace".

**Status:** ⏳ Draft — aprovado 2026-05-05 (Florida). Pré-requisitos validados neste plan §"Validações pré-plano".

**Goal:** Entregar o segundo par de endpoints de chat — `POST /v1/chats/{chat_id}/messages` (envia uma mensagem ao chat, resolving `sender_label` de `users.display_name`, auto-derivando `group_id` via compound FK) e `GET /v1/chats/{chat_id}/messages?after=<uuid>&limit=<n>` (cursor-paginated list por `(created_at DESC, id DESC)`, default 50, max 100, excluindo soft-deleted). Reutiliza 100% da fundação dos plans 0016/0017/0019/0020/0054 (AppPool + `Principal` + `RequirePermission` + audit + `WorkspaceAuditAction`). Zero migration nova, zero ADR novo, zero capability nova — `Action::{ChatsRead,ChatsWrite}` já existem.

**Architecture:**

1. Novo módulo `crates/garraia-gateway/src/rest_v1/messages.rs` (~350 LOC), espelhando `chats.rs` no shape: DTOs `SendMessageRequest`, `MessageResponse`, `MessageListResponse`, `MessageSummary` + handlers `send_message`, `list_messages` + módulo `tests` interno com unit tests dos validators.
2. **`POST /v1/chats/{chat_id}/messages`** — happy path:
   - `Principal` extractor 403'a non-members. Handler valida `principal.group_id == header`.
   - `can(&principal, Action::ChatsWrite)`.
   - `body.validate()` — body não-vazio, len ≤ 100_000 chars (espelha CHECK DB), reply_to_id ignorado se fornecido (aceito, não validado além de UUID format).
   - `tx = pool.begin()` → SET LOCAL user+group → SELECT `chats.group_id` WHERE `id = chat_id AND group_id = principal.group_id` → 404 se 0 rows → SELECT `users.display_name` WHERE `id = principal.user_id` → INSERT messages RETURNING id, created_at → audit `message.sent` com `{ body_len, has_reply_to }` → COMMIT → 201.
3. **`GET /v1/chats/{chat_id}/messages?after=<uuid>&limit=<n>`** — happy path:
   - Validação header/path + extractor.
   - `can(&principal, Action::ChatsRead)`.
   - `tx = pool.begin()` → SET LOCAL → SELECT `chats.group_id` (404 se ausente) → SELECT paginated via keyset `(created_at, id)` → COMMIT → 200.
   - Keyset cursor: `after=<last_message_id>` → `WHERE (created_at, id) < (SELECT created_at, id FROM messages WHERE id = $after)`. `limit` default 50, max 100. `next_cursor` = id da última item se `items.len() == limit`.
4. **`WorkspaceAuditAction::MessageSent`** novo variant. Metadata: `{ body_len, has_reply_to }` ONLY — body content é PII.
5. **OpenAPI**: registrar 2 paths + 4 schemas em `openapi.rs`.
6. **Router**: 2 rotas novas em todos os 3 modes.
7. **Tests**: `tests/rest_v1_messages.rs` (all scenarios bundled em 1 `#[tokio::test]`) + extensão de `authz_http_matrix.rs` com 6 novos casos.

**Tech stack:** Axum 0.8, `utoipa 5`, `garraia-auth::{Action, Principal, can, WorkspaceAuditAction, audit_workspace_event}`, `sqlx 0.8` (postgres), `serde 1.0`, `chrono 0.4`, `uuid 1.x`. Test harness: `testcontainers` + `pgvector/pgvector:pg16`.

---

## Design invariants (não-negociáveis deste slice)

1. **`messages` está sob FORCE RLS — `app.current_group_id` é obrigatório.** Policy `messages_group_isolation` (migration 007:83-87) usa `group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid`. Esquecer `SET LOCAL app.current_group_id` causa `permission denied for relation messages` (SQLSTATE 42501).
2. **`messages.group_id` é denormalizado de `chats.group_id` — compound FK.** Handler DEVE fazer SELECT em `chats` para obter `group_id` antes do INSERT em `messages`. Compound FK `(chat_id, group_id) REFERENCES chats(id, group_id)` enforce consistência no DB. Group_id NÃO vem do body (seria injeção silenciosa de cross-tenant).
3. **`messages.body_tsv` é `GENERATED ALWAYS AS` — NUNCA incluir na lista de colunas do INSERT.** Postgres mantém automaticamente. Incluir causaria `ERROR: column "body_tsv" can only be updated to DEFAULT`.
4. **`sender_label` = `users.display_name` resolvido DENTRO da mesma transaction.** Evita race onde o user é deletado entre o lookup e o INSERT. Display name no momento do envio é a fonte autorizada (erasure-survival path documentado no schema comment).
5. **Audit metadata: `{ body_len, has_reply_to }` ONLY.** `messages.body` é user-controlled e pode conter PII (secrets, nomes pessoais, dados médicos). NUNCA incluir `body` literal em `audit_events.metadata`. `body_len` é estrutural e PII-safe.
6. **Cursor = `after=<last_message_uuid>`.** Keyset por `(created_at DESC, id DESC)` para ordenação estável (mesmo `created_at` com UUIDs diferentes não causa duplicates/gaps). `id` como tie-breaker porque é monotônico dentro de um microsegundo pelo `gen_random_uuid()` + timing.
7. **Chat ownership validation = SELECT dentro da tx.** `chat_id` no path não garante que o chat pertence ao `principal.group_id`. Handler DEVE fazer `SELECT group_id FROM chats WHERE id = $chat_id AND group_id = $principal_group_id` dentro da tx RLS. 0 rows → 404 (não 403 — esconde existência de chats de outros grupos, cf. ADR 0004 §7 pattern já usado em uploads).
8. **`X-Group-Id` header obrigatório.** Mesmo pattern de `create_chat`/`list_chats` e endpoints anteriores.
9. **Soft-deleted messages excluídos do GET.** `WHERE deleted_at IS NULL` no SELECT. Não expor mensagens deletadas nesta slice.

---

## Validações pré-plano (gate executado nesta sessão)

- ✅ `Action::ChatsRead` existe — `crates/garraia-auth/src/action.rs:52`.
- ✅ `Action::ChatsWrite` existe — `crates/garraia-auth/src/action.rs:53`.
- ✅ `messages` schema completo em `migrations/004_chats_and_messages.sql:57-98`. Colunas: `id, chat_id, group_id, sender_user_id, sender_label, body, body_tsv GENERATED, reply_to_id, thread_id, created_at, edited_at, deleted_at`. Compound FK `(chat_id, group_id) REFERENCES chats(id, group_id)`.
- ✅ `messages` sob FORCE RLS em `migrations/007_row_level_security.sql:80-87`. Policy `messages_group_isolation`: direct via `group_id = NULLIF(...)::uuid`.
- ✅ `messages_chat_created_idx ON messages(chat_id, created_at DESC) WHERE deleted_at IS NULL` — suporta keyset pagination.
- ✅ `WorkspaceAuditAction` enum + `audit_workspace_event` helper em `crates/garraia-auth/src/audit_workspace.rs`. Padrão de adição de variant documentado em plan 0054.
- ✅ `users.display_name` existe (migration 001) — resolvível via SELECT na mesma tx.
- ✅ Harness `tests/common/` com `seed_user_with_group` cobre fixtures necessários.
- ✅ `chats.rs` pattern idêntico ao que usaremos — serve de fonte do copy estrutural.
- ⚠️ `messages.body CHECK (length(body) BETWEEN 1 AND 100000)` — usar `length()` que em Postgres retorna character count para text (equivalente a chars().count() em Rust).

---

## Out of scope (rejeitado explicitamente)

- `POST /v1/messages/{id}/threads` — slice 3 (plan 0056).
- WebSocket `/v1/chats/{id}/stream` — slice 4.
- Message editing (`PATCH /v1/messages/{id}`) — slice futuro.
- Message deletion (`DELETE /v1/messages/{id}`) — slice futuro.
- FTS search (`/v1/search`) — plan separado.
- `reply_to_id` validation além de UUID format (aceitar mas não verificar se id existe).
- DM creation (`type = 'dm'`) — ainda na slice 2 não commitada.
- `thread_id` — slice 3.
- `cursor` com timestamp-based (só UUID-based nesta slice).
- Attachments.

---

## Rollback plan

Aditivo por task. Cada task é commit independente; `git revert` por commit desfaz limpo:

- Task 0 (plano + README) — revert remove a linha do README.
- Task 1 (`WorkspaceAuditAction::MessageSent`) — revert remove o variant + roll-back dos 3 test arrays.
- Task 2 (módulo `messages.rs` DTOs + unit tests) — revert deleta o arquivo novo.
- Task 3 (handlers `send_message` + `list_messages`) — revert volta ao state pós-Task 2.
- Task 4 (rotas em `mod.rs`) — revert volta o router ao state pré-slice.
- Task 5 (OpenAPI registration) — revert remove os 4 schemas + 2 paths.
- Task 6 (integration tests `rest_v1_messages.rs`) — revert deleta o arquivo de test.
- Task 7 (extensão de `authz_http_matrix`) — revert volta a matriz de N+6 para N cenários.

Zero migration, zero novo role, zero ADR, zero capability nova. Rollback é 7 git reverts sequenciais.

---

## §12 Open questions (respondidas antes do start)

1. **`group_id` vem do path ou do body?** → **Do principal.group_id** (via `X-Group-Id` header). Validado contra `chats.group_id` via SELECT dentro da tx.
2. **`reply_to_id` é validado (FK para messages)?** → **Não nesta slice.** Aceitar no body como `Option<Uuid>`, inserir no INSERT diretamente. O FK `reply_to_id REFERENCES messages(id) ON DELETE SET NULL` é enforced pelo DB; se o id não existir, o DB retorna um FK violation que o handler converte em 400.
3. **`limit` default e max?** → **default 50, max 100.** Se `limit > 100`, clamp para 100 (não rejeitar, apenas limitar). Se `limit < 1`, rejeitar com 400.
4. **`next_cursor` quando lista tem exatamente `limit` items?** → **`Some(last_item.id)`**. Client pode usar isso para próxima página — mesmo que a página seguinte seja vazia. Simples e determinístico.
5. **Chat sem mensagens retorna 200 com `items: []`?** → **Sim.**
6. **Mensagem com body > 100_000 chars → rejeitar no handler ou deixar para o DB?** → **Rejeitar no handler** (400 antes de tocar o DB). Menos overhead, melhor UX. Usa `chars().count()` para consistência com `body_tsv` generation (PostgreSQL `length()` para text = chars não bytes).

---

## File Structure

**Criar:**
- `crates/garraia-gateway/src/rest_v1/messages.rs` — DTOs + handlers + unit tests
- `crates/garraia-gateway/tests/rest_v1_messages.rs` — integration tests

**Modificar:**
- `crates/garraia-auth/src/audit_workspace.rs` — variant `MessageSent` + 3 testes
- `crates/garraia-gateway/src/rest_v1/mod.rs` — `pub mod messages;` + 2 rotas novas
- `crates/garraia-gateway/src/rest_v1/openapi.rs` — 2 paths + 4 schemas
- `crates/garraia-gateway/tests/authz_http_matrix.rs` — 6 novos cenários
- `plans/README.md` — registrar linha 0055
- `ROADMAP.md` — marcar `[x]` em §3.4 Chats quando o PR mergear

**NÃO tocar:** migrations, `garraia-auth/src/action.rs`/`role.rs`/`can.rs`, `garraia-workspace`, `CLAUDE.md`, `docs/adr/*`, `chats.rs`, plans 0010..0054.

---

## M1 — Slice 2 completo

### Task 0: Registrar 0055 no índice

- [x] Adicionar linha na tabela Index de `plans/README.md`
- [x] Commit `docs(plans): register plan 0055 (GAR-507 chats slice 2 — messages)`

---

### Task 1: `WorkspaceAuditAction::MessageSent` + testes

**Files:** `crates/garraia-auth/src/audit_workspace.rs`

- [ ] Atualizar os 3 testes (TDD red): adicionar asserções para `MessageSent` em `workspace_audit_action_as_str_stable`, `workspace_audit_action_distinct_strings`, `workspace_audit_action_display_delegates_to_as_str`
- [ ] `cargo test -p garraia-auth --lib audit_workspace::tests` → deve falhar
- [ ] Adicionar variant `MessageSent` + match arm `"message.sent"`
- [ ] `cargo test -p garraia-auth --lib audit_workspace::tests` → deve passar
- [ ] Commit `feat(auth): add WorkspaceAuditAction::MessageSent (plan 0055 t1)`

---

### Task 2: Módulo `messages.rs` — DTOs + unit tests

**Files:** Create `crates/garraia-gateway/src/rest_v1/messages.rs`

DTOs:
- `SendMessageRequest { body: String, reply_to_id: Option<Uuid> }` — `#[serde(deny_unknown_fields)]`
- `SendMessageRequest::validate()` — body não-vazio + len ≤ 100_000 chars
- `MessageResponse { id, chat_id, group_id, sender_user_id, sender_label, body, reply_to_id, created_at }`
- `MessageSummary { id, chat_id, sender_user_id, sender_label, body, reply_to_id, created_at }`
- `MessageListResponse { items: Vec<MessageSummary>, next_cursor: Option<Uuid> }`

Unit tests (5):
- `send_message_request_valid`
- `send_message_request_rejects_empty_body`
- `send_message_request_rejects_whitespace_body`
- `send_message_request_rejects_body_over_100k_chars`
- `send_message_request_accepts_body_at_100k_chars`

- [ ] Escrever DTOs + unit tests
- [ ] `cargo test -p garraia-gateway --lib rest_v1::messages::tests` → verde
- [ ] Adicionar `pub mod messages;` em `rest_v1/mod.rs`
- [ ] `cargo check -p garraia-gateway`
- [ ] Commit `feat(gateway): add messages DTOs + validation (plan 0055 t2)`

---

### Task 3: Handlers `send_message` + `list_messages`

**Files:** Modify `crates/garraia-gateway/src/rest_v1/messages.rs`

`send_message`:
1. Header/path coherence (same as chats.rs)
2. `can(&principal, Action::ChatsWrite)`
3. `body.validate()`
4. `pool.begin()` → SET LOCAL user+group
5. SELECT `group_id FROM chats WHERE id = $chat_id AND group_id = $principal_group_id` → 404 se 0 rows
6. SELECT `display_name FROM users WHERE id = $principal.user_id`
7. INSERT messages (id, chat_id, group_id, sender_user_id, sender_label, body, reply_to_id) RETURNING id, created_at
8. `audit_workspace_event(MessageSent, ..., "messages", msg_id.to_string(), json!({ "body_len": ..., "has_reply_to": ... }))`
9. COMMIT → 201

`list_messages`:
1. Header/path coherence
2. `can(&principal, Action::ChatsRead)`
3. `pool.begin()` → SET LOCAL
4. SELECT `group_id FROM chats WHERE id = $chat_id AND group_id = $principal_group_id` → 404
5. Parse `?after=<uuid>` + `?limit=<n>` query params
6. SELECT paginated keyset (with `after` cursor subquery if provided)
7. COMMIT → 200

- [ ] Implement `send_message` handler
- [ ] Implement `list_messages` handler
- [ ] `cargo check -p garraia-gateway` + `cargo clippy -p garraia-gateway -- -D warnings`
- [ ] Commit `feat(gateway): add send_message + list_messages handlers (plan 0055 t3)`

---

### Task 4: Roteamento — `mod.rs` 3-way match

- [ ] Mode 1: `.route("/v1/chats/{chat_id}/messages", post(messages::send_message).get(messages::list_messages))`
- [ ] Mode 2: fail-soft 503
- [ ] Mode 3: fail-soft 503
- [ ] `cargo check -p garraia-gateway`
- [ ] Commit `feat(gateway): wire /v1/chats/{id}/messages routes (plan 0055 t4)`

---

### Task 5: OpenAPI registration

- [ ] Adicionar `super::messages::send_message` + `super::messages::list_messages` em `paths(...)`
- [ ] Adicionar 4 schemas em `components(schemas(...))`
- [ ] `cargo check -p garraia-gateway`
- [ ] Commit `docs(gateway): register messages endpoints in OpenAPI (plan 0055 t5)`

---

### Task 6: Integration tests `rest_v1_messages.rs`

Scenarios (all in 1 `#[tokio::test]`):

POST scenarios (5):
- M1. POST 201 happy path — asserts response, DB row, `sender_label` match, audit `message.sent` PII-safe
- M2. POST 400 empty body
- M3. POST 401 missing bearer
- M4. POST 400 `X-Group-Id` missing
- M5. POST 404 chat belongs to different group (cross-tenant)

GET scenarios (5):
- G1. GET 200 happy path — 3 messages, newest first
- G2. GET 200 cursor pagination — `after=<mid_id>` returns only older
- G3. GET 200 empty chat
- G4. GET 401 missing bearer
- G5. GET 404 chat of different group

- [ ] Write `tests/rest_v1_messages.rs`
- [ ] `cargo test -p garraia-gateway --test rest_v1_messages` → verde
- [ ] Commit `test(gateway): bundled integration tests for /v1/chats/{id}/messages (plan 0055 t6)`

---

### Task 7: Cross-group authz matrix expansion

6 new cases in `tests/authz_http_matrix.rs`:
- `POST /v1/chats/{alice_chat}/messages` × {alice (member, owns chat)} → 201
- `POST /v1/chats/{alice_chat}/messages` × {eve (non-member)} → 403
- `POST /v1/chats/{alice_chat}/messages` × {alice, X-Group-Id mismatch} → 400
- `GET /v1/chats/{alice_chat}/messages` × {alice} → 200
- `GET /v1/chats/{alice_chat}/messages` × {eve} → 403
- `GET /v1/chats/{alice_chat}/messages` × {alice, X-Group-Id mismatch} → 400

- [ ] Extend `authz_http_matrix.rs`
- [ ] `cargo test -p garraia-gateway --test authz_http_matrix` → verde
- [ ] Commit `test(gateway): extend authz_http_matrix with 6 messages cases (plan 0055 t7)`

---

### Task 8: Final validation + ROADMAP

- [ ] `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings`
- [ ] Mark `[x] POST /v1/chats/{chat_id}/messages` and `[x] GET /v1/chats/{chat_id}/messages` in ROADMAP.md §3.4
- [ ] Commit `docs(roadmap): mark /v1/chats/{id}/messages slice 2 done (plan 0055 t8)`

---

## Risk register

| Risk | Probability | Impact | Mitigation |
|---|---|---|---|
| Esquecer `app.current_group_id` | Média | Alto (RLS 42501) | Invariant 1 + cross-tenant test M5/G5 + authz matrix |
| INSERT `body_tsv` diretamente | Baixa | Alto (DB error) | Invariant 3 doc + código NÃO inclui body_tsv na lista de colunas |
| `sender_label` vazio ou NULL | Baixa | Médio (DB NOT NULL constraint) | SELECT display_name + fallback para email handle |
| Cursor pagination off-by-one | Média | Médio | G2 test com 3 msgs + assert len==1 pós-after |
| Audit body vazamento PII | Média | Crítico (LGPD) | Test M1 assert `meta.get("body").is_none()` |

---

## Acceptance criteria (PR-level)

- [ ] `cargo check --workspace` green.
- [ ] `cargo clippy --workspace -- -D warnings` green.
- [ ] `cargo test --workspace` green em CI.
- [ ] OpenAPI mostra 2 novos endpoints.
- [ ] ROADMAP §3.4 atualizado.
- [ ] Linear GAR-507 marcado Done.

---

## Cross-references

- **Plan 0054** (`POST/GET /v1/groups/{id}/chats`) — padrão de handler + audit + test shape.
- **Plan 0020** (`setRole/delete member`) — padrão de `SET LOCAL app.current_group_id`.
- **Migration 004** (`messages` schema) — fonte do shape DB.
- **Migration 007** (FORCE RLS) — policy `messages_group_isolation`.
- **ADR 0005** (identity provider) — contexto do `Principal` extractor.
- **ROADMAP §3.4** — checklist de endpoints; este slice fecha 2 dos 4 itens Chats restantes.

---

## Estimativa

- Task 0: 5 min
- Task 1: 10 min
- Task 2: 25 min
- Task 3: 60 min
- Task 4: 10 min
- Task 5: 5 min
- Task 6: 60 min
- Task 7: 30 min
- Task 8: 10 min

**Total:** ~3.5h de trabalho focado.
