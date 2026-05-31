# Plan 0237 — GAR-755: Chats Slice 10 — @mentions

> **For agentic workers:** Use `superpowers:executing-plans` to implement task-by-task.

**Linear issue:** [GAR-755](https://linear.app/chatgpt25/issue/GAR-755) — "REST /v1 chats slice 10: @mentions — migration 022 + POST /v1/chats/{id}/messages mentions field + GET /v1/me/mentions" (In Progress, High). Labels: `epic:ws-chat`, `epic:ws-api`. Project: "Fase 3 — Group Workspace".

**Branch:** `routine/202605310015-chats-slice10-mentions`

**Status:** ⏳ Draft — aprovado 2026-05-31 (Florida).

**Goal:** Fechar o item `[ ] Menções (@user, @channel)` do ROADMAP §3.6 com três entregáveis:

1. **Migration 022** — tabela `message_mentions` com FORCE RLS (mesmo padrão de `message_reactions`, migration 021).
2. **`POST /v1/chats/{chat_id}/messages`** — campo `mentions: Vec<Uuid>` opcional (max 50) inserido atomicamente na mesma tx da mensagem, com validação de membership e auditoria PII-safe.
3. **`GET /v1/me/mentions`** — endpoint cursor-paginado para listar menções recebidas pelo caller.

---

## Architecture

### T1 — Migration 022: `message_mentions`

Colunas: `message_id uuid FK→messages(id) ON DELETE CASCADE`, `mentioned_user_id uuid FK→users(id) ON DELETE CASCADE`, `group_id uuid NOT NULL` (denormalizado, copiado de `messages.group_id` no INSERT), `created_at timestamptz DEFAULT now()`.

PK: `(message_id, mentioned_user_id)` — uma menção por usuário por mensagem.

FORCE RLS idêntica a `message_reactions`: `NULLIF(current_setting('app.current_group_id', true), '')::uuid`.

Índice `(mentioned_user_id, created_at DESC)` para `GET /v1/me/mentions` eficiente.

GRANT SELECT, INSERT, DELETE TO garraia_app.

### T2 — `send_message` (messages.rs)

- `SendMessageRequest` ganha `mentions: Vec<Uuid>` com `#[serde(default)]` (lista vazia se ausente, max 50).
- Validate em `validate()`: `mentions.len() > 50 → Err("too many mentions")`.
- Após o INSERT da mensagem (dentro da tx): validar que todos os `mentions` UUIDs são membros do grupo via `SELECT COUNT(*) FROM group_members WHERE group_id = $1 AND user_id = ANY($2)`. Contagem ≠ input deduplicado → 422.
- Batch-INSERT em `message_mentions` (loop de queries individuais parameterizadas — no SQL concat) dentro da mesma tx.
- Audit: `WorkspaceAuditAction::MessageMentionCreated` com `{ mention_count: N }` (PII-safe — sem user IDs).
- `MessageResponse` ganha `mentions: Vec<Uuid>`.

### T3 — `GET /v1/me/mentions` (me.rs)

Handler `list_my_mentions`, rota registrada em `mod.rs`.

Query params: `group_id: Uuid` (required), `after: Option<Uuid>` (cursor = message_id da última menção recebida), `limit: Option<i64>` (1–100, default 50).

RLS: SET LOCAL app.current_user_id + app.current_group_id na tx.

Cursor pattern idêntico ao de `list_messages`: subquery `(mm.created_at, mm.message_id) < (SELECT created_at, message_id FROM message_mentions WHERE message_id = $3 AND mentioned_user_id = $1)`.

Response: `MentionsListResponse { items: Vec<MentionSummary>, next_cursor: Option<Uuid> }`.

`MentionSummary = { message_id: Uuid, chat_id: Uuid, group_id: Uuid, sender_user_id: Uuid, sender_label: String, body_excerpt: String (first 200 chars), created_at: DateTime<Utc> }`.

---

## Design invariants

1. **NO SQL string concat** — todos os inputs via bind parameters.
2. **`group_id` denormalizado** no INSERT de `message_mentions` (copiado de `messages.group_id` do retorno do INSERT da mensagem).
3. **Audit metadata PII-safe**: `mention_count` apenas, sem user IDs.
4. **NULLIF fail-closed** na policy RLS.
5. **Atomicidade**: INSERT message + INSERT message_mentions na mesma tx — se qualquer mention falhar, a mensagem inteira reverte.
6. **Deduplicação**: `ON CONFLICT DO NOTHING` no INSERT de cada mention (PK já garante unicidade; idempotente).
7. **Validação cross-group**: only group members may be mentioned → 422 se qualquer UUID não for membro.
8. **Max 50 mentions** validado em `validate()` antes de qualquer DB access.

---

## Validações pré-plano

- ✅ Migration 021 (`message_reactions`) é o template exato de FORCE RLS + denormalized `group_id` + GRANT.
- ✅ `send_message` em `messages.rs` já tem tx aberta + SET LOCAL antes do INSERT da mensagem.
- ✅ `WorkspaceAuditAction` enum em `garraia-auth/src/audit_workspace.rs` — adicionar `MessageMentionCreated`.
- ✅ Cursor pagination `(created_at, id) < (subquery)` padrão canônico em `list_messages`.
- ✅ `me.rs` tem `PATCH /v1/me` — `GET /v1/me/mentions` encaixa no mesmo módulo.
- ✅ `mod.rs` tem rota `/v1/me` — adicionar `/v1/me/mentions` seguindo o mesmo padrão.

---

## Out of scope

- `@channel` mentions (requer conceito de canal, slice futuro).
- Notificações push para mencionados (mobile/push, slice futuro).
- Contagem de menções não lidas / badge counter (slice futuro).
- Parsing automático de `@username` no body (slice futuro — este slice aceita UUIDs explícitos).

---

## Rollback

Sem breaking change no schema existente. Rollback = reverter migration 022 (`DROP TABLE message_mentions`), reverter `messages.rs` (remover campo `mentions`), reverter `me.rs` (remover handler), reverter `mod.rs` e `openapi.rs`.

---

## Tasks (M1)

- [ ] **T1** — `plans/0237-gar-755-chats-slice10-mentions.md` (este arquivo) + plans/README.md row.
- [ ] **T2** — Migration `crates/garraia-workspace/migrations/022_message_mentions.sql`.
- [ ] **T3** — `WorkspaceAuditAction::MessageMentionCreated` em `garraia-auth/src/audit_workspace.rs`.
- [ ] **T4** — `SendMessageRequest.mentions` + `MessageResponse.mentions` + validação + batch-INSERT + audit em `messages.rs`.
- [ ] **T5** — Handler `list_my_mentions` + structs em `me.rs`.
- [ ] **T6** — Route `/v1/me/mentions` em `mod.rs` + path + schemas em `openapi.rs`.
- [ ] **T7** — Unit tests ≥ 8 (em `messages.rs` e `me.rs`).
- [ ] **T8** — ROADMAP.md `[ ] Menções` → `[x]` + plano README row atualizado.

---

## File structure

```
crates/
  garraia-workspace/
    migrations/
      022_message_mentions.sql         [new]
  garraia-auth/
    src/
      audit_workspace.rs               [+MessageMentionCreated variant + as_str()]
  garraia-gateway/
    src/
      rest_v1/
        messages.rs                    [+mentions field + validation + batch-INSERT + audit]
        me.rs                          [+list_my_mentions handler + MentionSummary + MentionsListResponse]
        mod.rs                         [+GET /v1/me/mentions route]
        openapi.rs                     [+path list_my_mentions + schemas]
plans/
  0237-gar-755-chats-slice10-mentions.md  [this file]
  README.md                               [+row]
ROADMAP.md                               [[ ] Menções → [x]]
```

---

## Risk register

| Risk | Mitigation |
|------|-----------|
| SQL injection via mentions UUIDs | `sqlx::query` with `.bind(mentioned_user_id)` per loop iteration — no concat. |
| Cross-tenant mention leak | FORCE RLS via `group_id` denormalized + validated against `group_members` before INSERT. |
| N+1 insert performance | Max 50 mentions — acceptable for a first slice. Batch can be optimized later. |
| Cursor subquery returns NULL (deleted message) | Subquery returns NULL → tuple comparison `< (NULL, NULL)` is always false in Postgres → empty safe result (same pattern as `list_messages`). |

---

## Acceptance criteria

- [ ] `cargo check -p garraia-gateway --features test-helpers` — clean
- [ ] `cargo check -p garraia-auth` — clean
- [ ] `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` — zero warnings
- [ ] `cargo test -p garraia-gateway --features test-helpers --lib -- messages::tests` — ≥ 8 pass
- [ ] `cargo test -p garraia-gateway --features test-helpers --lib -- me::tests` — ≥ 4 pass
- [ ] All CI checks green on PR

---

## Cross-references

- Plan 0231 (GAR-747): migration 021 `message_reactions` — FORCE RLS template.
- Plan 0055 (GAR-507): `send_message` baseline + `MessageResponse`.
- Plan 0110 (GAR-599): `PATCH /v1/me` — `me.rs` module template.
- ROADMAP §3.6: `[ ] Menções (@user, @channel)` — this plan closes it.

---

## Estimativa

- **Low:** 3h
- **Provável:** 4h (inclui testes + clippy clean)
- **Alta:** 6h (se validação cross-group precisar de refactor)
