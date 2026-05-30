# Plan 0227 — GAR-745: Chats Slice 7 — `PATCH /v1/threads/{id}` + `PATCH /v1/chats/{id}/members/{uid}`

> **For agentic workers:** Use `superpowers:executing-plans` to implement task-by-task.

**Linear issue:** [GAR-745](https://linear.app/chatgpt25/issue/GAR-745) — "REST /v1 chats slice 7: PATCH /v1/threads/{id} (resolve/title) + PATCH /v1/chats/{id}/members/{uid} (muted/last_read/role)" (In Progress, High). Labels: `epic:ws-chat`, `epic:ws-api`. Project: "Fase 3 — Group Workspace".

**Branch:** `routine/202605291819-chats-slice7-thread-member-patch`

**Status:** ⏳ Draft — aprovado 2026-05-29 (Florida).

**Goal:** Completar a superfície de mutação das entidades `message_threads` e `chat_members`:

1. `PATCH /v1/threads/{thread_id}` — resolve/unresolve um thread + update do título.
2. `PATCH /v1/chats/{chat_id}/members/{user_id}` — atualizar `muted`, `last_read_at` e `role` de um membro do chat.

Ambos reutilizam 100% da fundação existente (AppPool, Principal, can(), RLS, audit). Zero migration nova.

**Architecture:**

1. **`PATCH /v1/threads/{thread_id}`** — novo handler em `chats.rs`:
   - Body: `PatchThreadRequest { title?: String, resolved?: bool }` (pelo menos um campo obrigatório)
   - `resolved: true` → `resolved_at = NOW()`; `resolved: false` → `resolved_at = NULL`
   - RBAC: qualquer membro do chat pode resolve/unresolve (ChatsRead); atualizar título de thread de outro requer ChatsModerate
   - Cross-tenant guard: `thread.chat_id` deve pertencer ao grupo do caller
   - Response: `ThreadDetailResponse { id, chat_id, root_message_id, title, created_by, resolved_at, created_at }`
   - Audit: `WorkspaceAuditAction::ThreadUpdated` com metadata `{ had_title, resolved }`

2. **`PATCH /v1/chats/{chat_id}/members/{user_id}`** — novo handler em `chats.rs`:
   - Body: `PatchChatMemberRequest { muted?: bool, last_read_at?: DateTime<Utc>, role?: String }` (pelo menos um campo)
   - Own-member (caller == user_id): pode atualizar `muted` e `last_read_at`
   - Role update: requer `MembersManage`; somente `'member'` e `'moderator'` aceitos
   - Cross-tenant guard: chat deve estar no grupo do caller; user_id deve ser membro do chat
   - Response: `ChatMemberResponse` estendido com `muted: bool, last_read_at: Option<DateTime<Utc>>`
   - Audit: `WorkspaceAuditAction::ChatMemberUpdated` com metadata `{ changed_fields }` (sem valores — PII-safe)

3. **Audit variants novos**: `ThreadUpdated` + `ChatMemberUpdated` em `audit_workspace.rs`.

4. **`ChatMemberResponse` estendido** para incluir `muted` e `last_read_at` (backward-compatible: adição de campos).

5. **Router**: ambas as rotas wired em modo full, fail-soft e no-auth.

6. **Unit tests** integrados no módulo `tests` de `chats.rs`.

---

## Design invariants

1. **`PATCH` sem campos resulta em 400** — pelo menos um campo deve estar presente em ambos os bodies.
2. **FORCE RLS obrigatório** — `SET LOCAL app.current_user_id` + `SET LOCAL app.current_group_id` em toda tx.
3. **Cross-tenant leak prevention** — `message_threads` não tem `group_id` direto; lookup via JOIN `message_threads → chats WHERE chats.group_id = $caller_group`. 404 para thread inexistente ou de outro grupo.
4. **Own-only guard para `muted`/`last_read_at`** — `PATCH /v1/chats/{id}/members/{uid}` com `uid ≠ caller` e sem MembersManage → 403.
5. **Metadata de audit PII-safe** — nunca incluir títulos ou timestamps no metadata; usar apenas `{ changed_fields: ["resolved", "title"] }`.
6. **`chat_members.last_read_at` aceita qualquer valor ≤ NOW()** — futuro `last_read_at > NOW()` é rejeitado com 422.
7. **`message_threads` não tem `deleted_at`** — DELETE de thread fora do escopo deste slice.

---

## Validações pré-plano

- ✅ `message_threads.resolved_at` existe (migration 004:107-109).
- ✅ `chat_members.(muted, last_read_at, role)` existem (migration 004:42-43).
- ✅ `Action::ChatsModerate` existe em `action.rs:21`.
- ✅ `WorkspaceAuditAction::ThreadCreated` existe como precedente de padrão.
- ✅ `WorkspaceAuditAction::ChatMemberAdded`/`ChatMemberRemoved` existem como precedente.
- ✅ `message_threads` está sob FORCE RLS via JOIN `message_threads_through_chats` (migration 007).
- ✅ `chat_members` está sob FORCE RLS via JOIN `chat_members_group_isolation` (migration 007).

---

## Out of scope

- `DELETE /v1/threads/{thread_id}` (sem `deleted_at` na migration 004).
- `GET /v1/threads/{thread_id}` standalone (retorna apenas via list já existente).
- Typing indicators, reactions, menções.
- Cursor pagination para `list_chat_threads` (já existe paginação cursor; thread patch não precisa de paginação).

---

## Tasks (M1)

- [x] T1: Plan file criado (`plans/0227-...md`) + `plans/README.md` atualizado — commit `docs(plans):`
- [ ] T2: `WorkspaceAuditAction::ThreadUpdated` + `ChatMemberUpdated` em `audit_workspace.rs` (com testes atualizados) — commit `feat(audit):`
- [ ] T3: `PatchThreadRequest` + `ThreadDetailResponse` + `patch_thread` handler em `chats.rs` — commit `feat(chats):`
- [ ] T4: `PatchChatMemberRequest` + `patch_chat_member` handler em `chats.rs` (estende `ChatMemberResponse`) — commit `feat(chats):`
- [ ] T5: Wire ambas as rotas em `mod.rs` (full, fail-soft, no-auth) — commit `feat(router):`
- [ ] T6: Unit tests (resolve/unresolve, title update, muted toggle, last_read_at, role update, cross-tenant rejection, own-only guard) — commit `test(chats):`
- [ ] T7: `cargo clippy` limpo + `cargo check -p garraia-gateway` — commit `style:` se necessário
- [ ] T8: ROADMAP.md atualiza `§3.6` checklist `[ ] Threads (entidade dedicada)` para `[x]` — commit `docs(roadmap):`

---

## Risk register

| Risco | Mitigação |
|---|---|
| `message_threads` RLS JOIN falha | Teste cruzado (T6) valida 404 para caller de outro grupo |
| PATCH sem campo aceito silenciosamente | Body validation rejeita 400 se all-None |
| `last_read_at` futuro aceito | Validator rejeita `last_read_at > Utc::now()` com 422 |

---

## Acceptance criteria

- `PATCH /v1/threads/{id}` com `resolved:true` → 200 com `resolved_at` preenchido.
- `PATCH /v1/threads/{id}` com `resolved:false` → 200 com `resolved_at: null`.
- `PATCH /v1/threads/{id}` com `title` por não-dono sem ChatsModerate → 403.
- `PATCH /v1/threads/{id}` com thread de outro grupo → 404.
- `PATCH /v1/chats/{id}/members/{uid}` com `muted:true` pelo próprio usuário → 200.
- `PATCH /v1/chats/{id}/members/{uid}` com `role:'moderator'` requer MembersManage.
- `PATCH /v1/chats/{id}/members/{uid}` com caller ≠ uid sem MembersManage → 403.
- `cargo clippy --workspace` e `cargo check -p garraia-gateway` verdes.

---

## Cross-references

- Plans anteriores: 0076 (GAR-530 — chats slice 4 member CRUD), 0225 (GAR-740 — list chat threads).
- Migration: 004 (`message_threads`, `chat_members`), 007 (FORCE RLS).
- Auth: `Action::{ChatsRead,ChatsModerate,MembersManage}`, `can()`, `Principal`, `WorkspaceAuditAction`.

---

## Estimativa

- T1: 0.1h (plan já escrito)
- T2: 0.3h (audit variants + testes)
- T3: 0.8h (handler + tipos)
- T4: 0.8h (handler + extensão response)
- T5: 0.2h (router wiring)
- T6: 0.8h (unit tests)
- T7: 0.1h (clippy)
- T8: 0.1h (ROADMAP)
- **Total: ~3.2h**
