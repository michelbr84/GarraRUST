# Plan 0017 — GAR-393 Slice 1: `PATCH /v1/groups/{group_id}`

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Linear issue:** [GAR-393](https://linear.app/chatgpt25/issue/GAR-393) — "Rotas POST/GET/PATCH /v1/groups com OpenAPI" (Backlog, High). Slice 1 de 3 da issue (POST+GET já entregues em plan 0016 M4). Slice 2 (members + invites) fica para plan 0018+. `LIST /v1/groups` collection **não está no escopo de GAR-393** — ver nota no fim deste plano.

**Status:** ⏳ Draft — aprovado 2026-04-15 (Florida) após gate 2 (R2 RLS validado empiricamente).

**Goal:** Entregar o primeiro endpoint de mutação sobre recurso tenant-root existente (`PATCH /v1/groups/{group_id}`), reutilizando 100% da fundação do plan 0016 (AppPool + harness + authz matrix) e sem criar migration nova.

**Architecture:**
1. Handler `patch_group` em `rest_v1/groups.rs` segue o pattern transactional já estabelecido por `create_group`/`get_group`: `BEGIN` → `SET LOCAL app.current_user_id` → membership lookup → authz via `can(Principal, Action::GroupSettings)` → UPDATE parcial com `COALESCE($novo, campo)` → `COMMIT`.
2. Struct `UpdateGroupRequest` com todos os campos opcionais (`Option<String>`). Empty body (nenhum campo setado) → 400. Qualquer campo setado dispara um UPDATE com `updated_at = now()` explicitamente (sem trigger no schema).
3. Authz: `Action::GroupSettings` já existe (22-action enum, migration 002 seed, 63 role_permissions). Owner e Admin têm a permissão; Member/Guest/Child não. 403 para não-autorizado, 404 para grupo inexistente OU membro fantasma (mesma resposta para evitar enumeration), 400 para body inválido, 200 para sucesso.
4. OpenAPI: operação `patch` adicionada via `#[utoipa::path]` no handler + `UpdateGroupRequest` schema registrado em `ApiDoc`.
5. Cobertura: 6 cenários bundled novos no `rest_v1_groups` + expansão da `authz_http_matrix` de 15 para 19 cenários (4 × PATCH: owner/admin/member/outsider).

**Tech Stack:** Axum 0.8, `utoipa 5`, `garraia-auth::{AppPool, Principal, Action::GroupSettings, can}`, `sqlx 0.8` (postgres), `testcontainers` + harness do plan 0016 M2.

**Design invariants (não-negociáveis deste slice):**

1. **Verbo HTTP = `PATCH`, semântica = modificação parcial.** O handler aceita body com qualquer subconjunto dos campos mutáveis (`name`, `type`) e **só atualiza os campos explicitamente presentes** — via `COALESCE($novo, coluna)` no UPDATE. Isto é modificação parcial idempotente por campo, **não** substituição integral do recurso (que seria `PUT`). Clientes que enviarem `{}` recebem **400** (ver invariante 4), não 200 no-op — empty PATCH é sempre mistake do cliente.
2. **`updated_at` deve ser setado explicitamente pelo handler, em todo PATCH que chega a executar o UPDATE.** O schema de `groups` **não tem trigger** (`crates/garraia-workspace/migrations/001_initial_users_groups.sql:115` — *"Caller responsibility — no trigger. Rust code must SET updated_at = now() explicitly on UPDATE."*). Esquecer isto é bug silencioso: o banco aceita, mas o campo fica desatualizado. A query UPDATE do handler (Task 2) **sempre** contém `updated_at = now()` na cláusula `SET`, independente de quais colunas o caller mudou.
3. **`type = "personal"` é rejeitado com HTTP 400.** `personal` é um tipo reservado programmatic-only (migration 001:114 declara: *"API layer (GAR-393) must not expose 'personal' as a user-selectable option — owner-only, programmatic"*). Rejeição acontece em **3 camadas** para defense-in-depth:
   - (a) `UpdateGroupRequest::validate` (Task 1) devolve `"group type 'personal' is reserved and cannot be set via API"` antes de qualquer acesso ao banco — 400.
   - (b) Teste unit em `groups.rs` (Task 1 Step 1, `update_group_request_validate_rejects_personal_type`).
   - (c) Teste integration em `rest_v1_groups.rs` (Task 5 scenario P3, `PATCH {"type": "personal"}` → 400).
   Mesmo que (a) falhe futuramente por regressão, (b) e (c) pegam antes do merge.
4. **Empty body → 400 determinístico.** Body JSON `{}` (ou body com todos os campos setados explicitamente para `null`) é rejeitado com `detail = "patch body must set at least one field"`. Isto protege o banco de UPDATE espúrio e dá feedback claro ao cliente. Ver Task 1 `UpdateGroupRequest::is_empty` + `validate`.

**Validações pré-plano já executadas (gate 2):**
- ✅ `groups` **não está sob RLS** — decisão documentada em `crates/garraia-workspace/migrations/001_initial_users_groups.sql:100-101` e `007_row_level_security.sql:19-21`. Autorização é **app-layer only**.
- ✅ `garraia_app` role tem `GRANT INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public` (`007_row_level_security.sql:70`). UPDATE em `groups` funciona direto via `AppPool` existente.
- ✅ `Action::GroupSettings` existe em `crates/garraia-auth/src/action.rs:37` com dot-format `"group.settings"`.
- ✅ Owner (22 actions) e Admin (20 actions) incluem `GroupSettings` em `crates/garraia-auth/src/can.rs:6-7`. Member/Guest/Child não incluem.
- ⚠️ `groups.updated_at` **não tem trigger** — `crates/garraia-workspace/migrations/001_initial_users_groups.sql:115` exige que o caller Rust faça `SET updated_at = now()` explicitamente no UPDATE. Este plan reflete essa regra.
- ⚠️ `groups.type = 'personal'` **não deve ser exposto como opção de usuário** — `crates/garraia-workspace/migrations/001_initial_users_groups.sql:114` declara que `'personal'` é owner-only programmatic (para GAR-413 SQLite→PG fallback). Handler PATCH rejeita 400 se `type = "personal"` no request.

**Out of scope (rejeitado explicitamente por decisão operacional 2026-04-15):**
- `LIST /v1/groups` (collection) — **não está em GAR-393** conforme validado no Linear. Fica para sub-issue ou plan separado.
- `POST /v1/groups/{id}/invites` — slice 2 de GAR-393, plan 0018+.
- `POST /v1/groups/{id}/members/{user_id}:setRole` — slice 2.
- `DELETE /v1/groups/{id}/members/{user_id}` — slice 2.
- Soft-delete de grupo — não está em GAR-393.
- ETag / `If-Match` concurrency control — vira slice separado se necessário.
- `schemathesis` contract tests — mencionado em GAR-393 mas requer infra CI adicional, fica para plan separado.
- Itens extras do comentário de índice do plan 0016 (admin_url accessor, exec_with_tenant closure, test-support rename, get_group transactional wrap trigger, cenário 8 coverage gap) — decisão de escopo 2026-04-15, não entram aqui.

**Rollback plan:** Aditivo por task. Cada task é um commit independente; `git revert` commit-a-commit desfaz. Zero migration, zero novo role, zero ADR, zero enum novo. Handler PATCH desaparece limpo; rota volta ao estado 0016 M4 apenas com POST+GET.

**§12 Open questions (pré-start):**
1. **Retorno do PATCH:** 200 com `GroupReadResponse` completa (igual GET), 204 No Content, ou 200 com body mínimo? → **Decisão:** 200 com `GroupReadResponse` completa, reutilizando o mesmo struct de resposta do GET. Permite ao cliente refletir `updated_at` sem round-trip extra.
2. **Noop PATCH (body vazio):** 400 Bad Request ou 200 idempotente? → **Decisão:** 400 Bad Request com detail `"patch body must set at least one field"`. Evita UPDATE desnecessário no banco e dá feedback claro.
3. **Permitir mudança de `type`?** → **Decisão:** Sim para `family`/`team`, **não** para `personal`. Validação em `UpdateGroupRequest` — rejeita 400 se `type == "personal"`. Consistente com regra de `create_group`.
4. **Campo `owner_id`:** PATCH pode transferir ownership? → **Decisão:** **Não** nesta slice. Transferência de ownership é domínio de `MembersManage`/`GroupDelete`, não `GroupSettings`. Campo `owner_id` não existe no `UpdateGroupRequest`.

---

## File Structure

**Criar:** nenhum arquivo novo.

**Modificar:**
- `crates/garraia-gateway/src/rest_v1/groups.rs` — adicionar `UpdateGroupRequest` struct + `patch_group` handler + unit tests (deserialize, rejeita personal, empty body é None-None-None)
- `crates/garraia-gateway/src/rest_v1/mod.rs` — registrar `.patch(patch_group)` no route existente `/v1/groups/{id}` (modes 1/2 authed) + stub fail-soft (mode 3)
- `crates/garraia-gateway/src/rest_v1/openapi.rs` — registrar operação `patch_group` + schema `UpdateGroupRequest` em `ApiDoc`
- `crates/garraia-gateway/tests/rest_v1_groups.rs` — adicionar bloco PATCH scenarios (6 cenários bundled na mesma função `#[tokio::test]`)
- `crates/garraia-gateway/tests/authz_http_matrix.rs` — expandir matriz de 15 para 19 cenários (adiciona 4× PATCH cases)
- `plans/README.md` — registrar linha 0017
- `ROADMAP.md` — marcar `[x] PATCH /v1/groups/{group_id}` em §3.4 Grupos **apenas quando o PR mergear**

**NÃO tocar:** migrations (reuso puro), `garraia-auth` (enum + can() já corretos), `garraia-workspace` (schema OK), `CLAUDE.md`, `docs/adr/*`, `mobile_auth.rs`, `mobile_chat.rs`, `auth_routes.rs`, plans 0010..0016.

---

## M1 — Slice 1 completo

### Task 0: Registrar 0017 no índice

**Files:**
- Modify: `plans/README.md`

- [ ] **Step 1: Adicionar linha na tabela Index**

Após a linha 0016, adicionar:

```markdown
| 0017 | [GAR-393 Slice 1 — `PATCH /v1/groups/{group_id}`](0017-gar-393-v1-groups-patch.md) | [GAR-393](https://linear.app/chatgpt25/issue/GAR-393) | ⏳ Draft |
```

- [ ] **Step 2: Commit**

```bash
git add plans/README.md plans/0017-gar-393-v1-groups-patch.md
git commit -m "docs(plans): register plan 0017 (GAR-393 slice 1 — PATCH /v1/groups)"
```

---

### Task 1: `UpdateGroupRequest` struct + unit tests

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/groups.rs`

- [ ] **Step 1: Escrever os testes unit primeiro (TDD red)**

No módulo `tests` existente de `groups.rs`, adicionar:

```rust
#[test]
fn update_group_request_empty_body_all_none() {
    let req: UpdateGroupRequest = serde_json::from_str("{}").unwrap();
    assert!(req.name.is_none());
    assert!(req.group_type.is_none());
    assert!(req.is_empty());
}

#[test]
fn update_group_request_deserializes_type_with_rename() {
    let req: UpdateGroupRequest =
        serde_json::from_str(r#"{"type":"family"}"#).unwrap();
    assert_eq!(req.group_type.as_deref(), Some("family"));
    assert!(!req.is_empty());
}

#[test]
fn update_group_request_name_only_is_not_empty() {
    let req: UpdateGroupRequest =
        serde_json::from_str(r#"{"name":"new name"}"#).unwrap();
    assert_eq!(req.name.as_deref(), Some("new name"));
    assert!(!req.is_empty());
}

#[test]
fn update_group_request_validate_rejects_personal_type() {
    let req = UpdateGroupRequest {
        name: None,
        group_type: Some("personal".into()),
    };
    assert_eq!(
        req.validate().unwrap_err(),
        "group type 'personal' is reserved and cannot be set via API"
    );
}

#[test]
fn update_group_request_validate_rejects_empty_name() {
    let req = UpdateGroupRequest {
        name: Some("".into()),
        group_type: None,
    };
    assert_eq!(
        req.validate().unwrap_err(),
        "name must not be empty"
    );
}

#[test]
fn update_group_request_validate_accepts_valid_family_rename() {
    let req = UpdateGroupRequest {
        name: Some("Updated".into()),
        group_type: Some("family".into()),
    };
    assert!(req.validate().is_ok());
}
```

- [ ] **Step 2: Rodar os testes — devem falhar**

Run: `cargo test -p garraia-gateway --lib rest_v1::groups::tests::update_group_request`
Expected: 6 failures, `error[E0422]: cannot find struct ...UpdateGroupRequest` ou similar.

- [ ] **Step 3: Adicionar `UpdateGroupRequest` struct + `is_empty` + `validate`**

Inserir antes do `patch_group` handler (que será adicionado na Task 2):

```rust
/// Request body for `PATCH /v1/groups/{group_id}`.
///
/// All fields are `Option<T>` — only the fields explicitly set in the JSON
/// body are applied. `{}` (all-None) is rejected by [`UpdateGroupRequest::validate`]
/// because a no-op PATCH is always a client mistake: either the client
/// meant to send something, or they should not have called PATCH.
#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateGroupRequest {
    /// New display name. Rejected if empty string.
    #[serde(default)]
    pub name: Option<String>,
    /// New group type. Must be `family` or `team`. `personal` is a
    /// reserved programmatic-only type (see migration 001 line 114) and
    /// is rejected with 400.
    #[serde(default, rename = "type")]
    pub group_type: Option<String>,
}

impl UpdateGroupRequest {
    /// True when no field was set — caller sent an empty body or a body
    /// of explicit nulls. This is a client error.
    pub fn is_empty(&self) -> bool {
        self.name.is_none() && self.group_type.is_none()
    }

    /// Structural validation. Returns `Ok(())` if the body is coherent,
    /// `Err(&'static str)` with a PII-safe detail otherwise.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.is_empty() {
            return Err("patch body must set at least one field");
        }
        if let Some(name) = &self.name {
            if name.trim().is_empty() {
                return Err("name must not be empty");
            }
        }
        if let Some(t) = &self.group_type {
            match t.as_str() {
                "family" | "team" => {}
                "personal" => {
                    return Err(
                        "group type 'personal' is reserved and cannot be set via API",
                    );
                }
                _ => return Err("invalid group type"),
            }
        }
        Ok(())
    }
}
```

- [ ] **Step 4: Rodar os testes — devem passar (TDD green)**

Run: `cargo test -p garraia-gateway --lib rest_v1::groups::tests::update_group_request`
Expected: 6 passed.

- [ ] **Step 5: Commit**

```bash
git add crates/garraia-gateway/src/rest_v1/groups.rs
git commit -m "feat(gateway): add UpdateGroupRequest struct + validate (plan 0017 t1)"
```

---

### Task 2: `patch_group` handler

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/groups.rs`

