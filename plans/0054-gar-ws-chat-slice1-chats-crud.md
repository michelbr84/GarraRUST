# Plan 0054 — GAR-WS-CHAT Slice 1: `POST` + `GET /v1/groups/{group_id}/chats`

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Linear issue:** [GAR-WS-CHAT slice 1] — TBD (issue child a ser criada sob épico `GAR-WS-CHAT` da Fase 3.6 / §3.4 do `ROADMAP.md`). Quando a issue for criada, atualizar este plan + `plans/README.md` com o id real (`GAR-NNN`).

**Status:** ⏳ Draft — aprovado 2026-05-04 (Florida). Pré-requisitos validados empiricamente neste plan §"Validações pré-plano".

**Goal:** Entregar o primeiro par de endpoints sobre `chats` — `POST /v1/groups/{group_id}/chats` (cria channel + auto-enroll do criador como `chat_members.role='owner'`) e `GET /v1/groups/{group_id}/chats` (lista chats ativos do grupo). Reutiliza 100% da fundação dos plans 0016/0017/0019/0020 (AppPool + harness + `Principal` + `RequirePermission` + audit). Zero migration nova, zero ADR novo, zero capability nova — `Action::{ChatsRead,ChatsWrite}` já existem e já estão seedados na matriz `can()` (verificado em `crates/garraia-auth/src/can.rs:42-99`).

**Architecture:**

1. Novo módulo `crates/garraia-gateway/src/rest_v1/chats.rs` (~ 300 LOC), espelhando `groups.rs` no shape: DTOs `CreateChatRequest`, `ChatResponse`, `ChatListResponse`, `ChatSummary` + handlers `create_chat`, `list_chats` + módulo `tests` interno com unit tests dos validators.
2. **`POST /v1/groups/{group_id}/chats`** — happy path:
   - `Principal` extractor já 403'a non-members. Handler valida `principal.group_id == path.group_id` (400 mismatch).
   - `can(&principal, Action::ChatsWrite)` (Owner/Admin/Member/Guest/Child todos passam — 5 roles têm a capability seedada; ver `can.rs`).
   - `body.validate()` — name não-vazio, type ∈ {`channel`} (slice 1 só permite channel; `dm` e `thread` rejeitados 400 com mensagem "type 'dm' is not yet supported in this slice; only 'channel'" / análogo para `thread`).
   - `tx = pool.begin()` → `SET LOCAL app.current_user_id` → `SET LOCAL app.current_group_id` → INSERT chats → INSERT chat_members (creator como `'owner'`) → `audit_workspace_event(ChatCreated)` → `COMMIT` → 201 com `ChatResponse`.
3. **`GET /v1/groups/{group_id}/chats`** — happy path:
   - Mesma checagem de header/path + extractor.
   - `can(&principal, Action::ChatsRead)` (todos os 5 roles passam).
   - `tx = pool.begin()` → `SET LOCAL app.current_user_id` → `SET LOCAL app.current_group_id` → `SELECT id, type, name, topic, created_by, created_at, updated_at FROM chats WHERE archived_at IS NULL ORDER BY created_at DESC LIMIT 100` → `COMMIT` → 200 com `ChatListResponse { items: Vec<ChatSummary> }`.
   - **Sem cursor pagination nesta slice** (LIMIT 100 fixo). Cursor + `?after=...` ficam para slice 2 quando `messages` chegar e o volume justificar.
4. **`WorkspaceAuditAction::ChatCreated`** novo variant em `crates/garraia-auth/src/audit_workspace.rs` com string `"chat.created"`. Atualiza os 3 testes do módulo (`workspace_audit_action_as_str_stable`, `workspace_audit_action_distinct_strings`, `workspace_audit_action_display_delegates_to_as_str`) para incluir o novo variant.
5. **OpenAPI**: registrar `chats::create_chat` + `chats::list_chats` em `paths(...)` e `CreateChatRequest`/`ChatResponse`/`ChatListResponse`/`ChatSummary` em `components(schemas(...))` em `openapi.rs`.
6. **Router**: 2 rotas novas em todos os 3 modes (mode 1 real, modes 2 e 3 fail-soft 503 via `unconfigured_handler`).
7. **Tests**: novo arquivo `tests/rest_v1_chats.rs` com **uma** `#[tokio::test]` bundled (mesmo pattern dos demais — evita race do sqlx runtime teardown). 9 cenários (POST: 5; GET: 4) + extensão da `authz_http_matrix` em `tests/authz_http_matrix.rs` adicionando 6 cenários (3× POST, 3× GET) para satisfazer regra 10 do `CLAUDE.md`.

**Tech stack:** Axum 0.8, `utoipa 5`, `garraia-auth::{Action, Principal, can, WorkspaceAuditAction, audit_workspace_event}`, `sqlx 0.8` (postgres), `serde 1.0`, `chrono 0.4`, `uuid 1.x`. Test harness: `testcontainers` + `pgvector/pgvector:pg16` (já presente em `tests/common/`).

---

## Design invariants (não-negociáveis deste slice)

