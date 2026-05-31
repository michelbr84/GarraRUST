# Plan 0241 — GAR-759: Chats Slice 11 — Bot Garra no Chat

> **For agentic workers:** Use `superpowers:executing-plans` to implement task-by-task.

**Linear issue:** [GAR-759](https://linear.app/chatgpt25/issue/GAR-759) — "REST /v1 chats slice 11: Bot Garra no chat — /garra <prompt> detection + AgentRuntime async reply" (In Progress, High). Labels: `epic:ws-chat`, `epic:ws-api`. Project: "Fase 3 — Group Workspace".

**Branch:** `routine/202605311000-chats-slice11-bot-garra`

**Status:** ⏳ Draft — aprovado 2026-05-31 (Florida).

**Goal:** Fechar o item `[ ] Bot Garra no chat` do ROADMAP §3.6 com dois entregáveis:

1. **Migration 023** — coluna `is_bot_response BOOLEAN NOT NULL DEFAULT FALSE` na tabela `messages`.
2. **Trigger `/garra <prompt>`** em `POST /v1/chats/{id}/messages` — detecta o prefixo, spawna Tokio task que chama `AgentRuntime`, insere resposta com `is_bot_response = true`, broadcast SSE.

---

## Architecture

### T1 — Migration 023: `is_bot_response` em `messages`

```sql
ALTER TABLE messages
  ADD COLUMN is_bot_response BOOLEAN NOT NULL DEFAULT FALSE;
```

Forward-only. Rows existentes ficam `false` via DEFAULT. Nenhum índice adicional (bot responses são raras; a flag é informacional).

### T2 — Trigger detection em `send_message`

Após o `tx.commit()` e o `publish_chat_event` da mensagem do usuário, verificar:

```rust
if trimmed_body.starts_with("/garra ") {
    let prompt = trimmed_body[7..].trim().to_string();
    let s = Arc::clone(&state); // state é RestV1FullState = Arc<AppState>
    tokio::spawn(bot_reply_task(s, chat_id, group_id, principal.user_id, prompt));
}
```

### T3 — `bot_reply_task` async function

```rust
async fn bot_reply_task(
    state: RestV1FullState,
    chat_id: Uuid,
    group_id: Uuid,
    user_id: Uuid,
    prompt: String,
) {
    // 1. Handle empty prompt
    let response_body = if prompt.is_empty() {
        "Uso: /garra <prompt>. Exemplo: /garra me dê um resumo desta conversa.".to_string()
    } else {
        // 2. Call AgentRuntime
        match state.agents.process_message(&chat_id.to_string(), &prompt, &[]).await {
            Ok(text) => text,
            Err(e) => {
                tracing::warn!(chat_id=%chat_id, error=%e, "bot_reply_task: AgentRuntime error");
                format!("Garra: provider não disponível ({})", e)
            }
        }
    };

    // 3. Insert bot message in new transaction (same RLS pattern)
    let pool = state.app_pool.pool_for_handlers();
    let result: Result<(), _> = async {
        let mut tx = pool.begin().await?;
        sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
            .bind(user_id.to_string()).execute(&mut *tx).await?;
        sqlx::query("SELECT set_config('app.current_group_id', $1, true)")
            .bind(group_id.to_string()).execute(&mut *tx).await?;

        let (bot_msg_id, created_at): (Uuid, DateTime<Utc>) = sqlx::query_as(
            "INSERT INTO messages \
               (chat_id, group_id, sender_user_id, sender_label, body, is_bot_response) \
             VALUES ($1, $2, $3, 'Garra', $4, TRUE) \
             RETURNING id, created_at"
        )
        .bind(chat_id).bind(group_id).bind(user_id).bind(&response_body)
        .fetch_one(&mut *tx).await?;

        tx.commit().await?;

        // 4. Broadcast bot reply via SSE
        state.publish_chat_event(chat_id, serde_json::json!({
            "id": bot_msg_id, "chat_id": chat_id, "group_id": group_id,
            "sender_user_id": user_id, "sender_label": "Garra",
            "body": response_body, "is_bot_response": true,
            "created_at": created_at,
        }));

        Ok::<(), sqlx::Error>(())
    }.await;

    if let Err(e) = result {
        tracing::error!(chat_id=%chat_id, error=%e, "bot_reply_task: DB write failed");
    }
}
```

### T4 — Type updates

- `MessageResponse`: add `is_bot_response: bool`
- `MessageSummary`: add `is_bot_response: bool`
- All SELECT queries on `messages`: include `is_bot_response` column

### T5 — Tests

Unit tests in `messages::tests` (no DB):
- `bot_trigger_detects_prefix`: `"/garra hello".starts_with("/garra ")` → true; `"hello".starts_with("/garra ")` → false.
- `bot_prompt_extraction`: `"/garra   hello world  ".trim_start_matches("/garra ").trim()` → `"hello world"`.
- `bot_empty_prompt`: body = `"/garra "` → prompt is empty → usage-hint branch.

---

## Design invariants

- **V1 limitation**: `sender_user_id` do bot reply = UUID do usuário que o triggou. `is_bot_response = true` distingue bot de humano. Plan futuro introduzirá usuário sistema dedicado (bot user).
- **RLS respeitado**: SET LOCAL both `app.current_user_id` + `app.current_group_id` antes de qualquer write na bot task.
- **Fire-and-forget**: 201 retorna para o caller antes da bot task completar. Erro na bot task → log, sem panic.
- **AgentRuntime errors**: provider não configurado → graceful error message postada no chat.
- **Sem SQL concat**: todos os queries usam `$N` binds.
- **PII-safe audit**: apenas `body_len` em metadata, nunca o conteúdo.
- **NUNCA `unwrap()`** em código de produção.

## Out of scope

- Dedicated bot system user / migration para `users` table (Plan futuro).
- Audit via `WorkspaceAuditAction::MessageSent` para a bot reply (fica como TODO — a estrutura de audit requer tx que já fechou; seria necessário abrir nova tx só para audit, que duplica custo).
- `/garra @channel` (mention de canal) — adiado.
- Resposta streaming (SSE) da bot reply — usa full response text; streaming é Phase 2.
- Rate limiting de bot invocações — adiado.

## Rollback

`ALTER TABLE messages DROP COLUMN is_bot_response;` reverte a migration.
Code rollback: `git revert` do commit de migration + handler.

## M1 — Tasks

- [x] T1: `023_bot_response_flag.sql` — `ALTER TABLE messages ADD COLUMN is_bot_response ...`
- [x] T2: `messages.rs` — `bot_reply_task` async fn + trigger in `send_message`
- [x] T3: `MessageResponse` + `MessageSummary` + SELECT queries updated
- [x] T4: 3 unit tests in `messages::tests`
- [x] T5: plan file + plans/README.md + ROADMAP.md update
- [ ] CI green

## Risk register

| Risco | Mitigação |
|---|---|
| AgentRuntime não configurado | Error path posta mensagem de fallback no chat; log warning |
| Bot task falha no DB write | Error logged, sem panic; user message já committed |
| V1: sender_user_id = triggering user | `is_bot_response = true` flag distingue; documentado como V1 |
| Migration 023 bloqueia tabela em prod | `ALTER TABLE ADD COLUMN DEFAULT` é fast-path no Postgres 11+ (sem lock longo) |

## Acceptance criteria

- `POST /v1/chats/{id}/messages` com body `"/garra ping"` retorna 201 com a mensagem do usuário (is_bot_response: false).
- Bot reply inserida com `is_bot_response: true`, `sender_label = "Garra"`.
- GET /v1/chats/{id}/messages lista ambas as mensagens com campo `is_bot_response` correto.
- 3 unit tests verdes.
- `cargo check -p garraia-gateway --features test-helpers` clean.
- `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` zero warnings.
- CI green.

## Cross-references

- ROADMAP §3.6 `[ ] Bot Garra no chat`
- Plan 0237 (GAR-755) — @mentions (slice 10, predecessor)
- GAR-670 — SSE chat stream (plan 0162)
- Migration 022 — message_mentions (predecessor)

## Estimativa

~350 LOC (migration 10 + messages.rs changes 200 + tests 100 + plan/docs 40).