- [ ] **Step 1: Adicionar handler `patch_group`**

Abaixo do `get_group` handler existente, inserir:

```rust
/// PATCH /v1/groups/{group_id}
///
/// Authz: caller must be a member of the group AND have
/// `Action::GroupSettings` via their role (owner or admin). Others get
/// 403. Non-member → 404 (same response as missing group to avoid
/// existence enumeration).
///
/// Protocol: transactional `SET LOCAL app.current_user_id` → membership
/// lookup → `can()` → parametrized UPDATE with `COALESCE($new, col)` +
/// explicit `updated_at = now()` (no trigger in schema — see migration
/// 001 line 115) → COMMIT → SELECT back for response.
#[utoipa::path(
    patch,
    path = "/v1/groups/{group_id}",
    params(
        ("group_id" = uuid::Uuid, Path, description = "Group ID")
    ),
    request_body = UpdateGroupRequest,
    responses(
        (status = 200, description = "Group updated", body = GroupReadResponse),
        (status = 400, description = "Invalid body", body = ProblemDetails),
        (status = 401, description = "Unauthenticated", body = ProblemDetails),
        (status = 403, description = "Forbidden", body = ProblemDetails),
        (status = 404, description = "Group not found or not a member", body = ProblemDetails),
    ),
    security(("bearer" = [])),
    tag = "groups",
)]
pub async fn patch_group(
    Principal(principal): Principal,
    AxumState(state): AxumState<RestV1FullState>,
    AxumPath(group_id): AxumPath<uuid::Uuid>,
    Json(body): Json<UpdateGroupRequest>,
) -> Result<Json<GroupReadResponse>, RestError> {
    // 1. Structural validation (no DB access, no PII).
    body.validate().map_err(|msg| RestError::BadRequest(msg.into()))?;

    // 2. Transactional scope — SET LOCAL is tx-bound.
    let mut tx = state
        .app_pool
        .inner()
        .begin()
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
        .bind(principal.user_id.to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    // 3. Membership + role lookup. Non-member = 404 (no enumeration).
    let role_row: Option<(String,)> = sqlx::query_as(
        "SELECT role FROM group_members WHERE group_id = $1 AND user_id = $2",
    )
    .bind(group_id)
    .bind(principal.user_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    let role = match role_row {
        Some((r,)) => garraia_auth::Role::from_str(&r)
            .ok_or_else(|| RestError::Internal(anyhow::anyhow!("unknown role in DB: {r}")))?,
        None => return Err(RestError::NotFound),
    };

    // 4. Authz: GroupSettings → owner/admin only.
    let authz_principal = garraia_auth::Principal {
        user_id: principal.user_id,
        group_id: Some(group_id),
        role: Some(role),
    };
    if !garraia_auth::can(&authz_principal, garraia_auth::Action::GroupSettings) {
        return Err(RestError::Forbidden);
    }

    // 5. UPDATE with COALESCE — only sets fields the caller provided.
    //    Explicit `updated_at = now()` (no trigger — see migration 001:115).
    let updated: (uuid::Uuid, String, String, uuid::Uuid, chrono::DateTime<chrono::Utc>) =
        sqlx::query_as(
            r#"
            UPDATE groups
               SET name       = COALESCE($1, name),
                   type       = COALESCE($2, type),
                   updated_at = now()
             WHERE id = $3
         RETURNING id, name, type, created_by, updated_at
            "#,
        )
        .bind(body.name.as_deref())
        .bind(body.group_type.as_deref())
        .bind(group_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    tx.commit()
        .await
        .map_err(|e| RestError::Internal(anyhow::anyhow!(e)))?;

    Ok(Json(GroupReadResponse {
        id: updated.0,
        name: updated.1,
        group_type: updated.2,
        created_by: updated.3,
        role: role.as_str().to_string(),
        // updated_at exposed if GroupReadResponse has the field; else omit.
    }))
}
```