1. **`chats` está sob FORCE RLS — `app.current_group_id` é obrigatório.** A policy `chats_group_isolation` (migration 007:93-94) exige `group_id = NULLIF(current_setting('app.current_group_id', true), '')::uuid` e funciona como `WITH CHECK` implícito para INSERT (Postgres comportamento default quando `WITH CHECK` está ausente: a expressão `USING` é reaplicada). **Esquecer `SET LOCAL app.current_group_id` é falha silenciosa de INSERT**: o erro vem como `permission denied for relation chats` (`SQLSTATE 42501`), o que é detectável mas precisa estar coberto por teste — ver Task 4 cenário **C-X** (cross-group attempt) que valida o caminho fail-closed.
2. **Auto-enroll do criador como `chat_members.role = 'owner'`.** Mesmo pattern de `create_group → group_members[owner]`. INSERT em `chat_members` precisa rodar **na mesma transaction** do INSERT em `chats` para que o caminho seja atômico. Se o INSERT em `chat_members` falhar, a transaction roll-back garante que o `chats` row também desaparece (sem chat órfão sem owner). `chat_members` é JOIN-RLS via `chats` (migration 007:103-109) — o subquery `chat_id IN (SELECT id FROM chats WHERE group_id = current_group_id)` resolve correto desde que `chats` já esteja inserida na MESMA tx (visível dentro da tx mesmo antes do commit).
3. **Slice 1 só permite `type = 'channel'`.** `dm` requer 2 chat_members na criação + UNIQUE `(group_id, sorted(user_a, user_b))` para evitar DM duplicado — escopo do slice 2 (`POST /v1/chats/{id}/messages` + DM creation). `thread` é variante reservada (cada thread é um `chat` filho de um `message_threads` row) — escopo do slice 3 (`POST /v1/messages/{id}/threads`). Validate rejeita 400 com mensagem PII-safe explícita.
4. **`updated_at = now()` é responsabilidade do caller.** `chats.updated_at` não tem trigger (migration 004:33). Esta slice **só faz INSERT**, então o default `now()` da coluna basta — mas o futuro `PATCH /v1/chats/{id}` (slice futuro) **deve** lembrar de setar `updated_at = now()` explicitamente. Plan futuro herda essa regra; não é preocupação deste plan.
5. **GET retorna apenas `archived_at IS NULL`.** Soft-delete (futuro `DELETE /v1/chats/{id}`) escreve em `archived_at`. Slice 1 não expõe chats arquivados; um query param `?include_archived=true` fica para slice futuro com authz dedicada (`Action::ChatsModerate`).
6. **LIMIT 100 fixo.** Sem cursor pagination até `messages` chegar. Documentar no OpenAPI summary que listagem é "first 100 active chats by `created_at DESC`". Para grupos com > 100 chats ativos, a recomendação até slice 2 é "use search" (futuro `/v1/search`). Aceitável porque grupos típicos têm ≤ 50 chats; gating product-side via documentação.
7. **Audit `chat.created` carrega `metadata = { name_len, type, has_topic }` apenas — NUNCA o nome ou tópico literal.** Nomes de chats são user-controlled e podem conter PII (apelido familiar, nome de cliente). Audit_events é retido para LGPD art. 8 §5 mas não deve persistir PII redundante; o `chats` row já tem o nome para read-back autorizado. Esta convenção segue a regra das outras `WorkspaceAuditAction` (ex: invite carrega `email_domain` em vez do email completo no metadata).
8. **`X-Group-Id` header obrigatório em ambos os endpoints.** Mesmo pattern de `get_group`/`patch_group`/`create_invite`. A presença do header é o que permite o `Principal` extractor fazer o `group_members` membership lookup (Gap C `GRANT SELECT ON group_members TO garraia_login`). Header ausente → 400 BadRequest "X-Group-Id header is required". Header com mismatch ao path → 400 "X-Group-Id header and path id must match".

---

## Validações pré-plano (gate executado nesta sessão)

- ✅ `Action::ChatsRead` existe — `crates/garraia-auth/src/action.rs:52` com string `"chats.read"`.
- ✅ `Action::ChatsWrite` existe — `crates/garraia-auth/src/action.rs:53` com string `"chats.write"`.
- ✅ `can(Owner|Admin|Member|Guest|Child, ChatsRead)` = true para todos os 5 roles — `crates/garraia-auth/src/can.rs:42,69,87,96`.
- ✅ `can(Owner|Admin|Member|Guest|Child, ChatsWrite)` = true para todos os 5 roles — `crates/garraia-auth/src/can.rs:43,70,87,96`.
- ✅ `chats`/`chat_members`/`messages`/`message_threads` schema completo em `migrations/004_chats_and_messages.sql`. UNIQUE `(id, group_id)` em chats (linha 23) suporta compound FK de futuras migrations.
- ✅ `chats`/`chat_members` sob FORCE RLS em `migrations/007_row_level_security.sql:89-112`. Policy direct para chats, JOIN para chat_members.
- ✅ `garraia_app` tem `GRANT INSERT, UPDATE, DELETE, SELECT ON ALL TABLES` (`migrations/007:70`) — INSERT em chats funciona via `AppPool`.
- ✅ `audit_events_group_or_self` policy (migration 007:161-168) cobre INSERT de audit do tipo `chat.created` — `group_id` IS NOT NULL + `app.current_group_id` setado.
- ✅ `WorkspaceAuditAction` enum + `audit_workspace_event` helper existem em `crates/garraia-auth/src/audit_workspace.rs`. Padrão de adição de variant + atualização de testes documentado.
- ✅ `Principal` extractor com `X-Group-Id` membership lookup já testado em produção (plan 0017 + 0019 + 0020 mergeados em main).
- ✅ Harness de teste (`tests/common/`) com `seed_user_with_group` + `seed_member_via_admin` cobre os fixtures necessários para POST e GET. Não precisa de fixture novo.
- ⚠️ `chats.archived_at` é a coluna oficial de soft-delete. GET filtra `archived_at IS NULL` — alinhado com índice parcial `chats_group_id_idx WHERE archived_at IS NULL` (migration 004:26).
- ⚠️ `chats.type IN ('channel','dm','thread')` (CHECK constraint, migration 004:15). Slice 1 expõe apenas `channel`. Validate em `CreateChatRequest::validate` rejeita as outras 2 com mensagens distintas para auxiliar debug do cliente.

---

## Out of scope (rejeitado explicitamente)

- `POST /v1/chats/{id}/messages` + `GET /v1/chats/{id}/messages?cursor=...` — slice 2 (plan 0055+). Requer cursor pagination + FTS query design.
- `POST /v1/messages/{id}/threads` — slice 3 (plan 0056+).
- WebSocket `/v1/chats/{id}/stream` — slice 4 (plan separado, requer backpressure design).
- `PATCH /v1/chats/{id}` (rename, change topic) — slice 2 ou plan separado.
- `DELETE /v1/chats/{id}` (soft-delete via `archived_at`) — slice 2 ou plan separado.
- `POST /v1/chats/{id}/members` (adicionar membro a um channel) — slice 2.
- DM creation (`type = 'dm'`) — slice 2 (requer 2 `chat_members` + UNIQUE constraint para evitar duplicado).
- `?include_archived=true` query param — slice 2+ com `Action::ChatsModerate`.
- Cursor pagination — quando `messages` chegar.
- ETag / `If-Match` concurrency control — fora do escopo de slice 1.
- `schemathesis` contract tests — infra CI separada, plan próprio.

---

## Rollback plan

Aditivo por task. Cada task é um commit independente; `git revert` por commit desfaz limpo:

- Task 0 (registrar 0054 no índice) — revert remove a linha do README.
- Task 1 (`WorkspaceAuditAction::ChatCreated` + testes) — revert remove o variant + roll-back dos test arrays (3 funções).
- Task 2 (módulo `chats.rs` + DTOs + unit tests) — revert deleta o arquivo novo.
- Task 3 (handlers `create_chat` + `list_chats`) — revert volta o módulo ao state pós-Task 2 (só DTOs + unit tests).
- Task 4 (rotas em `mod.rs` 3-way match) — revert volta o router ao state pré-slice. Como Tasks 2-3 só geram código não roteado, esse revert também desativa logicamente (mas não remove) os handlers — ainda aceitável.
- Task 5 (OpenAPI registration) — revert remove os 4 schemas + 2 paths.
- Task 6 (integration tests `rest_v1_chats.rs`) — revert deleta o arquivo de test.
- Task 7 (extensão de `authz_http_matrix`) — revert volta a matriz de N+6 para N cenários.

Zero migration, zero novo role, zero ADR, zero enum role/action novo. Esquema de banco intocado. Worst-case revert é 7 git revert sequenciais.

