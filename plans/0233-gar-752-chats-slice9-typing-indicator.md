# Plan 0233 — GAR-752: Chats Slice 9 — Typing Indicator

> **For agentic workers:** Use `superpowers:executing-plans` to implement task-by-task.

**Linear issue:** [GAR-752](https://linear.app/chatgpt25/issue/GAR-752) — "REST /v1 chats slice 9: POST /v1/chats/{chat_id}/typing — typing indicators via SSE" (In Progress, High). Labels: `epic:ws-chat`, `epic:ws-api`. Project: "Fase 3 — Group Workspace".

**Branch:** `routine/202605301233-chats-slice9-typing-indicator`

**Status:** ⏳ Draft — aprovado 2026-05-30 (Florida).

**Goal:** Adicionar endpoint de typing indicator — o sinal ephemeral que informa outros membros que alguém está digitando. Entrega um único endpoint:

- `POST /v1/chats/{chat_id}/typing` → 204 No Content, publica evento SSE `{"type":"typing","user_id":"...","chat_id":"..."}` no canal broadcast do chat já existente.

Sem migration (nenhuma tabela nova — evento ephemeral, não persistido). Reutiliza a infraestrutura SSE de `publish_chat_event` introduzida no plan 0162 (GAR-670).

**Architecture:**

1. **Handler `typing_indicator`** — `POST /v1/chats/{chat_id}/typing`:
   - Sem body (sem `Content-Type`).
   - RBAC: caller deve ter `Action::ChatsRead` (mesmo que `stream_chat`).
   - Cross-tenant guard: `SELECT group_id FROM chats WHERE id = $1 AND group_id = $2 AND archived_at IS NULL` → 404 se não encontrar.
   - FORCE RLS: `SET LOCAL app.current_user_id` + `SET LOCAL app.current_group_id` em tx antes do SELECT.
   - Publica via `state.publish_chat_event(chat_id, json!({...}))` — no-op se não há assinantes.
   - Sem audit row — evento de alta frequência / efémero (até 1 evento por segundo por usuário ativo).
   - Response: 204 No Content.

2. **Payload SSE**:
   ```json
   { "type": "typing", "user_id": "<uuid>", "chat_id": "<uuid>" }
   ```
   - Sem `display_name` — não está no JWT (`Principal` não expõe); client faz lookup de cache local.
   - Sem PII adicional.

3. **OpenAPI**: path `POST /v1/chats/{chat_id}/typing` adicionado em `openapi.rs`.

4. **Router**: rota wired em modo full, fail-soft e no-auth.

---

## Design invariants

1. **Ephemeral — sem persistência** — nenhum INSERT em nenhuma tabela; no-op se não há SSE subscribers.
2. **FORCE RLS obrigatório** — `SET LOCAL app.current_user_id` + `SET LOCAL app.current_group_id` na tx de verificação do chat.
3. **Cross-tenant 404** — `chat_id` não pertencente ao grupo do caller retorna 404, não 204.
4. **Sem audit row** — alta frequência; sem valor forensic adicional.
5. **Sem `display_name` no payload** — não disponível no JWT; client usa cache.
6. **204 idempotente** — múltiplas chamadas são aceitas sem erro.

---

## Validações pré-plano

- ✅ `state.publish_chat_event(chat_id, json)` existe — introduzido em plan 0162 (`RestV1FullState`).
- ✅ `Action::ChatsRead` existe em `action.rs`.
- ✅ `chats.group_id` + `chats.archived_at` existem (migration 004) — padrão já usado em `stream_chat`.
- ✅ FORCE RLS via `SET LOCAL` documentado — padrão canônico em todos os handlers de chats.
- ✅ `stream_chat` handler é o template exato — mesmo guard, sem body, sem audit.
- ✅ Sem migration necessária — evento ephemeral sem tabela.
- ✅ Router já tem padrão para rotas fail-soft e no-auth.

---

## Out of scope

- `display_name` no payload (requer lookup extra ou alargamento do JWT).
- Rate limiting por usuário/chat (nível de infraestrutura, slice separado).
- Persistência de "last seen typing" (desacoplado desta entrega).
- Menções `@user` / `@channel` (parsing de body, slice separado).
- Read receipts / last-read sync (slice separado via `patch_chat_member`).

---

## Rollback

Sem migration. Rollback = reverter `chats.rs` (remover handler) + `mod.rs` (remover rota) + `openapi.rs` (remover path). Nenhuma tabela é modificada.

---

## Tasks (M1)

- [ ] **T1** — Handler `typing_indicator` em `chats.rs`: RBAC + cross-tenant guard + `publish_chat_event`.
- [ ] **T2** — Wiring no router (full / fail-soft / no-auth) em `mod.rs`.
- [ ] **T3** — OpenAPI path em `openapi.rs`.
- [ ] **T4** — Unit tests (≥6): missing group → 400, no ChatsRead → 403, cross-tenant → 404, event has `type` field, event has `user_id` + `chat_id`, no `display_name` in payload.
- [ ] **T5** — ROADMAP + `plans/README.md` updates.

---

## File structure

```
crates/
  garraia-gateway/
    src/
      rest_v1/
        chats.rs        [+1 handler + ≥6 tests]
        openapi.rs      [+1 path]
        mod.rs          [+1 route × 3 modes]
ROADMAP.md              [+[x] bullet typing indicator em §3.6]
plans/README.md         [+row 0233]
```

---

## Risk register

| Risk | Mitigation |
|------|-----------|
| High-frequency DoS via typing events | `publish_chat_event` is in-process; no DB write; no-op with no subscribers. Throttling is client-side concern. |
| Cross-tenant probe via 204 timing | FORCE RLS + explicit `chat_id` membership check returns 404 for foreign chats. |
| SSE channel flood (no subscribers) | `publish_chat_event` drops if no receiver — `broadcast::Sender::send` error is silently ignored. |

---

## Acceptance criteria

- [ ] `cargo check -p garraia-gateway --features test-helpers` — clean
- [ ] `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` — zero warnings
- [ ] `cargo test -p garraia-gateway --features test-helpers --lib -- chats::tests::typing` — ≥6 pass
- [ ] All CI checks green on PR

---

## Cross-references

- Plan 0231 (GAR-747): slice 8 que entrega message reactions — mesmo crate/módulo.
- Plan 0162 (GAR-670): SSE broadcast `publish_chat_event` + `subscribe_chat` — infraestrutura reutilizada.
- Plan 0163 (GAR-679): SSE per-user rate limit (`MAX_SSE_PER_USER`) — não se aplica ao typing (POST, não SSE).
- ROADMAP §3.6: `[ ] Menções (@user, @channel) e typing indicators são slices futuros` — este slice entrega os typing indicators.

---

## Estimativa

- **Low:** 1h (handler simples, sem migration)
- **Provável:** 2h (inclui testes + clippy clean)
- **Alta:** 3h (se router ou openapi precisar de refactor)