> **Nota de execução:** `GroupReadResponse` pode ou não ter campo `updated_at`. Se não tiver, este plan **não** adiciona — é mudança fora do corpo do slice. Se o campo já existir, o handler popula. Executor verifica com grep em `groups.rs` antes de escrever o handler; se faltar, deixa o `updated` só atualizando o banco e devolve os outros 5 campos.

- [ ] **Step 2: Adicionar `Role::from_str` em `garraia-auth` se não existir**

Run: `grep -n "fn from_str\|impl.*Role" crates/garraia-auth/src/role.rs`

Se `Role::from_str(&str) -> Option<Role>` não existir, adicionar ao final de `role.rs`:

```rust
impl Role {
    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "owner" => Role::Owner,
            "admin" => Role::Admin,
            "member" => Role::Member,
            "guest" => Role::Guest,
            "child" => Role::Child,
            _ => return None,
        })
    }
}
```

Se já existir, pular o Step 2.

- [ ] **Step 3: Build check**

Run: `cargo check -p garraia-gateway`
Expected: Finished OK.

- [ ] **Step 4: Commit**

```bash
git add crates/garraia-gateway/src/rest_v1/groups.rs crates/garraia-auth/src/role.rs
git commit -m "feat(gateway): add patch_group handler (plan 0017 t2)"
```