---

## §12 Open questions (pré-start)

1. **`type` permite `dm` ou `thread` no body?** → **Decisão:** **não**. Validate rejeita 400 com mensagem específica "type 'dm' is not yet supported in this slice; only 'channel'". `thread` análogo. Mantém slice pequeno e evita design incompleto de DM (precisa de 2 chat_members + UNIQUE) ou thread (precisa de message_threads row).
2. **`topic` é obrigatório?** → **Decisão:** opcional. `chats.topic` é nullable no schema. Se ausente no body, fica `NULL`.
3. **GET filtra por type?** → **Decisão:** não nesta slice. Como só `channel` é criável, todos os retornos serão channels. Quando `dm` chegar (slice 2), adicionar query param `?type=channel|dm` opcional.
4. **GET retorna o `chat_members` count?** → **Decisão:** não. Adiciona overhead (subquery COUNT por row); UI pode pedir endpoint dedicado se precisar. Mantém shape simples.
5. **Auto-enroll do criador como `'owner'` ou `'moderator'`?** → **Decisão:** `'owner'`. Espelha `groups → group_members[owner]`. `'moderator'` é tier intermediário usado em chats já existentes onde o creator não é owner do channel original (uso futuro). Para slice 1, criador é owner do chat.
6. **Limite de chats por grupo?** → **Decisão:** não nesta slice. Rate limit existente (`tower-governor` já protege contra abuse). Limite hard (ex: 1000 chats/grupo) é product decision; documentar como TODO no comment do handler para slice futuro.
7. **Resposta do POST inclui o `chat_members` row recém-inserido?** → **Decisão:** não. Resposta é o `chats` row + `created_by` + `created_at`. O cliente sabe que ele é owner por construção (acabou de criar). GET subsequente (com `chat_members` join, futuro) retorna a info detalhada.

---

## File Structure

**Criar:**
- `crates/garraia-gateway/src/rest_v1/chats.rs` — módulo novo com 4 DTOs + 2 handlers + 5 unit tests internos
- `crates/garraia-gateway/tests/rest_v1_chats.rs` — integration test bundled

**Modificar:**
- `crates/garraia-auth/src/audit_workspace.rs` — adicionar variant `ChatCreated` + atualizar 3 testes
- `crates/garraia-gateway/src/rest_v1/mod.rs` — `pub mod chats;` + 2 rotas novas (em mode 1, 2, 3)
- `crates/garraia-gateway/src/rest_v1/openapi.rs` — registrar 2 paths + 4 schemas
- `crates/garraia-gateway/tests/authz_http_matrix.rs` — adicionar 6 cenários (3× POST, 3× GET)
- `plans/README.md` — registrar linha 0054
- `ROADMAP.md` — marcar `[x] POST /v1/groups/{group_id}/chats` e `[x] GET /v1/groups/{group_id}/chats` em §3.4 Chats **apenas quando o PR mergear**

**NÃO tocar:** migrations (reuso puro), `garraia-auth/src/action.rs`/`role.rs`/`can.rs` (capability + role + matriz já corretos), `garraia-workspace` (schema OK), `CLAUDE.md`, `docs/adr/*`, `mobile_*`, `auth_routes.rs`, plans 0010..0053.

---

## M1 — Slice 1 completo

### Task 0: Registrar 0054 no índice

**Files:**
- Create: `plans/0054-gar-ws-chat-slice1-chats-crud.md` (este arquivo)
- Modify: `plans/README.md`

- [ ] **Step 1: Adicionar linha na tabela Index**

Após a linha 0053, adicionar:

```markdown
| 0054 | [GAR-WS-CHAT slice 1 — `POST/GET /v1/groups/{group_id}/chats`](0054-gar-ws-chat-slice1-chats-crud.md) | GAR-WS-CHAT (issue child TBD) | ⏳ Draft |
```

- [ ] **Step 2: Commit**

```bash
git add plans/README.md plans/0054-gar-ws-chat-slice1-chats-crud.md
git commit -m "docs(plans): register plan 0054 (GAR-WS-CHAT slice 1 — chats CRUD)"
```

---

### Task 1: `WorkspaceAuditAction::ChatCreated` + testes

**Files:**
- Modify: `crates/garraia-auth/src/audit_workspace.rs`

- [ ] **Step 1: Atualizar os 3 testes primeiro (TDD red)**

No bloco `#[cfg(test)] mod tests`, adicionar a asserção para o novo variant em **cada um** dos 3 testes:

```rust
// workspace_audit_action_as_str_stable: adicionar
assert_eq!(WorkspaceAuditAction::ChatCreated.as_str(), "chat.created");

// workspace_audit_action_distinct_strings: adicionar ChatCreated ao array
let strings = [
    WorkspaceAuditAction::InviteAccepted.as_str(),
    WorkspaceAuditAction::MemberRoleChanged.as_str(),
    WorkspaceAuditAction::MemberRemoved.as_str(),
    WorkspaceAuditAction::UploadCompleted.as_str(),
    WorkspaceAuditAction::UploadTerminated.as_str(),
    WorkspaceAuditAction::UploadExpired.as_str(),
    WorkspaceAuditAction::ChatCreated.as_str(),  // NOVO
];

// workspace_audit_action_display_delegates_to_as_str: adicionar 1 assert + 1 uso em format!
assert_eq!(format!("{}", WorkspaceAuditAction::ChatCreated), "chat.created");
```

- [ ] **Step 2: Rodar os testes — devem falhar**

```bash
cargo test -p garraia-auth --lib audit_workspace::tests
```

Expected: 3 failures, `error[E0599]: no variant or associated item named ChatCreated found for enum WorkspaceAuditAction`.

- [ ] **Step 3: Adicionar o variant + match arm**

No `pub enum WorkspaceAuditAction`, antes ou após `UploadExpired`, adicionar:

```rust
    /// A new chat (channel/dm/thread) was created in a group via
    /// `POST /v1/groups/{group_id}/chats` (plan 0054 / GAR-WS-CHAT slice 1).
    ///
    /// `resource_type = "chats"`, `resource_id = "{chat_id}"`.
    /// Metadata: `{ name_len, type, has_topic }`. Carrega APENAS metadados
    /// estruturais — nome e tópico do chat são PII potencial (apelido
    /// familiar, nome de cliente) e ficam só na tabela `chats`. Read-back
    /// autorizado via `GET /v1/groups/{id}/chats`.
    ChatCreated,
```

E no `impl WorkspaceAuditAction { pub fn as_str(...) }`, adicionar:

```rust
    WorkspaceAuditAction::ChatCreated => "chat.created",
```

- [ ] **Step 4: Rodar os testes — devem passar (TDD green)**

```bash
cargo test -p garraia-auth --lib audit_workspace::tests
```

