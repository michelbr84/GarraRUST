# Plan 0110 — GAR-599: GET /v1/groups/{group_id}/task-lists/{list_id} + PATCH /v1/me

> **For agentic workers:** implement task-by-task following the TDD pattern (test first → red → impl → green → clippy → commit).

**Linear issue:** [GAR-599](https://linear.app/chatgpt25/issue/GAR-599) — "REST /v1 — GET /v1/groups/{group_id}/task-lists/{list_id} + PATCH /v1/me (profile update)" (In Progress, High). Labels: `epic:ws-api`, `epic:ws-tasks`. Project: "Fase 3 — Group Workspace".

**Status:** ⏳ Draft — 2026-05-13 (Florida). Pré-requisitos validados abaixo.

**Goal:** Fechar dois gaps de CRUD na superfície `/v1`:
1. `GET /v1/groups/{group_id}/task-lists/{list_id}` — busca uma task list por ID; a rota já tem `PATCH` + `DELETE` mas faltava `GET`.
2. `PATCH /v1/me` — permite ao usuário autenticado atualizar seu próprio `display_name`. Status (`active/suspended/deleted`) é admin-controlled e permanece fora de escopo.

Zero migration nova, zero ADR novo, zero nova capability. Reutiliza `TaskListRow`/`TaskListResponse` (existentes em `tasks.rs`), `AppPool` (já wired no `AppState`), e `Principal` extractor.

**Architecture:**

### GET /v1/groups/{group_id}/task-lists/{list_id}
- Handler `get_task_list` em `crates/garraia-gateway/src/rest_v1/tasks.rs` (~80 LOC).
- Mesmo padrão de `get_task`: `require_group_id` → `check_group_match` → `can(&principal, Action::TasksRead)` → tx → `SET LOCAL` (user_id + group_id via `set_rls_context`) → `SELECT ... FROM task_lists WHERE id = $1 AND group_id = $2 AND archived_at IS NULL` → `COMMIT` → 200/404.
- Registrado no router (`mod.rs`) como `get(tasks::get_task_list)` na rota `/v1/groups/{group_id}/task-lists/{list_id}` que já tem `.patch(tasks::patch_task_list).delete(tasks::delete_task_list)`.
- OpenAPI: path `GET /v1/groups/{group_id}/task-lists/{list_id}` + responses 200/400/401/403/404.

### PATCH /v1/me
- Handler `patch_me` em `crates/garraia-gateway/src/rest_v1/me.rs` (~120 LOC).
- `State<RestV1FullState>` para acesso ao `app_pool`. `Principal` para `user_id`. JSON body `PatchMeRequest { display_name: Option<String> }` com `deny_unknown_fields`.
- Validação: `display_name` ≤ 128 chars se presente; body completamente vazio (`{}`) → 200 no-op (idempotente).
- `UPDATE users SET display_name = COALESCE($2, display_name), updated_at = now() WHERE id = $1 RETURNING id, email, display_name, status, created_at, updated_at`.
- `users` NÃO está em FORCE RLS group-scoped (é tenant-root, migration 001). `SET LOCAL` NOT needed — apenas o `WHERE id = $1` com `principal.user_id` garante que o usuário só altera o próprio registro.
- Retorna `PatchMeResponse { user_id, email, display_name, created_at, updated_at }`.
- Registrado como `patch(me::patch_me)` na rota `/v1/me` (atualmente só tem `get`).
- Modo 2/3 (AppPool ausente) → `unconfigured_handler` no router.

**Tech stack:** Axum 0.8, `utoipa 5`, `garraia-auth::{Action, Principal, can}`, `sqlx 0.8` (postgres), `serde 1.0`, `chrono 0.4`, `uuid 1.x`. Test harness: `testcontainers` + `pgvector/pgvector:pg16` (existente em `tests/common/`).

---

## Design invariants

1. **`task_lists` está sob FORCE RLS — `app.current_group_id` obrigatório.** Policy `task_lists_group_isolation` usa `group_id = NULLIF(current_setting(...), '')::uuid`. Falhar `SET LOCAL` retorna 0 rows (RLS filtra), que handler trata como 404 — fail-closed correto.
2. **GET task-list retorna 404 para listas arquivadas.** `archived_at IS NULL` no WHERE é intencional — alinha com PATCH/DELETE que também rejeitam listas arquivadas.
3. **PATCH /v1/me: somente `display_name` exposto.** `status` (active/suspended/deleted) é responsabilidade do admin — expô-lo via self-service endpoint violaria RBAC. Se um segundo campo for necessário no futuro, requer ADR ou nova issue.
4. **PATCH /v1/me: `deny_unknown_fields`.** Garante que campos não suportados retornam 422, evitando confusão de API.
5. **PATCH /v1/me: `UPDATE ... WHERE id = $1` com principal.user_id.** Não precisa de SET LOCAL porque users é tenant-root sem FORCE RLS por grupo. A filtragem é por PK do usuário (JWT-authenticated).
6. **Sem auditoria no PATCH /v1/me nesta slice.** O `audit_events` table é group-scoped. Profile updates são user-scoped; auditoria por tenant fica para epic GAR-compliance.

---

## Validações pré-plano

- ✅ `TaskListRow`, `TaskListResponse`, `TaskListSummary` já existem em `tasks.rs:109-275`.
- ✅ `Action::TasksRead` existe em `action.rs:58` com string `"tasks.read"`.
- ✅ `can(Owner|Admin|Member|Guest|Child, TasksRead)` = true — verificar `can.rs`.
- ✅ `set_rls_context(&mut tx, user_id, group_id)` helper existe em `tasks.rs` (re-check import path).
- ✅ `task_lists` tem `archived_at` (migration 006) — `WHERE id = $1 AND group_id = $2 AND archived_at IS NULL` é seguro.
- ✅ `app_pool.pool_for_handlers()` disponível em `RestV1FullState`. Padrão em todos os handlers que fazem SQL.
- ✅ `users` schema: `id UUID PK`, `email citext`, `display_name TEXT`, `status TEXT CHECK(active/suspended/deleted)`, `created_at`, `updated_at` (migration 001).
- ✅ `garraia_app` tem `GRANT UPDATE ON ALL TABLES` (migration 007:70) — `UPDATE users` via AppPool é permitido.
- ✅ `users` NÃO está em FORCE RLS (migrations 007 lista as 10 tabelas; users não está).

---

## Out of scope

- PATCH /v1/me para `status` — admin-only.
- GET /v1/groups/{group_id}/task-lists/{list_id} com chats arquivadas — arquivadas retornam 404.
- Cursor pagination no GET — retorna o objeto único; paginação não se aplica.
- Audit event para profile update — user-scoped, deferred.
- `?include_archived=true` na task-list GET — futuro com `Action::TasksAdmin`.
- WebSocket `/v1/task-lists/{list_id}/stream` — large scope, issue separada.
- PATCH /v1/me para email — implica re-verificação; fora de escopo.

---

## Rollback plan

Aditivo por task. Cada task é commit independente:
- Task 0 (plano no README) — revert remove a linha do índice.
- Task 1 (`get_task_list` handler + utoipa path + testes unitários) — revert remove o handler (arquivo existente, remover apenas a função).
- Task 2 (router wiring: adiciona `get(tasks::get_task_list)`) — revert volta ao `.patch().delete()` sem `.get()`.
- Task 3 (OpenAPI registration de GET task-list) — revert remove o path de `openapi.rs`.
- Task 4 (integration test `rest_v1_task_list_get.rs`) — revert deleta o arquivo de test.
- Task 5 (`PatchMeRequest` + `PatchMeResponse` + `patch_me` handler + unit tests) — revert remove as structs e a função.
- Task 6 (router wiring PATCH /v1/me + Mode 2/3 unconfigured_handler) — revert volta `/v1/me` a `get(me::get_me)` only.
- Task 7 (OpenAPI registration de PATCH /v1/me) — revert remove o path.
- Task 8 (integration test `rest_v1_me_patch.rs`) — revert deleta o arquivo.

Zero migration, zero novo role, zero ADR. Worst-case: 9 revert commits sequenciais.

---

## §12 Open questions

1. **`display_name` pode ser NULL?** → Schema tem `TEXT NOT NULL` implícito? Verificar migration 001. Se `NOT NULL`, PATCH com `display_name: null` deve retornar 422. Se nullable, `null` limpa o campo.
2. **PATCH /v1/me deve retornar o objeto completo do usuário (com email)?** → Sim: `PatchMeResponse { user_id, email, display_name, created_at, updated_at }` — facilita sync do cliente sem GET adicional.
3. **Qual state type para `patch_me`?** → `State<RestV1FullState>` — requer full AppPool. Modes 2/3 ficam com `unconfigured_handler`.

---

## File structure

```
crates/garraia-gateway/src/rest_v1/
  tasks.rs          ← adiciona get_task_list handler (~80 LOC)
  me.rs             ← adiciona PatchMeRequest, PatchMeResponse, patch_me (~120 LOC)
  mod.rs            ← wiring: get(tasks::get_task_list), patch(me::patch_me)
  openapi.rs        ← registra 2 novos paths + PatchMeRequest + PatchMeResponse
tests/
  rest_v1_task_list_get.rs   ← 5 cenários TL-GET-1..5 (novo arquivo)
  rest_v1_me_patch.rs        ← 5 cenários ME-PATCH-1..5 (novo arquivo)
plans/
  0110-gar-599-task-list-get-patch-me.md   ← este arquivo
  README.md         ← adiciona linha 0110
```

---

## M1 Tasks

### T0 — Bookkeeping: registrar plano no README
- [ ] Adicionar linha `| 0110 | GAR-599 — GET task-list + PATCH /v1/me | GAR-599 | 🔄 In Progress |` ao `plans/README.md`.
- [ ] Commit: `docs(plans): add plan 0110 for GAR-599 (GET task-list + PATCH me)`.

### T1 — `get_task_list` handler em `tasks.rs`
- [ ] Adicionar handler `pub async fn get_task_list(...)` seguindo padrão de `get_task`:
  - `State`, `Principal`, `Path<(Uuid, Uuid)>` (path_group_id, list_id).
  - `require_group_id` + `check_group_match` + `can(TasksRead)`.
  - `pool.begin()` → `set_rls_context(user_id, group_id)` → `SELECT ... FROM task_lists WHERE id = $1 AND group_id = $2 AND archived_at IS NULL` → `COMMIT` → 200/404.
  - Columns: `id, group_id, name, type AS list_type, description, created_by, created_by_label, created_at, updated_at, archived_at`.
  - `#[utoipa::path(get, path = "/v1/groups/{group_id}/task-lists/{list_id}", ...)]` com responses 200/400/401/403/404.
- [ ] Unit tests dentro de `mod tests` (inline): `task_list_response_from_row_roundtrip` (verifica conversão `TaskListRow → TaskListResponse`).
- [ ] Commit: `feat(tasks): add get_task_list handler (plan 0110 T1)`.

### T2 — Router wiring para GET task-list
- [ ] Em `mod.rs`, alterar rota `/v1/groups/{group_id}/task-lists/{list_id}` de `patch(...).delete(...)` para `get(tasks::get_task_list).patch(tasks::patch_task_list).delete(tasks::delete_task_list)`.
- [ ] Adicionar a mesma wiring no bloco Mode 2 (`unconfigured_handler` para get).
- [ ] Commit: `feat(rest-v1): wire GET /v1/groups/{group_id}/task-lists/{list_id} (plan 0110 T2)`.

### T3 — OpenAPI para GET task-list
- [ ] Em `openapi.rs`, adicionar `get_task_list` em `paths(...)`.
- [ ] `TaskListResponse` já está registrado em `schemas` — sem duplicata.
- [ ] Commit: `docs(openapi): register GET task-list path (plan 0110 T3)`.

### T4 — Integration tests para GET task-list
- [ ] Criar `tests/rest_v1_task_list_get.rs` com função `#[tokio::test] async fn rest_v1_task_list_get()`:
  - **TL-GET-1**: owner 200 com campos corretos (name, type, description, created_by).
  - **TL-GET-2**: member 200 (seed segundo user via `seed_member_via_admin`).
  - **TL-GET-3**: cross-group → 404 (usar list_id de outro grupo).
  - **TL-GET-4**: archived task list → 404 (`UPDATE task_lists SET archived_at = now()`).
  - **TL-GET-5**: missing `X-Group-Id` header → 400.
- [ ] Commit: `test(rest-v1): integration tests for GET task-list (plan 0110 T4)`.

### T5 — `patch_me` handler + DTOs em `me.rs`
- [ ] Adicionar `PatchMeRequest`:
  ```rust
  #[derive(Debug, Deserialize, ToSchema)]
  #[serde(deny_unknown_fields)]
  pub struct PatchMeRequest {
      pub display_name: Option<String>,
  }
  ```
  Com `validate()` → erro se `display_name.as_ref().map(|s| s.chars().count()).unwrap_or(0) > 128`.
- [ ] Adicionar `PatchMeResponse { user_id: Uuid, email: String, display_name: Option<String>, created_at: DateTime<Utc>, updated_at: DateTime<Utc> }`.
- [ ] Handler `patch_me`:
  - `State<RestV1FullState>`, `Principal`, `Json<PatchMeRequest>`.
  - Validate body.
  - Se tudo None → retornar 200 com dados atuais do usuário (SELECT sem UPDATE).
  - Caso contrário: `UPDATE users SET display_name = COALESCE($2, display_name), updated_at = now() WHERE id = $1 RETURNING id, email, display_name, created_at, updated_at`.
  - Retornar `PatchMeResponse`.
- [ ] Unit tests inline: `patch_me_request_validates_name_too_long`, `patch_me_request_rejects_unknown_fields`.
- [ ] `#[utoipa::path(patch, path = "/v1/me", ...)]`.
- [ ] Commit: `feat(me): add patch_me handler + PatchMeRequest + PatchMeResponse (plan 0110 T5)`.

### T6 — Router wiring para PATCH /v1/me
- [ ] Em `mod.rs` Mode 1: `.route("/v1/me", get(me::get_me).patch(me::patch_me))`.
- [ ] Em Mode 2/3: adicionar `patch(unconfigured_handler)` na rota `/v1/me`.
- [ ] Commit: `feat(rest-v1): wire PATCH /v1/me (plan 0110 T6)`.

### T7 — OpenAPI para PATCH /v1/me
- [ ] Em `openapi.rs`, adicionar `patch_me` em `paths(...)` + `PatchMeRequest` + `PatchMeResponse` em `schemas(...)`.
- [ ] Commit: `docs(openapi): register PATCH /v1/me path (plan 0110 T7)`.

### T8 — Integration tests para PATCH /v1/me
- [ ] Criar `tests/rest_v1_me_patch.rs`:
  - **ME-PATCH-1**: success 200 — atualiza `display_name`, verifica campo retornado.
  - **ME-PATCH-2**: no-op 200 — body `{}`, retorna dados atuais sem erro.
  - **ME-PATCH-3**: `display_name` too long (129 chars) → 422.
  - **ME-PATCH-4**: unknown field no body → 422 (graças a `deny_unknown_fields`).
  - **ME-PATCH-5**: sem JWT → 401.
- [ ] Commit: `test(rest-v1): integration tests for PATCH /v1/me (plan 0110 T8)`.

### T9 — Bookkeeping final + clippy/fmt
- [ ] `cargo fmt --check` → limpo.
- [ ] `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` → zero warnings.
- [ ] Atualizar `plans/README.md` linha 0110: status → `✅ Merged` (pós-merge).
- [ ] Atualizar ROADMAP.md §3.4 com `[x]` para GET task-list + PATCH /v1/me.

---

## Risk register

| Risco | Mitigação |
|-------|-----------|
| `users.display_name` pode ser `NOT NULL` no schema — rejeitar `null` no PATCH | Verificar migration 001 linha 28; se `NOT NULL`, documentar no utoipa e retornar 422 para body com `display_name: null` |
| FORCE RLS em `task_lists` bloqueia silenciosamente com 404 se `SET LOCAL` falhar | Coberto por TL-GET-3 (cross-group) + TL-GET-5 (missing header) |
| `get_task_list` pode entrar em conflito de nome com `list_task_lists` | Nomes distintos: `get_task_list` (singular, by ID) vs `list_task_lists` (plural, all) — ok |

---

## Acceptance criteria

- [ ] `cargo test -p garraia-gateway --test rest_v1_task_list_get` → 5/5 verde.
- [ ] `cargo test -p garraia-gateway --test rest_v1_me_patch` → 5/5 verde.
- [ ] `GET /v1/groups/{group_id}/task-lists/{list_id}` e `PATCH /v1/me` aparecem em `/docs` OpenAPI.
- [ ] `cargo clippy --workspace -- -D warnings` verde (zero novo warning).
- [ ] PR CI: todos os 17+ checks passam.

---

## Cross-references

- Plan 0068 (GAR-518): PATCH/DELETE task-list (onde GET ficou faltando).
- Plan 0015 (GAR-393): scaffold GET /v1/me (base para PATCH).
- Plan 0066 (GAR-516): tasks slice 1 (full CRUD — mas missing GET task-list).
- [GAR-599](https://linear.app/chatgpt25/issue/GAR-599): issue Linear deste plan.
- ROADMAP §3.4 "API REST /v1" — Fase 3.

---

## Estimativa

**LOC:** ~210 (tasks.rs +80, me.rs +130) + routing/openapi ~40 + tests ~200 = ~450 LOC total.
**Tempo:** 1–2h implementação + CI (~45min). Total: < 3h wall clock.
**Risco:** Baixo — padrão estabelecido, sem migration, sem nova capability.
