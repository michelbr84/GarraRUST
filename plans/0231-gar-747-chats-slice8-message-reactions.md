# Plan 0231 — GAR-747: Chats Slice 8 — Message Reactions

> **For agentic workers:** Use `superpowers:executing-plans` to implement task-by-task.

**Linear issue:** [GAR-747](https://linear.app/chatgpt25/issue/GAR-747) — "REST /v1 chats slice 8: message reactions — POST/DELETE/GET /v1/messages/{id}/reactions" (In Progress, High). Labels: `epic:ws-chat`, `epic:ws-api`. Project: "Fase 3 — Group Workspace".

**Branch:** `routine/202605300015-chats-slice8-message-reactions`

**Status:** ⏳ Draft — aprovado 2026-05-30 (Florida).

**Goal:** Adicionar reações emoji a mensagens — o padrão ubíquo de feedback rápido em plataformas de chat. Entrega três endpoints:

1. `POST /v1/messages/{message_id}/reactions` — reagir com emoji.
2. `DELETE /v1/messages/{message_id}/reactions/{emoji}` — remover reação (idempotente).
3. `GET /v1/messages/{message_id}/reactions` — listar reações agrupadas por emoji.

Mais migration 021 (`message_reactions`).

**Architecture:**

1. **Migration 021 — `message_reactions`**:
   - Colunas: `message_id uuid NOT NULL REFERENCES messages(id) ON DELETE CASCADE`, `user_id uuid NOT NULL REFERENCES users(id) ON DELETE CASCADE`, `emoji varchar(64) NOT NULL CHECK (char_length(emoji) BETWEEN 1 AND 10)`, `group_id uuid NOT NULL`, `reacted_at timestamptz NOT NULL DEFAULT now()`
   - PK composta: `(message_id, user_id, emoji)` — um usuário, um emoji, uma reação por mensagem
   - `group_id` denormalizado para audit queries e cross-tenant guards
   - FORCE RLS via JOIN through `messages`: `CREATE POLICY message_reactions_group_isolation ON message_reactions USING (group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid)`
   - Índice: `(message_id, emoji)` para GROUP BY eficiente no GET

2. **`POST /v1/messages/{message_id}/reactions`** — handler `add_message_reaction`:
   - Body: `AddReactionRequest { emoji: String }` — validado 1–10 grapheme clusters
   - RBAC: caller deve ser membro do chat que contém a mensagem (`ChatsRead`)
   - Upsert (ON CONFLICT DO NOTHING) → 201 idempotente
   - Cross-tenant: JOIN `messages WHERE group_id = $caller_group_id` → 404 se não encontrar
   - Audit: `WorkspaceAuditAction::MessageReactionAdded` com metadata `{ emoji_len }` (PII-safe)
   - Response: 201 com body `{ "ok": true }`

3. **`DELETE /v1/messages/{message_id}/reactions/{emoji}`** — handler `remove_message_reaction`:
   - RBAC: só a própria reação; `MessagesModerate` permite remover qualquer reação
   - Idempotente — 204 mesmo se a reação não existir
   - Cross-tenant: 404 se mensagem não pertence ao grupo
   - Audit: `WorkspaceAuditAction::MessageReactionRemoved` com metadata `{ emoji_len }`
   - Response: 204 No Content

4. **`GET /v1/messages/{message_id}/reactions`** — handler `list_message_reactions`:
   - Retorna `[ReactionSummary { emoji, count, reacted_by_me }]` ordenado por `emoji ASC`
   - Sem paginação (reações por mensagem ≤ 100 na prática)
   - `reacted_by_me`: Boolean — `user_id = $caller_user_id` via subquery ou array_agg
   - Cross-tenant: 404
   - Response: `{ "reactions": [...] }`

5. **Audit variants novos**: `MessageReactionAdded` + `MessageReactionRemoved` em `audit_workspace.rs`.

6. **OpenAPI**: novos paths em `openapi.rs`.

7. **Router**: três rotas wired em modo full, fail-soft e no-auth.

---

## Design invariants

1. **PK idempotente** — `INSERT ... ON CONFLICT (message_id, user_id, emoji) DO NOTHING` para o POST.
2. **FORCE RLS obrigatório** — `SET LOCAL app.current_user_id` + `SET LOCAL app.current_group_id` em toda tx que escreve.
3. **Cross-tenant leak prevention** — `message_reactions` não tem acesso direto ao `chat_id`; lookup via `JOIN messages WHERE messages.group_id = $group_id`. 404 para mensagem inexistente ou de outro grupo.
4. **PII-safe audit** — nunca incluir o valor do emoji no metadata; usar apenas `{ emoji_len: usize }`.
5. **emoji validation** — `char_length(emoji) BETWEEN 1 AND 10` em DB CHECK + validação em Rust no handler antes de executar SQL.
6. **`reacted_by_me` é false para unauthenticated/fail-soft** — nunca panic; `Option<Uuid>` para user_id no contexto no-auth.
7. **DELETE idempotente** — 204 mesmo se a linha não existe (sem 404 no delete de reação própria).

---

## Validações pré-plano

- ✅ `messages.group_id` existe (migration 004) — JOIN disponível para cross-tenant guard.
- ✅ `Action::MessagesModerate` existe em `action.rs` como precedente para override de admin.
- ✅ `WorkspaceAuditAction::MessageReactionAdded/Removed` ainda não existem — criar neste slice.
- ✅ Pattern FORCE RLS via JOIN documentado em migration 020 (`message_attachments`).
- ✅ `WorkspaceDb::begin_for` disponível para criar tx com SET LOCAL.
- ✅ Migration numeração: última é 020 → nova será 021.
- ✅ Emoji validation pattern: `char_length` (grapheme clusters em Postgres).

---

## Out of scope

- Typing indicators (requer SSE/WebSocket state, slice separado).
- Menções `@user` / `@channel` (requer parsing de body, slice separado).
- Notificação push ao autor quando reagem à mensagem.
- Reações em threads (mesma tabela funcionaria, mas OUT OF SCOPE aqui).
- Listagem de quem reagiu com cada emoji (além de `reacted_by_me` booleano).

---

## Rollback

Migration 021 é forward-only. Em caso de rollback: `DROP TABLE message_reactions CASCADE;` e reverter `chats.rs` + `audit_workspace.rs` + `openapi.rs` + `router.rs`. Nenhuma tabela existente é modificada.

---

## Tasks (M1)

- [ ] **T1** — Migration 021: `message_reactions` table + FORCE RLS + index. Arquivo: `crates/garraia-workspace/migrations/021_message_reactions.sql`.
- [ ] **T2** — `MessageReactionAdded` + `MessageReactionRemoved` em `garraia-auth/src/audit_workspace.rs`.
- [ ] **T3** — Tipos `AddReactionRequest`, `ReactionSummary`, `ReactionsResponse` em `chats.rs`.
- [ ] **T4** — Handler `add_message_reaction` (POST) com upsert + audit.
- [ ] **T5** — Handler `remove_message_reaction` (DELETE) com idempotência + admin override + audit.
- [ ] **T6** — Handler `list_message_reactions` (GET) com GROUP BY + `reacted_by_me`.
- [ ] **T7** — Wiring no router (full / fail-soft / no-auth) + OpenAPI paths.
- [ ] **T8** — Unit tests (≥15): upsert idempotente, own-delete, admin-delete, cross-tenant 404, emoji validation, list grouped, reacted_by_me true/false, ROADMAP + plans/README updates.

---

## File structure

```
crates/
  garraia-workspace/
    migrations/
      021_message_reactions.sql          [new]
  garraia-auth/
    src/
      audit_workspace.rs                 [+2 variants]
  garraia-gateway/
    src/
      rest_v1/
        chats.rs                         [+3 handlers + types + tests]
      openapi.rs                         [+paths]
      router.rs                          [+3 routes]
ROADMAP.md                               [+[x] bullet in §3.6]
plans/README.md                          [+row 0229]
```

---

## Risk register

| Risk | Mitigation |
|------|-----------|
| Emoji Unicode edge cases (ZWJ sequences, skin tones) | `char_length` in Postgres counts grapheme clusters; Rust validates via byte length cap | 
| Large fan-out of reactions (DoS) | PK enforces 1 reaction per (user, emoji, message); no DoS vector |
| Cross-tenant via message_id brute force | FORCE RLS + JOIN check returns 404 always |

---

## Acceptance criteria

- [ ] `cargo check -p garraia-gateway --features test-helpers` — clean
- [ ] `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` — zero warnings
- [ ] `cargo test -p garraia-gateway --features test-helpers --lib -- chats::tests::reaction` — ≥15 pass
- [ ] Migration 021 SQL parses without error
- [ ] All 20 CI checks green on PR

---

## Cross-references

- Plan 0227 (GAR-745): slice 7 que completa `message_threads` + `chat_members` mutation surface.
- Plan 0179 / GAR-697: migration 020 `message_attachments` — template FORCE RLS via JOIN.
- ROADMAP §3.6: `[ ] Reações, menções (@user, @channel), typing indicators` — este slice entrega as reações.

---

## Estimativa

- **Low:** 3h (sem surpresas no emoji handling)
- **Provável:** 5h (inclui testes + clippy clean)
- **Alta:** 8h (se emoji validation precisar de unicode crate)