Expected: 3 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/garraia-auth/src/audit_workspace.rs
git commit -m "feat(auth): add WorkspaceAuditAction::ChatCreated (plan 0054 t1)"
```

---

### Task 2: Módulo `chats.rs` — DTOs + unit tests (sem handlers)

**Files:**
- Create: `crates/garraia-gateway/src/rest_v1/chats.rs`

- [ ] **Step 1: Escrever os testes unit primeiro (TDD red)**

Criar o arquivo com APENAS os DTOs + um módulo `tests` cobrindo:

```rust
//! `/v1/groups/{group_id}/chats` real handlers (plan 0054, GAR-WS-CHAT slice 1).
//!
//! Two endpoints landing on the `garraia_app` RLS-enforced pool. Both
//! require `X-Group-Id` matching the path id (the `Principal` extractor
//! does the membership lookup; non-members get 403 at extractor time).
//!
//! ## Tenant-context protocol
//!
//! `chats` is under FORCE RLS (migration 007:89-94, policy
//! `chats_group_isolation`), so handlers MUST execute BOTH
//!
//! ```text
//! SET LOCAL app.current_user_id  = '{caller_uuid}'
//! SET LOCAL app.current_group_id = '{path_uuid}'
//! ```
//!
//! before any read or write to `chats` / `chat_members` / `audit_events`.
//! Forgetting `app.current_group_id` causes Postgres to fail the INSERT
//! with `permission denied for relation chats` (SQLSTATE 42501) — the
//! USING clause acts as implicit WITH CHECK when no explicit WITH CHECK
//! is provided.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use garraia_auth::{
    Action, Principal, WorkspaceAuditAction, audit_workspace_event, can,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use utoipa::ToSchema;
use uuid::Uuid;

use super::RestV1FullState;
use super::problem::RestError;

/// Slice 1 só permite `channel`. `dm` e `thread` são reservados para
/// slices futuras (DM precisa de 2 chat_members + UNIQUE; thread depende
/// de `message_threads`).
const ALLOWED_CHAT_TYPES_SLICE1: &[&str] = &["channel"];

/// Request body for `POST /v1/groups/{group_id}/chats`.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateChatRequest {
    /// Display name. Must not be empty after trim. Max length enforced by
    /// the `chats.name` column type (text, no DB CHECK).
    pub name: String,
    /// Chat type. Slice 1: must be `"channel"`. `"dm"` and `"thread"` are
    /// rejected with 400.
    #[serde(rename = "type")]
    pub chat_type: String,
    /// Optional topic / description.
    #[serde(default)]
    pub topic: Option<String>,
}

impl CreateChatRequest {
    /// Structural validation. Returns `Ok(())` on success, `Err(&'static str)`
    /// with a PII-safe detail otherwise.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.name.trim().is_empty() {
            return Err("chat name must not be empty");
        }
        match self.chat_type.as_str() {
            "channel" => {}
            "dm" => return Err("type 'dm' is not yet supported in this slice; only 'channel'"),
            "thread" => return Err("type 'thread' is not yet supported in this slice; only 'channel'"),
            _ => return Err("invalid chat type; must be 'channel'"),
        }
        if let Some(t) = &self.topic {
            if t.len() > 4_000 {
                return Err("topic must be 4000 characters or fewer");
            }
        }
        Ok(())
    }
}