---

### Task 3: Route wiring + fail-soft stub

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/mod.rs`

- [ ] **Step 1: Adicionar `.patch(patch_group)` nas rotas authed**

Localizar os arms que registram `/v1/groups/{id}` (modes 1 e 2 — branch com `AppPool` configurado) e trocar:

```rust
.route("/v1/groups/{id}", get(get_group))
```

por:

```rust
.route("/v1/groups/{id}", get(get_group).patch(patch_group))
```

Importar `patch_group` no `use` statement do topo junto com `create_group`/`get_group`.

- [ ] **Step 2: Adicionar stub fail-soft (mode 3)**

No arm `(_, None)` localizar:

```rust
.route("/v1/groups/{id}", get(unconfigured_handler))
```

e substituir por:

```rust
.route("/v1/groups/{id}", get(unconfigured_handler).patch(unconfigured_handler))
```

- [ ] **Step 3: Build + grep de sanidade**

Run: `cargo check -p garraia-gateway && grep -n "patch" crates/garraia-gateway/src/rest_v1/mod.rs`
Expected: compile OK; 2 linhas novas com `.patch(`.

- [ ] **Step 4: Commit**

```bash
git add crates/garraia-gateway/src/rest_v1/mod.rs
git commit -m "feat(gateway): wire PATCH /v1/groups/{id} route + fail-soft stub (plan 0017 t3)"
```

---

### Task 4: OpenAPI registration

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/openapi.rs`

- [ ] **Step 1: Registrar operação e schema**

Adicionar `patch_group` à lista `paths(...)` do `#[derive(OpenApi)]` `ApiDoc` e `UpdateGroupRequest` à lista `schemas(...)`.

Padrão atual (ler `openapi.rs` primeiro; se `ApiDoc` usa `paths(create_group, get_group, ...)`, adicionar `patch_group` logo após `get_group`; se usa `schemas(CreateGroupRequest, GroupReadResponse, ...)`, adicionar `UpdateGroupRequest` logo após `CreateGroupRequest`).

- [ ] **Step 2: Build + grep**

Run: `cargo check -p garraia-gateway && grep -n "patch_group\|UpdateGroupRequest" crates/garraia-gateway/src/rest_v1/openapi.rs`
Expected: compile OK; símbolos referenciados em `paths` e `schemas`.

- [ ] **Step 3: Commit**

```bash
git add crates/garraia-gateway/src/rest_v1/openapi.rs
git commit -m "docs(gateway): register PATCH /v1/groups in OpenAPI ApiDoc (plan 0017 t4)"
```

---

### Task 5: Integration tests — bundled PATCH scenarios

**Files:**
- Modify: `crates/garraia-gateway/tests/rest_v1_groups.rs`

- [ ] **Step 1: Escrever os testes primeiro (TDD red — esperado: só passa parcialmente porque Task 2 já subiu handler)**

Dentro da função `#[tokio::test]` bundled existente (`groups_endpoints_end_to_end` ou equivalente), **ao final do bloco**, adicionar um sub-bloco PATCH com estes 6 cenários sequenciais, reusando `seed_user_with_group`/`seed_user_without_group` já existentes:

```rust
// ─── PATCH /v1/groups/{id} — 6 bundled scenarios ─────────────────────

// Scenario P1: owner renames successfully → 200, body carries new name
let resp = client
    .patch(format!("{base}/v1/groups/{group_id}"))
    .bearer_auth(&owner_jwt)
    .json(&serde_json::json!({"name": "Renamed"}))
    .send()
    .await
    .unwrap();
assert_eq!(resp.status(), 200, "owner rename");
let body: serde_json::Value = resp.json().await.unwrap();
assert_eq!(body["name"], "Renamed");

// Scenario P2: empty body → 400 with deterministic detail
let resp = client
    .patch(format!("{base}/v1/groups/{group_id}"))
    .bearer_auth(&owner_jwt)
    .json(&serde_json::json!({}))
    .send()
    .await
    .unwrap();
assert_eq!(resp.status(), 400, "empty body");
let body: serde_json::Value = resp.json().await.unwrap();
assert_eq!(body["detail"], "patch body must set at least one field");

// Scenario P3: type=personal → 400
let resp = client
    .patch(format!("{base}/v1/groups/{group_id}"))
    .bearer_auth(&owner_jwt)
    .json(&serde_json::json!({"type": "personal"}))
    .send()
    .await
    .unwrap();
assert_eq!(resp.status(), 400, "personal type rejected");

// Scenario P4: non-member JWT (outsider) → 404 (no existence leak)
let resp = client
    .patch(format!("{base}/v1/groups/{group_id}"))
    .bearer_auth(&outsider_jwt)
    .json(&serde_json::json!({"name": "hacked"}))
    .send()
    .await
    .unwrap();
assert_eq!(resp.status(), 404, "outsider PATCH is 404");

// Scenario P5: unauthenticated → 401
let resp = client
    .patch(format!("{base}/v1/groups/{group_id}"))
    .json(&serde_json::json!({"name": "hacked"}))
    .send()
    .await
    .unwrap();
assert_eq!(resp.status(), 401, "no JWT is 401");

// Scenario P6: owner changes type family → team → 200, type reflected
let resp = client
    .patch(format!("{base}/v1/groups/{group_id}"))
    .bearer_auth(&owner_jwt)
    .json(&serde_json::json!({"type": "team"}))
    .send()
    .await
    .unwrap();
assert_eq!(resp.status(), 200, "owner type change");
let body: serde_json::Value = resp.json().await.unwrap();
assert_eq!(body["type"], "team");
```

> **Nota:** se o fixture existente `seed_user_without_group` já fornece um outsider JWT (plan 0014 criou esse helper), usar `outsider_jwt` dessa função. Se não existe ainda, executor adiciona helper no mesmo módulo `tests/common/fixtures.rs` como parte desta task.

- [ ] **Step 2: Rodar a suíte**

Run: `cargo test -p garraia-gateway --features test-helpers --test rest_v1_groups`
Expected: 1 test bundled passa (todos os 6 sub-cenários PATCH + os já existentes POST/GET).

- [ ] **Step 3: Commit**

```bash
git add crates/garraia-gateway/tests/rest_v1_groups.rs crates/garraia-gateway/tests/common/fixtures.rs
git commit -m "test(gateway): PATCH /v1/groups 6-scenario bundled (plan 0017 t5)"
```

---

### Task 6: Cross-group authz matrix expansion

**Files:**
- Modify: `crates/garraia-gateway/tests/authz_http_matrix.rs`

- [ ] **Step 1: Expandir a matriz de 15 para 19 cenários**

A matriz existente cobre `GET /v1/me`, `POST /v1/groups`, `GET /v1/groups/{id}` × {alice_owner, bob_other_group, eve_no_group}. Adicionar coluna PATCH × 4 casos:

```rust
// ─── PATCH /v1/groups/{id} ─────────────────────────────────────────────
AuthzCase {
    name: "patch_group alice-owner → 200",
    method: Method::PATCH,
    path: format!("/v1/groups/{alice_group_id}"),
    jwt: alice_jwt.clone(),
    body: Some(serde_json::json!({"name": "OwnerRename"})),
    expected: StatusCode::OK,
},
AuthzCase {
    name: "patch_group alice-admin of group → 200",
    method: Method::PATCH,
    path: format!("/v1/groups/{alice_group_id}"),
    jwt: alice_admin_jwt.clone(),
    body: Some(serde_json::json!({"name": "AdminRename"})),
    expected: StatusCode::OK,
},
AuthzCase {
    name: "patch_group eve (no group) → 401 or 404",
    method: Method::PATCH,
    path: format!("/v1/groups/{alice_group_id}"),
    jwt: eve_jwt.clone(),
    body: Some(serde_json::json!({"name": "EveHack"})),
    expected: StatusCode::NOT_FOUND,
},
AuthzCase {
    name: "patch_group bob (member of other group) → 404",
    method: Method::PATCH,
    path: format!("/v1/groups/{alice_group_id}"),
    jwt: bob_jwt.clone(),
    body: Some(serde_json::json!({"name": "BobHack"})),
    expected: StatusCode::NOT_FOUND,
},
```

**Nota sobre o caso "alice-admin":** se a fixture atual só provisiona alice como `owner`, adicionar um segundo principal "alice_admin" seedado como `admin` no mesmo grupo via helper `seed_user_with_role("admin", group_id)`. Se o helper não existir, cortar este caso (matriz fica em 18, não 19) e documentar em review comment que admin coverage fica para slice 2.

- [ ] **Step 2: Se `AuthzCase` ainda não suporta `body: Option<serde_json::Value>`, estender o struct**

Procurar em `authz_http_matrix.rs` a definição de `AuthzCase`. Se não tem `body`, adicionar:

```rust
struct AuthzCase {
    name: &'static str,
    method: Method,
    path: String,
    jwt: String,
    body: Option<serde_json::Value>,  // novo
    expected: StatusCode,
}
```

E no dispatch loop, se `body` está `Some`, usar `.json(&body)`; se `None`, request sem body.

- [ ] **Step 3: Rodar a matriz**

Run: `cargo test -p garraia-gateway --features test-helpers --test authz_http_matrix`
Expected: 1 test passa com 18 ou 19 sub-cenários (dependendo da decisão do step 1).

- [ ] **Step 4: Commit**

```bash
git add crates/garraia-gateway/tests/authz_http_matrix.rs
git commit -m "test(gateway): expand authz matrix with PATCH cases (plan 0017 t6)"
```

---

### Task 7: Full validation pass

Sem commit. Validação pura antes de abrir PR.

- [ ] **Step 1:** `cargo check --workspace`
- [ ] **Step 2:** `cargo clippy -p garraia-gateway --no-deps -- -D warnings` — aceitar os mesmos 5 warnings pré-existentes em `admin/*` + `bootstrap.rs`; rejeitar qualquer warning novo em `rest_v1/*`
- [ ] **Step 3:** `cargo test -p garraia-gateway --lib` — 167+ unit tests (os 6 novos de `UpdateGroupRequest` somam a 173)
- [ ] **Step 4:** `cargo test -p garraia-gateway --features test-helpers --test rest_v1_me` — bundled authed scenarios
- [ ] **Step 5:** `cargo test -p garraia-gateway --features test-helpers --test rest_v1_groups` — bundled com os 6 PATCH novos
- [ ] **Step 6:** `cargo test -p garraia-gateway --features test-helpers --test harness_smoke`
- [ ] **Step 7:** `cargo test -p garraia-gateway --features test-helpers --test authz_http_matrix` — 18 ou 19 cenários
- [ ] **Step 8:** `cargo test -p garraia-gateway --test router_smoke_test` — 3 tests legado

Todos verdes → pronto para PR.

---

## §8 Rollback plan

Cada task é um commit independente. `git revert` commit-a-commit restaura estado anterior. Pontos críticos:

- **Task 1 (`UpdateGroupRequest`)** é puramente aditiva — adiciona um struct novo, não toca nada existente. Revert limpo.
- **Task 2 (`patch_group` handler)** depende de Task 1; se revertida isoladamente, Task 1 fica com struct sem consumer (morto mas compila).
- **Task 3 (route wiring)** depende de Task 2; revert isolado volta o route a só `get(get_group)`, mesmo comportamento do plan 0016 M4.
- **Task 4 (OpenAPI)** puramente documental — revert não afeta runtime.
- **Tasks 5+6 (testes)** — revert só reduz cobertura, não afeta código de produção.

**Migrations:** **nenhuma nova**. Zero risco de schema. Zero role nova.

**Env vars:** **nenhuma nova**. Reuso puro de `GARRAIA_APP_DATABASE_URL` já estabelecido pelo plan 0016 M1.

---

## §12 Open questions (fechadas antes do start)

Todas as 4 open questions do header foram fechadas com decisão explícita antes de aprovar o plano. Ver seção "Architecture" item 1-5 para o detalhe operacional.

---

## Self-review checklist (2026-04-15)

- **Spec coverage:** GAR-393 define 6 endpoints; este plano entrega **apenas 1** (`PATCH`) — slice 1 de 3, out-of-scope documentado. ✅
- **Placeholder scan:** nenhum TBD/TODO. Passos com código mostram o código completo. Notas de execução (sobre `GroupReadResponse.updated_at`, `Role::from_str`, admin fixture) são pontos de verificação empírica no start, não placeholders. ✅
- **Type consistency:** `UpdateGroupRequest` definido na Task 1, consumido nas Tasks 2/4/5/6 consistentemente. `patch_group` referenciado em Tasks 2/3/4. `GroupSettings`/`Role::from_str` são símbolos existentes em `garraia-auth` validados no gate 2. ✅
- **Ambiguidade:** Task 2 Step 1 tem nota explícita sobre `GroupReadResponse.updated_at` (verificar e adaptar). Task 5 tem nota sobre `seed_user_without_group` (existente ou criar). Task 6 tem nota sobre admin case (cut se fixture falta). ✅
- **Risco coberto:**
  - R1 (Action): resolvido — `GroupSettings` existe, não mudado.
  - R2 (RLS groups UPDATE): resolvido — sem RLS, grant OK.
  - R3 (SET LOCAL): reutilizado do pattern POST/GET M4, não redefinido.
  - R4 (PATCH semântico): decidido — parcial com `COALESCE`.
  - R5 (cascata de routes): baixo, coberto por `router_smoke_test`.
  - R6 (authz matrix): coberto pela Task 6.
  - R7 (updated_at explicit): coberto pelo handler (Task 2).
  - R8 (personal rejection): coberto por `UpdateGroupRequest::validate` (Task 1) + teste unit + teste integration (Task 5 scenario P3).

---

## Nota sobre `LIST /v1/groups` (fora do escopo deste plano)

Durante o gate 1 (2026-04-15), foi identificado que o usuário desejava uma fatiação `PATCH + LIST` na slice 1. Validação no Linear mostrou que **`LIST /v1/groups` não está nos endpoints formais de GAR-393**:

> "Endpoints: POST /v1/groups, GET /v1/groups/{group_id}, PATCH /v1/groups/{group_id}, POST /v1/groups/{group_id}/invites, POST /v1/groups/{group_id}/members/{user_id}:setRole, DELETE /v1/groups/{group_id}/members/{user_id}"

Decisão operacional 2026-04-15: slice 1 = **apenas PATCH**. LIST fica para:
- (a) nova sub-issue Linear filha de GAR-393, ou
- (b) nova issue GAR-XXX standalone de epic `ws-api`, ou
- (c) descartado se o produto não precisar de listagem global de grupos (pode não precisar — cada usuário já tem `/v1/me` com `group_id` resolvido).

Qualquer uma das 3 rotas é válida; decisão fica para sessão posterior.

---

## Tamanho e shape

- **Escopo:** 7 tasks (Task 0..6) + 1 validation pass (Task 7). Maior que um slice trivial mas menor que plan 0016 (18 tasks).
- **Commits esperados:** 6 (Task 0 + Tasks 1..6 = 7 commits, Task 7 sem commit). PR pequeno e coerente.
- **LOC estimado:** ~150 linhas líquidas de produção + ~180 linhas de teste. Total ~330 LOC.
- **Review blast radius:** `rest_v1/*`, `garraia-auth/src/role.rs` (só se `from_str` faltar), `tests/rest_v1_groups.rs`, `tests/authz_http_matrix.rs`. Contido.
- **Security-auditor trigger:** SIM no PR — primeiro endpoint de mutação em recurso tenant-root + expansão de authz matrix. Conforme diretriz operacional, `@security-auditor` entra no review do PR, não na escrita do plan.