/// Response body for `POST /v1/groups/{group_id}/chats` (201 Created).
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatResponse {
    pub id: Uuid,
    pub group_id: Uuid,
    #[serde(rename = "type")]
    pub chat_type: String,
    pub name: String,
    pub topic: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

/// Compact summary used by `GET /v1/groups/{group_id}/chats`.
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatSummary {
    pub id: Uuid,
    #[serde(rename = "type")]
    pub chat_type: String,
    pub name: String,
    pub topic: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Response body for `GET /v1/groups/{group_id}/chats` (200 OK).
#[derive(Debug, Serialize, ToSchema)]
pub struct ChatListResponse {
    pub items: Vec<ChatSummary>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_chat_request_valid_channel() {
        let req = CreateChatRequest {
            name: "general".into(),
            chat_type: "channel".into(),
            topic: None,
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn create_chat_request_rejects_empty_name() {
        let req = CreateChatRequest {
            name: "  ".into(),
            chat_type: "channel".into(),
            topic: None,
        };
        assert_eq!(req.validate().unwrap_err(), "chat name must not be empty");
    }

    #[test]
    fn create_chat_request_rejects_dm_in_slice1() {
        let req = CreateChatRequest {
            name: "ok".into(),
            chat_type: "dm".into(),
            topic: None,
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "type 'dm' is not yet supported in this slice; only 'channel'"
        );
    }

    #[test]
    fn create_chat_request_rejects_thread_in_slice1() {
        let req = CreateChatRequest {
            name: "ok".into(),
            chat_type: "thread".into(),
            topic: None,
        };
        assert_eq!(
            req.validate().unwrap_err(),
            "type 'thread' is not yet supported in this slice; only 'channel'"
        );
    }

    #[test]
    fn create_chat_request_rejects_unknown_type() {
        let req = CreateChatRequest {
            name: "ok".into(),
            chat_type: "broadcast".into(),
            topic: None,
        };
        assert_eq!(req.validate().unwrap_err(), "invalid chat type; must be 'channel'");
    }

    #[test]
    fn create_chat_request_rejects_topic_over_4000_chars() {
        let req = CreateChatRequest {
            name: "ok".into(),
            chat_type: "channel".into(),
            topic: Some("a".repeat(4_001)),
        };
        assert_eq!(req.validate().unwrap_err(), "topic must be 4000 characters or fewer");
    }
}
```

- [ ] **Step 2: Rodar os testes — devem passar (compilação garante green)**

```bash
cargo test -p garraia-gateway --lib rest_v1::chats::tests
```

Expected: 6 passed. Esta task NÃO compila o módulo no router ainda — só DTOs + tests.

- [ ] **Step 3: Adicionar `pub mod chats;` em `rest_v1/mod.rs`**

Após `pub mod me;`, adicionar:

```rust
pub mod chats;
```

(Apenas o `pub mod` — rotas vêm na Task 4.)

- [ ] **Step 4: `cargo check -p garraia-gateway`** — garante que o módulo compila no contexto do crate.

- [ ] **Step 5: Commit**

```bash
git add crates/garraia-gateway/src/rest_v1/chats.rs crates/garraia-gateway/src/rest_v1/mod.rs
git commit -m "feat(gateway): add chats DTOs + validation (plan 0054 t2)"
```

---

### Task 3: Handlers `create_chat` + `list_chats`

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/chats.rs`

- [ ] **Step 1: Adicionar `create_chat` handler**

Após o módulo `tests` (ou antes — convenção do crate é handlers antes de tests), inserir:

```rust
/// `POST /v1/groups/{group_id}/chats` — create a new channel.
///
/// Authz: caller must be a member of the group AND have
/// `Action::ChatsWrite` (all 5 roles do — Owner/Admin/Member/Guest/Child).
/// The creator is auto-enrolled as `chat_members.role = 'owner'` in the
/// same transaction.
///
/// ## Error matrix
///
/// | Condition                                  | Status | Source         |
/// |--------------------------------------------|--------|----------------|
/// | No JWT                                     | 401    | JWT extractor  |
/// | Non-member of target group                 | 403    | Principal ext. |
/// | `X-Group-Id` missing / mismatched          | 400    | this handler   |
/// | Body: empty name / unknown type / dm/thread| 400    | validate()     |
/// | Body: topic > 4000 chars                   | 400    | validate()     |
/// | (none — all 5 roles can create chats)      | 403    | n/a            |
/// | Happy path                                 | 201    |                |
#[utoipa::path(
    post,
    path = "/v1/groups/{group_id}/chats",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID. Must match the `X-Group-Id` header."),
    ),
    request_body = CreateChatRequest,
    responses(
        (status = 201, description = "Chat created; caller auto-enrolled as `'owner'` in `chat_members`.", body = ChatResponse),
        (status = 400, description = "Invalid body, header/path mismatch, or unsupported type.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of the requested group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn create_chat(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(group_id): Path<Uuid>,
    Json(body): Json<CreateChatRequest>,
) -> Result<(StatusCode, Json<ChatResponse>), RestError> {
    // 1. Header/path coherence (same rule as get_group/patch_group/create_invite).
    match principal.group_id {
        Some(hdr) if hdr == group_id => {}
        Some(_) => {
            return Err(RestError::BadRequest(
                "X-Group-Id header and path id must match".into(),
            ));
        }
        None => {
            return Err(RestError::BadRequest(
                "X-Group-Id header is required".into(),
            ));
        }
    }

    // 2. Capability check. All 5 roles have ChatsWrite seeded; this is a
    //    no-op gate today but stays here so a future role with reduced
    //    chats permission slots in cleanly.
    if !can(&principal, Action::ChatsWrite) {
        return Err(RestError::Forbidden);
    }

    // 3. Structural validation (no DB access; PII-safe messages).
    body.validate()
        .map_err(|msg| RestError::BadRequest(msg.into()))?;

    let trimmed_name = body.name.trim().to_string();

    // 4. Open transaction. SET LOCAL must be tx-scoped — auto-commit
    //    discards the setting between statements.
    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    // 5. Tenant context — BOTH user and group required.
    //    `chats` is FORCE RLS on `app.current_group_id`; `chat_members`
    //    is JOIN-RLS via chats; `audit_events` requires both. Uuid Display
    //    is 36 hex-with-dashes, injection-safe.
    sqlx::query(&format!(
        "SET LOCAL app.current_user_id = '{}'",
        principal.user_id
    ))
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    sqlx::query(&format!(
        "SET LOCAL app.current_group_id = '{group_id}'"
    ))
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    // 6. INSERT chat. RETURNING gives us id + created_at in one roundtrip.
    let (chat_id, created_at): (Uuid, DateTime<Utc>) = sqlx::query_as(
        "INSERT INTO chats (group_id, type, name, topic, created_by) \
         VALUES ($1, $2, $3, $4, $5) \
         RETURNING id, created_at",
    )
    .bind(group_id)
    .bind(&body.chat_type)
    .bind(&trimmed_name)
    .bind(body.topic.as_deref())
    .bind(principal.user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    // 7. Auto-enroll the creator as the chat owner. Same tx so the
    //    chat row + member row are atomic.
    sqlx::query(
        "INSERT INTO chat_members (chat_id, user_id, role) \
         VALUES ($1, $2, 'owner')",
    )
    .bind(chat_id)
    .bind(principal.user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    // 8. Audit. Metadata carries STRUCTURE only — no name/topic literal
    //    (PII risk). The chats row itself is the source of truth for
    //    read-back via GET.
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::ChatCreated,
        principal.user_id,
        group_id,
        "chats",
        chat_id.to_string(),
        json!({
            "name_len": trimmed_name.chars().count(),
            "type": body.chat_type,
            "has_topic": body.topic.is_some(),
        }),
    )
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    Ok((
        StatusCode::CREATED,
        Json(ChatResponse {
            id: chat_id,
            group_id,
            chat_type: body.chat_type,
            name: trimmed_name,
            topic: body.topic,
            created_by: principal.user_id,
            created_at,
        }),
    ))
}
```

- [ ] **Step 2: Adicionar `list_chats` handler**

Logo abaixo de `create_chat`:

```rust
/// `GET /v1/groups/{group_id}/chats` — list active chats in a group.
///
/// Returns up to 100 active (`archived_at IS NULL`) chats ordered by
/// `created_at DESC`. No cursor pagination in slice 1.
///
/// ## Error matrix
///
/// | Condition                                  | Status | Source         |
/// |--------------------------------------------|--------|----------------|
/// | No JWT                                     | 401    | JWT extractor  |
/// | Non-member of target group                 | 403    | Principal ext. |
/// | `X-Group-Id` missing / mismatched          | 400    | this handler   |
/// | (none — all 5 roles can read chats)        | 403    | n/a            |
/// | Happy path                                 | 200    |                |
#[utoipa::path(
    get,
    path = "/v1/groups/{group_id}/chats",
    params(
        ("group_id" = Uuid, Path, description = "Group UUID. Must match the `X-Group-Id` header."),
    ),
    responses(
        (status = 200, description = "Up to 100 active chats, newest first.", body = ChatListResponse),
        (status = 400, description = "`X-Group-Id` header missing or mismatched.", body = super::problem::ProblemDetails),
        (status = 401, description = "Missing or invalid JWT.", body = super::problem::ProblemDetails),
        (status = 403, description = "Caller is not a member of the requested group.", body = super::problem::ProblemDetails),
    ),
    security(("bearer" = []))
)]
pub async fn list_chats(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path(group_id): Path<Uuid>,
) -> Result<Json<ChatListResponse>, RestError> {
    // 1. Header/path coherence.
    match principal.group_id {
        Some(hdr) if hdr == group_id => {}
        Some(_) => {
            return Err(RestError::BadRequest(
                "X-Group-Id header and path id must match".into(),
            ));
        }
        None => {
            return Err(RestError::BadRequest(
                "X-Group-Id header is required".into(),
            ));
        }
    }

    // 2. Capability gate — all 5 roles pass; defensive.
    if !can(&principal, Action::ChatsRead) {
        return Err(RestError::Forbidden);
    }

    // 3. Tx-bound tenant context. SELECT on chats requires
    //    `app.current_group_id` because chats is FORCE RLS.
    let pool = state.app_pool.pool_for_handlers();
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    sqlx::query(&format!(
        "SET LOCAL app.current_user_id = '{}'",
        principal.user_id
    ))
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    sqlx::query(&format!(
        "SET LOCAL app.current_group_id = '{group_id}'"
    ))
    .execute(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    // 4. SELECT — RLS enforces group isolation; explicit `archived_at IS
    //    NULL` filter excludes soft-deleted rows from this slice.
    //    LIMIT 100 fixed; cursor pagination lands when messages do.
    let rows: Vec<(
        Uuid,
        String,
        String,
        Option<String>,
        Uuid,
        DateTime<Utc>,
        DateTime<Utc>,
    )> = sqlx::query_as(
        "SELECT id, type, name, topic, created_by, created_at, updated_at \
         FROM chats \
         WHERE archived_at IS NULL \
         ORDER BY created_at DESC \
         LIMIT 100",
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(e.into()))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(e.into()))?;

    let items = rows
        .into_iter()
        .map(|(id, ct, name, topic, created_by, created_at, updated_at)| ChatSummary {
            id,
            chat_type: ct,
            name,
            topic,
            created_by,
            created_at,
            updated_at,
        })
        .collect();

    Ok(Json(ChatListResponse { items }))
}
```

- [ ] **Step 3: `cargo check -p garraia-gateway` + `cargo clippy -p garraia-gateway -- -D warnings`**

Expected: zero warnings, zero errors. Os handlers ainda não estão roteados (Task 4) mas devem compilar via `pub use`.

- [ ] **Step 4: Commit**

```bash
git add crates/garraia-gateway/src/rest_v1/chats.rs
git commit -m "feat(gateway): add create_chat + list_chats handlers (plan 0054 t3)"
```

---

### Task 4: Roteamento — `mod.rs` 3-way match

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/mod.rs`

- [ ] **Step 1: Mode 1 (full state) — adicionar 2 rotas**

Localizar o `Router::new()` da branch `(Some(full), Some(_auth))` e adicionar antes de `.merge(rate_limited_routes)`:

```rust
                .route(
                    "/v1/groups/{group_id}/chats",
                    post(chats::create_chat).get(chats::list_chats),
                )
```

- [ ] **Step 2: Mode 2 (auth wired, no AppPool) — fail-soft 503**

Na branch `(None, Some(auth))`, adicionar:

```rust
                .route(
                    "/v1/groups/{group_id}/chats",
                    post(unconfigured_handler).get(unconfigured_handler),
                )
```

- [ ] **Step 3: Mode 3 (no auth) — fail-soft 503**

Na branch `(_, None)`, adicionar a mesma linha que o Mode 2 (`unconfigured_handler` em ambos os métodos).

- [ ] **Step 4: `cargo check -p garraia-gateway`**

Expected: green.

- [ ] **Step 5: Commit**

```bash
git add crates/garraia-gateway/src/rest_v1/mod.rs
git commit -m "feat(gateway): wire /v1/groups/{id}/chats routes in 3-way match (plan 0054 t4)"
```

---

### Task 5: OpenAPI registration

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/openapi.rs`

- [ ] **Step 1: Adicionar paths**

Em `paths(...)`, após `super::groups::delete_member,`, adicionar:

```rust
        super::chats::create_chat,
        super::chats::list_chats,
```

- [ ] **Step 2: Adicionar schemas**

Em `components(schemas(...))`, após `MemberResponse,`, adicionar:

```rust
        super::chats::CreateChatRequest,
        super::chats::ChatResponse,
        super::chats::ChatSummary,
        super::chats::ChatListResponse,
```

- [ ] **Step 3: `cargo check -p garraia-gateway`**

Expected: green. utoipa derive valida que os tipos referenciados implementam `ToSchema` (Task 2 já garantiu).

- [ ] **Step 4: Commit**

```bash
git add crates/garraia-gateway/src/rest_v1/openapi.rs
git commit -m "docs(gateway): register chats endpoints + schemas in OpenAPI (plan 0054 t5)"
```

---

### Task 6: Integration tests — `tests/rest_v1_chats.rs`

**Files:**
- Create: `crates/garraia-gateway/tests/rest_v1_chats.rs`

- [ ] **Step 1: Esqueleto + helpers (espelhando rest_v1_groups.rs)**

```rust
//! Integration tests for `/v1/groups/{group_id}/chats` (plan 0054).
//!
//! All scenarios bundled into ONE `#[tokio::test]` function — same
//! pattern as `rest_v1_groups.rs`/`rest_v1_invites.rs`. Splitting into
//! multiple `#[tokio::test]`s triggers the sqlx runtime-teardown race
//! documented in plan 0016 M3 commit `4f8be37`.
//!
//! Scenarios covered (9 total):
//!
//!   POST scenarios (5):
//!   C1. POST 201 — happy path: owner of group creates a 'channel';
//!       asserts response shape, `chats` row, `chat_members[owner]`,
//!       and `audit_events` row with action=`chat.created` + structural
//!       metadata (no PII).
//!   C2. POST 400 — type='dm' rejected with specific message.
//!   C3. POST 400 — empty name.
//!   C4. POST 401 — missing bearer.
//!   C5. POST 400 — X-Group-Id missing.
//!
//!   GET scenarios (4):
//!   G1. GET 200 — happy path: 2 channels returned newest-first; asserts
//!       items length, ordering, archived chat NOT in response.
//!   G2. GET 400 — X-Group-Id mismatch.
//!   G3. GET 401 — missing bearer.
//!   G4. GET 200 empty — group with zero chats returns `items: []`.

mod common;

use axum::body::Body;
use axum::http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

use common::fixtures::{
    fetch_audit_events_for_group,
    seed_user_with_group,
};
use common::Harness;

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("failed to collect response body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("response body is not JSON")
}

fn post_chat(
    token: Option<&str>,
    group_path: &str,
    x_group_id: Option<&str>,
    body: serde_json::Value,
) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri(format!("/v1/groups/{group_path}/chats"))
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request builder");
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
            "127.0.0.1:1".parse().unwrap(),
        ));
    if let Some(t) = token {
        req.headers_mut().insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&format!("Bearer {t}")).unwrap(),
        );
    }
    if let Some(g) = x_group_id {
        req.headers_mut().insert(
            HeaderName::from_static("x-group-id"),
            HeaderValue::from_str(g).unwrap(),
        );
    }
    req
}

fn get_chats(
    token: Option<&str>,
    group_path: &str,
    x_group_id: Option<&str>,
) -> Request<Body> {
    let mut req = Request::builder()
        .method("GET")
        .uri(format!("/v1/groups/{group_path}/chats"))
        .body(Body::empty())
        .expect("request builder");
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo::<std::net::SocketAddr>(
            "127.0.0.1:1".parse().unwrap(),
        ));
    if let Some(t) = token {
        req.headers_mut().insert(
            HeaderName::from_static("authorization"),
            HeaderValue::from_str(&format!("Bearer {t}")).unwrap(),
        );
    }
    if let Some(g) = x_group_id {
        req.headers_mut().insert(
            HeaderName::from_static("x-group-id"),
            HeaderValue::from_str(g).unwrap(),
        );
    }
    req
}

#[tokio::test(flavor = "multi_thread")]
async fn rest_v1_chats_scenarios() {
    let h = Harness::start().await.expect("harness start");
    let router = h.router();

    let (owner_id, group_id, owner_token) =
        seed_user_with_group(&h, "owner@chat-slice1.test").await.unwrap();

    // ── C1. POST 201 happy path ─────────────────────────────────────
    let resp = router
        .clone()
        .oneshot(post_chat(
            Some(&owner_token),
            &group_id.to_string(),
            Some(&group_id.to_string()),
            json!({"name": "general", "type": "channel", "topic": "team-wide"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED, "C1 status");
    let body = body_json(resp).await;
    assert_eq!(body["name"], "general");
    assert_eq!(body["type"], "channel");
    assert_eq!(body["topic"], "team-wide");
    assert_eq!(body["created_by"], owner_id.to_string());
    let chat_id = body["id"].as_str().unwrap().to_string();

    // C1 — verify chat_members owner row created in same tx
    let cm_role: Option<(String,)> = sqlx::query_as(
        "SELECT role FROM chat_members WHERE chat_id = $1::uuid AND user_id = $2",
    )
    .bind(&chat_id)
    .bind(owner_id)
    .fetch_optional(&h.admin_pool)
    .await
    .unwrap();
    assert_eq!(cm_role, Some(("owner".into())), "C1 chat_members owner row");

    // C1 — verify audit row exists with structural metadata only (no PII)
    let events = fetch_audit_events_for_group(&h, group_id).await.unwrap();
    let chat_event = events
        .iter()
        .find(|e| e.action == "chat.created")
        .expect("C1 chat.created audit row");
    assert_eq!(chat_event.resource_type, "chats");
    assert_eq!(chat_event.resource_id, chat_id);
    let meta = &chat_event.metadata;
    assert_eq!(meta["name_len"], 7); // "general".len() == 7
    assert_eq!(meta["type"], "channel");
    assert_eq!(meta["has_topic"], true);
    // Defensive: PII must NOT leak into metadata
    assert!(meta.get("name").is_none(), "C1 audit must NOT carry chat name");
    assert!(meta.get("topic").is_none(), "C1 audit must NOT carry chat topic");

    // ── C2. POST 400 type='dm' rejected ─────────────────────────────
    let resp = router
        .clone()
        .oneshot(post_chat(
            Some(&owner_token),
            &group_id.to_string(),
            Some(&group_id.to_string()),
            json!({"name": "dm-attempt", "type": "dm"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "C2 status");
    let body = body_json(resp).await;
    assert!(body["detail"].as_str().unwrap().contains("'dm' is not yet supported"));

    // ── C3. POST 400 empty name ─────────────────────────────────────
    let resp = router
        .clone()
        .oneshot(post_chat(
            Some(&owner_token),
            &group_id.to_string(),
            Some(&group_id.to_string()),
            json!({"name": "  ", "type": "channel"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "C3 status");

    // ── C4. POST 401 missing bearer ─────────────────────────────────
    let resp = router
        .clone()
        .oneshot(post_chat(
            None,
            &group_id.to_string(),
            Some(&group_id.to_string()),
            json!({"name": "ok", "type": "channel"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "C4 status");

    // ── C5. POST 400 X-Group-Id missing ─────────────────────────────
    let resp = router
        .clone()
        .oneshot(post_chat(
            Some(&owner_token),
            &group_id.to_string(),
            None, // no header
            json!({"name": "ok", "type": "channel"}),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "C5 status");

    // ── G1. GET 200 happy path with 2 channels ──────────────────────
    // Create a 2nd channel for ordering check
    let _ = router
        .clone()
        .oneshot(post_chat(
            Some(&owner_token),
            &group_id.to_string(),
            Some(&group_id.to_string()),
            json!({"name": "random", "type": "channel"}),
        ))
        .await
        .unwrap();

    let resp = router
        .clone()
        .oneshot(get_chats(
            Some(&owner_token),
            &group_id.to_string(),
            Some(&group_id.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "G1 status");
    let body = body_json(resp).await;
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2, "G1 expected 2 channels");
    // Newest-first
    assert_eq!(items[0]["name"], "random");
    assert_eq!(items[1]["name"], "general");

    // ── G1 cont — soft-delete second chat via admin pool, verify exclusion
    sqlx::query("UPDATE chats SET archived_at = now() WHERE name = 'random'")
        .execute(&h.admin_pool)
        .await
        .unwrap();
    let resp = router
        .clone()
        .oneshot(get_chats(
            Some(&owner_token),
            &group_id.to_string(),
            Some(&group_id.to_string()),
        ))
        .await
        .unwrap();
    let body = body_json(resp).await;
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "G1 archived must be excluded");
    assert_eq!(items[0]["name"], "general");

    // ── G2. GET 400 X-Group-Id mismatch ─────────────────────────────
    let resp = router
        .clone()
        .oneshot(get_chats(
            Some(&owner_token),
            &group_id.to_string(),
            Some("00000000-0000-0000-0000-000000000000"),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "G2 status");

    // ── G3. GET 401 missing bearer ──────────────────────────────────
    let resp = router
        .clone()
        .oneshot(get_chats(
            None,
            &group_id.to_string(),
            Some(&group_id.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED, "G3 status");

    // ── G4. GET 200 empty ───────────────────────────────────────────
    let (_, group2_id, owner2_token) =
        seed_user_with_group(&h, "owner2@chat-slice1.test").await.unwrap();
    let resp = router
        .clone()
        .oneshot(get_chats(
            Some(&owner2_token),
            &group2_id.to_string(),
            Some(&group2_id.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "G4 status");
    let body = body_json(resp).await;
    assert_eq!(body["items"].as_array().unwrap().len(), 0, "G4 empty");
}
```

> **Note on `fetch_audit_events_for_group`:** este helper já existe em `tests/common/fixtures.rs` (importado também por `rest_v1_groups.rs`). Se a struct retornada não expor `metadata` como `serde_json::Value`, ajustar mínimo para acessar via field público; senão, deixar como está.

- [ ] **Step 2: Rodar o teste**

```bash
cargo test -p garraia-gateway --test rest_v1_chats
```

Expected: 1 test passed (a função bundled). Se o test container demorar, aumentar timeout via env var existente do harness.

- [ ] **Step 3: Commit**

```bash
git add crates/garraia-gateway/tests/rest_v1_chats.rs
git commit -m "test(gateway): bundled integration tests for /v1/groups/{id}/chats (plan 0054 t6)"
```

---

### Task 7: Cross-group authz matrix expansion

**Files:**
- Modify: `crates/garraia-gateway/tests/authz_http_matrix.rs`

- [ ] **Step 1: Adicionar 6 cenários novos**

A matriz atual cobre `GET /v1/me`, `POST /v1/groups`, `GET /v1/groups/{id}` × {alice, bob, eve} (15 cenários). Adicionar 6 novos:

- `POST /v1/groups/{id}/chats` × {alice (member of g1, posts to g1), eve (non-member of g1, posts to g1)} → expect 201, 403
- `POST /v1/groups/{id}/chats` × {alice (header X-Group-Id mismatch)} → expect 400
- `GET /v1/groups/{id}/chats` × {alice, eve, alice-mismatch} → expect 200, 403, 400

Exato shape segue o pattern existente do arquivo (driver table-driven com `(method, path, x_group_id, expected_status)` rows). Manter estilo idêntico ao já presente — não introduzir novo helper.

- [ ] **Step 2: Rodar a matriz**

```bash
cargo test -p garraia-gateway --test authz_http_matrix
```

Expected: 21 cenários (15 + 6), todos verdes. Se algum 403 vier como 404 ou vice-versa, é bug do extractor — debugar antes de continuar.

- [ ] **Step 3: Commit**

```bash
git add crates/garraia-gateway/tests/authz_http_matrix.rs
git commit -m "test(gateway): extend authz_http_matrix with 6 chats cases (plan 0054 t7)"
```

---

### Task 8: Validação final + ROADMAP

**Files:**
- Modify: `ROADMAP.md`

- [ ] **Step 1: `cargo clippy --workspace -- -D warnings`** — zero warnings.

- [ ] **Step 2: `cargo test --workspace`** — full suite green. Esperar latência adicional do testcontainer (~10-13s para o pgvector boot).

- [ ] **Step 3: ROADMAP marcação**

Em `ROADMAP.md` §3.4 Chats, marcar:

```diff
-- [ ] `POST /v1/groups/{group_id}/chats`
-- [ ] `GET /v1/groups/{group_id}/chats`
++ [x] `POST /v1/groups/{group_id}/chats` — plan 0054, entregue YYYY-MM-DD
++ [x] `GET /v1/groups/{group_id}/chats` — plan 0054, entregue YYYY-MM-DD
```

(Substituir `YYYY-MM-DD` pela data de merge em America/New_York.)

- [ ] **Step 4: Atualizar `plans/README.md`**

Substituir `⏳ Draft` na linha 0054 por `✅ Merged YYYY-MM-DD (commit-sha, PR #N)`.

- [ ] **Step 5: Commit final**

```bash
git add ROADMAP.md plans/README.md
git commit -m "docs(roadmap): mark /v1/groups/{id}/chats slice 1 done (plan 0054 t8)"
```

---

## Risk register

| Risk | Probability | Impact | Mitigation |
|---|---|---|---|
| Esquecer `SET LOCAL app.current_group_id` em handler novo | Média | Alto (INSERT 42501 silencioso) | Cobertura cross-group em Task 7 + invariant 1 documentada explicitamente. Future PR template: lista de check para qualquer handler que toca `chats`/`messages`/`memory_*`. |
| `chat_members` JOIN-RLS subquery quebra dentro da MESMA tx | Baixa | Alto | Validar empiricamente em C1: o INSERT em `chat_members` deve enxergar a row recém-INSERTed em `chats` antes do COMMIT. Postgres garante visibilidade dentro da tx. |
| Audit metadata acidentalmente carrega name/topic | Média | Crítico (LGPD) | Test C1 explícito: `assert!(meta.get("name").is_none())`. Code review obrigatório por `@security-auditor`. |
| `LIMIT 100` insuficiente em produção | Baixa (slice atual) | Médio | Documentado em invariant 6 + open question 6. Slice 2 traz cursor pagination quando messages chegar. |
| Test bundled cresce demais e fica difícil de debugar | Média | Médio | 9 cenários com asserts minimal por cenário; comentar separação clara entre POST e GET blocks. Plan 0055 (slice 2) pode quebrar em 2 funções se já houver sinal de race resolvido. |
| Future `dm` slice quebra signature de `CreateChatRequest` | Média | Médio | `chat_type` é `String` não enum — adicionar `dm` é puramente um relaxamento de validação + lógica adicional para `chat_members` initial set. Não muda shape. |

---

## Acceptance criteria (PR-level)

- [ ] `cargo check --workspace` green.
- [ ] `cargo clippy --workspace -- -D warnings` green.
- [ ] `cargo test --workspace` green em CI (incluindo `rest_v1_chats` + `authz_http_matrix` + `audit_workspace::tests`).
- [ ] Code review APPROVED por `@code-reviewer`.
- [ ] Security audit por `@security-auditor` — sem blockers. Especial atenção a invariant 1 (RLS context) e invariant 7 (audit PII).
- [ ] OpenAPI 3.1 em `/docs` mostra os 2 endpoints novos com examples válidos (manual eyeball).
- [ ] Linear issue (a ser criada como child de GAR-WS-CHAT) atualizada com link do PR.
- [ ] ROADMAP §3.4 atualizado.
- [ ] Plan 0054 status: `✅ Merged YYYY-MM-DD (commit-sha, PR #N)` em `plans/README.md`.

---

## Cross-references

- **Plan 0017** (`PATCH /v1/groups/{id}`) — pattern source para validate + transactional UPDATE.
- **Plan 0019** (`POST /v1/invites/{token}/accept`) — pattern source para `audit_workspace_event` em handler.
- **Plan 0020** (`POST/DELETE /v1/groups/{id}/members/{user_id}`) — pattern source para `SET LOCAL app.current_group_id` + audit.
- **Plan 0021** (`current_group_id` audit RLS gap) — referência pelo qual sabemos que audit_events INSERT exige `app.current_group_id`.
- **Migration 004** (chats/chat_members/messages schema) — fonte do shape DB.
- **Migration 007** (FORCE RLS wrap-up) — fonte das policies em `chats` (direct) e `chat_members` (JOIN).
- **ADR 0005** (identity provider) — contexto da BYPASSRLS role e do extractor `Principal`.
- **ROADMAP §3.4** (API REST `/v1`) — checklist de endpoints; este slice fecha 2 dos 6 itens de "Chats".

---

## Estimativa

- Task 0: 5 min (registro + commit)
- Task 1: 10 min (3 testes + variant + match arm)
- Task 2: 30 min (DTOs + 6 unit tests)
- Task 3: 50 min (2 handlers + comments)
- Task 4: 10 min (3 routing branches)
- Task 5: 5 min (OpenAPI register)
- Task 6: 60 min (9 integration scenarios + helpers)
- Task 7: 30 min (matrix expansion)
- Task 8: 10 min (final validation + ROADMAP)

**Total:** ~3.5h de trabalho focado. Adicionar 30-60 min para revisões inline durante TDD red→green→refactor.
