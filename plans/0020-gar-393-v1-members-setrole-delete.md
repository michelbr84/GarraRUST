# Plan 0020 — GAR-393 Slice 4: `setRole` + `DELETE` member

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Linear issue:** [GAR-393](https://linear.app/chatgpt25/issue/GAR-393) — "Rotas POST/GET/PATCH /v1/groups com OpenAPI" (In Progress, High). **Slice 4 de GAR-393** — fecha a descrição canônica. Slice 3 (accept invite) entregue em plan 0019 (PR #25, `d590313`).

**Status:** Draft — pendente aprovação.

**Scope split note:** este plan é a fatia **0020-a** do split combinado com o usuário após o review do PR #25. A fatia **0020-b** (plan 0021 quando escrito — SEC-01 rate-limit do accept + SEC-04 audit_events do accept) é ortogonal e fica fora deste documento.

**Goal:** fechar o contrato de membership management: Owner/Admin pode mudar o papel de um membro existente (excluindo promoção para `owner`) e remover um membro do grupo via soft-delete (`status = 'removed'`). Também permite que qualquer membro se auto-remova (leave group), exceto quando fizer isso o deixaria o grupo sem nenhum owner.

## Architecture

1. **Dois novos handlers** em `crates/garraia-gateway/src/rest_v1/groups.rs` (não um módulo separado `members.rs` — path é `/v1/groups/{id}/members/*`, manter coesão):
   - `set_member_role` → `POST /v1/groups/{id}/members/{user_id}/setRole`
   - `delete_member` → `DELETE /v1/groups/{id}/members/{user_id}`
2. **Path shape (convention decision):** `/members/{user_id}/setRole` (dois segmentos para o verbo `setRole`). Mesma limitação do Axum 0.8 / `matchit` que forçou `/accept` em vez de `:accept` no plan 0019. Futuras custom actions seguem o mesmo ajuste.

   **Explicit divergence from Linear canonical description:**

   | Canônico (GAR-393 description) | Entregue por este plan |
   |--------------------------------|------------------------|
   | `POST .../{user_id}:setRole` | `POST /v1/groups/{id}/members/{user_id}/setRole` |
   | `DELETE .../{user_id}` | `DELETE /v1/groups/{id}/members/{user_id}` (inalterado) |

   Decisão ratificada em 2026-04-20 antes da execução do slice 4. Registrada em (1) este plan, (2) comentário de abertura do slice em GAR-393, (3) body do PR de código. A descrição canônica da issue **não será editada** — o custom-verb `:action` continua a notação de especificação; o `/action` continua a entrega real.
3. **Authz em duas camadas:**
   - **Camada 1 (capability):** `can(&principal, Action::MembersManage)` — filtra Owner+Admin. Member/Guest/Child → 403.
   - **Camada 2 (hierarchy, dentro do handler):** Admin não pode modificar Owner nem outros Admins. Owner pode modificar qualquer papel (respeitando last-owner invariant).
   - **Self-action bypass:** se `target_user_id == principal.user_id`, pula a camada 1 (permite Member/Guest/Child se auto-removerem via DELETE, e permite Admin/Owner se auto-demoterem respeitando a regra do último owner).
4. **Last-owner invariant:** toda UPDATE/DELETE que possa reduzir o número de owners ativos para zero retorna 409 Conflict. Verificado via `SELECT COUNT(*) FILTER (WHERE role = 'owner' AND status = 'active')` dentro da mesma transação.
5. **Promote-to-owner rejeição:** `set_member_role` com `role = 'owner'` retorna 400 Bad Request. Ownership transfer é operação distinta (fora do escopo do v1; mapeia para um futuro endpoint dedicado).
6. **DELETE é soft:** `UPDATE group_members SET status = 'removed' WHERE ...`. A linha permanece para preservar FKs (`messages.author_id`, `tasks.created_by`, etc.). Re-invite do mesmo user no mesmo grupo retornará 409 via colisão PK — limitação v1 documentada (plano 0022+ tratará reativação).
7. **Self-remove leave-group:** qualquer papel pode DELETE-se, sujeito ao last-owner invariant. Um Owner que tenta self-leave sem outro Owner no grupo → 409.

## Tech Stack

Axum 0.8, `utoipa 5`, `garraia-auth::{Principal, Action, can}`, `sqlx 0.8`, testcontainers + harness (inalterado desde plan 0016 M2).

## Design invariants

1. **Capability gate:** `MembersManage` requerido exceto em self-action (ver #2).
2. **Self-action bypass na camada de capability:** quando `target_user_id == principal.user_id`, o `can(&principal, MembersManage)` check é pulado. Mantém self-leave acessível a Member/Guest/Child.
3. **Hierarchy gate (não-self):** Admin não pode modificar Owner nem outros Admins. Tentativa → 403 Forbidden com detail PII-safe `"admin cannot modify owners or other admins"`. Owner pode modificar qualquer papel.
4. **Last-owner invariant:** `setRole` que demota o único Owner OU `DELETE` do único Owner → 409 Conflict com detail `"cannot leave the group without an owner"`. Verificado por `COUNT(*)` dentro da tx.
5. **No-promote-to-owner:** `setRole` com `role = 'owner'` → 400 Bad Request com detail `"cannot promote to owner via setRole"`. Reforçado defensivamente pelo `group_members_single_owner_idx` (migration 002) que rejeitaria o UPDATE com 23505 — mas handler filtra antes para mensagem de erro clara.
6. **Allowed setRole values:** `{'admin', 'member', 'guest', 'child'}`. `owner` rejeitado (invariant #5). Mesmo subset que `ALLOWED_INVITE_ROLES` do plan 0018 — reutilizar a constante se possível, renomear se o domínio ficar ambíguo.
7. **Target-must-exist-and-be-active:** `setRole` e `DELETE` operam apenas em linhas com `status = 'active'`. Target com `status IN ('invited','removed','banned')` ou não-existente → 404.
8. **X-Group-Id header exigido:** mesmo padrão de PATCH/invites. `Principal` extractor 403 não-membros; handler 400 em mismatch header/path.
9. **Transação atômica:** SET LOCAL tenant context + SELECT (hierarchy/last-owner guards) + UPDATE (setRole ou soft-delete). Tudo em uma tx para que as invariants sejam verificadas contra um snapshot consistente.
10. **Idempotência do DELETE:** `DELETE` de um member já com `status = 'removed'` retorna 404 (não foi encontrado como ativo). Cliente deve tratar 404 como "já removido ou nunca existiu".

## Pré-plan validations (gate 2)

- ✅ `group_members.status` CHECK inclui `'removed'` (migration 001 line 123: `CHECK (status IN ('active', 'invited', 'removed', 'banned'))`).
- ✅ `group_members_single_owner_idx` (migration 002 line 146) — partial unique index `WHERE role = 'owner'`. Bloqueia promote-to-owner a nível DB com 23505. Handler filtra antes para 400 com mensagem específica.
- ✅ `Action::MembersManage` existe em `garraia-auth` (action.rs line 36, can.rs line 55) e está mapeado para Owner+Admin.
- ✅ `Principal` extractor + `X-Group-Id` header pattern já usado por PATCH (0017) e invites (0018) — consistência garantida.
- ⏳ A ser validado durante T1: `garraia_app` role tem GRANT UPDATE em `group_members`? (Migration 007 / 010 grant verificar.)
- ⏳ A ser validado durante T1: `Principal` extractor filtra `group_members.status = 'active'` no lookup de membership? (Se não, um member com `status = 'removed'` passaria como membro ativo — precisa corrigir no extractor, não no handler.)

Se qualquer uma das validações pendentes (⏳) falhar, T1 reporta e ajusta o plano antes de avançar.

## Status codes

### `POST /v1/groups/{id}/members/{user_id}/setRole`

| Condition | Status | Guard |
|-----------|--------|-------|
| No JWT | 401 | Principal extractor |
| Non-member of group | 403 | Principal extractor |
| `X-Group-Id` missing/mismatch | 400 | handler |
| Body invalid (unknown role, `role = 'owner'`) | 400 | validate() |
| Caller not self AND lacks `MembersManage` | 403 | `can()` |
| Caller is Admin trying to modify Owner/Admin (non-self) | 403 | handler (hierarchy) |
| Target not found or not active | 404 | UPDATE RETURNING empty |
| Would demote last Owner | 409 | handler (last-owner invariant) |
| Happy path | 200 | |

### `DELETE /v1/groups/{id}/members/{user_id}`

| Condition | Status | Guard |
|-----------|--------|-------|
| No JWT | 401 | Principal extractor |
| Non-member of group | 403 | Principal extractor |
| `X-Group-Id` missing/mismatch | 400 | handler |
| Caller not self AND lacks `MembersManage` | 403 | `can()` |
| Caller is Admin trying to delete Owner/Admin (non-self) | 403 | handler (hierarchy) |
| Target not found or not active (already removed) | 404 | UPDATE RETURNING empty |
| Would leave group ownerless | 409 | handler (last-owner invariant) |
| Happy path | 204 | (No Content — no body) |

## How this closes GAR-393

A descrição canônica de GAR-393 lista 6 endpoints. Entregues até aqui:

- ✅ `POST /v1/groups` (plan 0016 M4)
- ✅ `GET /v1/groups/{id}` (plan 0016 M4)
- ✅ `PATCH /v1/groups/{id}` (plan 0017)
- ✅ `POST /v1/groups/{id}/invites` (plan 0018)
- ✅ `POST /v1/invites/{token}/accept` (plan 0019 — slice adicional)

Este plano (slice 4) entrega os 2 restantes:

- 🎯 `POST /v1/groups/{id}/members/{user_id}/setRole`
- 🎯 `DELETE /v1/groups/{id}/members/{user_id}`

Com plan 0020 merged, GAR-393 estará 6/6 conforme a descrição canônica e pode ser fechado no Linear. Os endpoints entregues por plan 0019 (accept) ficam como extensão natural documentada no comentário de scope-adjustment de GAR-393.

## Out of scope

- **Ownership transfer** (promover alguém para `owner`). Mapeia para um endpoint separado tipo `POST /v1/groups/{id}/transfer-ownership` com semântica atômica de demote-old-owner + promote-new-owner em uma tx. Plan futuro.
- **Hard delete + reativação** — rows soft-deleted permanecem. Um user soft-deleted que receber novo invite + accept vai hitar 23505 (PK collision) porque `(group_id, user_id)` já existe na tabela. Retornará 409 "already a member" (mensagem tecnicamente ambígua mas comportamento correto — caller não consegue re-entrar sem intervenção admin). Reativação explícita via UPDATE `status='active'` OU endpoint dedicado é trabalho de plan 0022+.
- **Audit trail** do setRole/delete member. SEC-04 já cobre o accept; um follow-up análogo para setRole/delete é recomendado mas fica para plan 0021 (0020-b) ou além.
- **Soft-delete cascading** para referências em `messages`, `tasks`, `memory_items`. Deliberadamente NÃO cascade — soft-delete preserva histórico.
- **Rate-limit** dedicado em setRole/delete. Governor global cobre.
- **Bulk operations** (setRole de múltiplos users em uma chamada). v2.

---

## File structure

| File | Action | Responsibility |
|------|--------|---------------|
| `crates/garraia-gateway/src/rest_v1/groups.rs` | Modify | Add `SetRoleRequest` struct, `set_member_role` handler, `delete_member` handler, shared `check_member_hierarchy` + `assert_last_owner_preserved` helpers, `ALLOWED_SETROLE_VALUES` constant |
| `crates/garraia-gateway/src/rest_v1/mod.rs` | Modify | Wire `/v1/groups/{id}/members/{user_id}/setRole` (POST) + `/v1/groups/{id}/members/{user_id}` (DELETE) in all 3 modes |
| `crates/garraia-gateway/src/rest_v1/openapi.rs` | Modify | Register `set_member_role` + `delete_member` paths, add `SetRoleRequest` schema |
| `crates/garraia-gateway/tests/rest_v1_groups.rs` | Modify | Add M1–M8 (setRole) + D1–D6 (DELETE) scenarios |
| `crates/garraia-gateway/tests/authz_http_matrix.rs` | Modify | Extend matrix with cases 27..N for setRole + DELETE |
| `plans/README.md` | Modify | Mark 0020 entry Draft → In Execution → Merged over lifecycle |

No migrations. No crate-level additions. No new dependencies.

---

## Task 1: Pré-plan gate validations + `SetRoleRequest` struct + hierarchy helpers

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/groups.rs`

**Gate (must run before any code):**

- [ ] **Step 0a: Verify `garraia_app` has UPDATE on `group_members`.**

```sql
-- Run via admin_pool in the harness or via migration grep.
SELECT has_table_privilege('garraia_app', 'group_members', 'UPDATE');
```

Expected: `true`. If `false`, add a migration 012 granting UPDATE before continuing — but from migration 007 + 010 the grant is expected to already exist.

- [ ] **Step 0b: Verify `Principal` extractor filters `group_members.status = 'active'`.**

Read `crates/garraia-auth/src/extractor.rs` (or wherever the membership lookup lives). The SELECT MUST filter `status = 'active'`. If it doesn't, a user with `status = 'removed'` would still be treated as a member — a security bug that this plan would inadvertently ship. If missing, fix it in the extractor BEFORE proceeding with the plan.

- [ ] **Step 1: Write failing unit tests for `SetRoleRequest`.**

In `groups.rs` `mod tests`:

```rust
#[test]
fn set_role_request_rejects_owner() {
    let req: SetRoleRequest = serde_json::from_str(r#"{"role":"owner"}"#).unwrap();
    assert_eq!(
        req.validate().unwrap_err(),
        "cannot promote to owner via setRole"
    );
}

#[test]
fn set_role_request_rejects_unknown_role() {
    let req: SetRoleRequest = serde_json::from_str(r#"{"role":"superadmin"}"#).unwrap();
    assert!(req.validate().is_err());
}

#[test]
fn set_role_request_accepts_admin_member_guest_child() {
    for role in &["admin", "member", "guest", "child"] {
        let req = SetRoleRequest { role: role.to_string() };
        assert!(req.validate().is_ok(), "role {role} should be valid");
    }
}

#[test]
fn set_role_request_rejects_unknown_field() {
    let err = serde_json::from_str::<SetRoleRequest>(r#"{"rle":"admin"}"#);
    assert!(err.is_err(), "deny_unknown_fields should reject typo");
}
```

Run: `cargo test -p garraia-gateway --lib -- rest_v1::groups::tests::set_role_request`. Expect FAIL (struct missing).

- [ ] **Step 2: Add `SetRoleRequest` + `ALLOWED_SETROLE_VALUES`.**

```rust
/// Accepted values for `SetRoleRequest::role`. Excludes `'owner'` —
/// ownership transfer is a separate operation (plan 0022+, not this plan).
/// Also excludes `'personal'` for the same reason `groups.type` excludes it.
const ALLOWED_SETROLE_VALUES: &[&str] = &["admin", "member", "guest", "child"];

#[derive(Debug, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct SetRoleRequest {
    /// New role. Must be one of: `admin`, `member`, `guest`, `child`.
    /// `owner` is rejected — ownership transfer is a separate operation.
    pub role: String,
}

impl SetRoleRequest {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.role == "owner" {
            return Err("cannot promote to owner via setRole");
        }
        if !ALLOWED_SETROLE_VALUES.contains(&self.role.as_str()) {
            return Err("role must be one of: admin, member, guest, child");
        }
        Ok(())
    }
}
```

- [ ] **Step 3: Run tests, expect pass.**

- [ ] **Step 4: Commit.**

```bash
git add crates/garraia-gateway/src/rest_v1/groups.rs
git commit -m "feat(gateway): SetRoleRequest struct + validation (plan 0020 t1)"
```

---

## Task 2: `set_member_role` handler

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/groups.rs`

- [ ] **Step 1: Write the handler.**

High-level sequence (details in the code comments):

```rust
pub async fn set_member_role(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((id, target_user_id)): Path<(Uuid, Uuid)>,
    Json(body): Json<SetRoleRequest>,
) -> Result<Json<MemberResponse>, RestError> {
    // 1. Header/path coherence.
    // 2. Structural body validation (no DB).
    // 3. Authz camada 1: self-action bypass OR MembersManage.
    // 4. Open tx, SET LOCAL tenant context.
    // 5. Fetch target's current role (FOR UPDATE to serialize concurrent setRole).
    //    -> NOT FOUND if missing or status != 'active'.
    // 6. Hierarchy check (camada 2): Admin caller cannot modify Owner/Admin (non-self).
    // 7. UPDATE group_members SET role = $new_role WHERE group_id=$1 AND user_id=$2 AND status='active'.
    // 8. Last-owner invariant check (POST-UPDATE): COUNT owners >= 1. If not, abort tx (return 409).
    //    NOTE: uncommitted tx sees its own writes, so COUNT reflects post-UPDATE state.
    // 9. Commit tx.
    // 10. Return 200 with MemberResponse { group_id, user_id, role, status, updated_at }.
}
```

Key notes:

- **FOR UPDATE in step 5** (`SELECT ... FOR UPDATE`) acquires a row lock on the target member. Prevents two concurrent setRole calls from racing: the second one blocks until the first commits, then sees the new role and can re-evaluate its own hierarchy check.
- **Last-owner check in step 8** happens INSIDE the tx. If the UPDATE demoted the last owner, `COUNT(*) FILTER (WHERE role='owner' AND status='active') = 0`. Handler returns `RestError::Conflict` → tx drops without commit → UPDATE is rolled back. Invariant preserved.
- **Self-demote special case** (caller == target, role was owner, new role isn't owner): falls through the hierarchy check (self-bypass) but hits the last-owner check. If caller is the sole owner, 409. If there's another owner, proceeds.

- [ ] **Step 2: Add `MemberResponse` ToSchema struct** (shared with delete_member for the 200 body in setRole; DELETE returns 204 and has no body).

```rust
#[derive(Debug, Serialize, ToSchema)]
pub struct MemberResponse {
    pub group_id: Uuid,
    pub user_id: Uuid,
    pub role: String,
    pub status: String,
    pub updated_at: DateTime<Utc>,
}
```

- [ ] **Step 3: Verify compilation.**

`cargo check -p garraia-gateway`. Expected: fails at route wiring (next task), OR compiles clean if route wiring is deferred. Either way acceptable — the handler must typecheck.

- [ ] **Step 4: Commit.**

```bash
git add crates/garraia-gateway/src/rest_v1/groups.rs
git commit -m "feat(gateway): set_member_role handler with hierarchy + last-owner guards (plan 0020 t2)"
```

---

## Task 3: `delete_member` handler

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/groups.rs`

- [ ] **Step 1: Write the handler.**

```rust
pub async fn delete_member(
    State(state): State<RestV1FullState>,
    principal: Principal,
    Path((id, target_user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, RestError> {
    // 1. Header/path coherence.
    // 2. Authz camada 1: self-action bypass OR MembersManage.
    // 3. Open tx, SET LOCAL tenant context.
    // 4. Fetch target's role (FOR UPDATE).
    //    -> NOT FOUND if missing or status != 'active' (soft-deleted is idempotent 404).
    // 5. Hierarchy check (camada 2): Admin cannot delete Owner/Admin (non-self).
    // 6. UPDATE group_members SET status='removed' WHERE group_id=$1 AND user_id=$2 AND status='active'.
    // 7. Last-owner invariant: if the removed role was 'owner', COUNT owners FILTER status='active' >= 1; else abort tx.
    // 8. Commit tx.
    // 9. Return 204 No Content.
}
```

Notes:

- **Idempotent semantics:** DELETE on an already-removed member returns 404 (not 204). Some REST styles prefer 204 for idempotence on DELETE, but here 404 is cleaner because the caller can distinguish "I performed the removal" (204) from "already gone or never existed" (404). Plan 0019 followed the same convention (accept-twice → 404 via filtered SELECT).
- **Soft-delete only:** `UPDATE status='removed'` preserves the row. Existing FKs in `messages`, `tasks`, etc. continue to resolve.
- **Optimization opportunity (deferred):** could skip the last-owner check when the removed role is NOT `'owner'`. Current plan runs the COUNT unconditionally for simplicity — cost is ~µs on any realistic group size.

- [ ] **Step 2: Verify compilation.**

`cargo check -p garraia-gateway`.

- [ ] **Step 3: Commit.**

```bash
git add crates/garraia-gateway/src/rest_v1/groups.rs
git commit -m "feat(gateway): delete_member handler (soft-delete + last-owner guard) (plan 0020 t3)"
```

---

## Task 4: Route wiring + OpenAPI registration

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/mod.rs`
- Modify: `crates/garraia-gateway/src/rest_v1/openapi.rs`

- [ ] **Step 1: Wire routes in all 3 modes.**

Mode 1 (full state):

```rust
.route(
    "/v1/groups/{id}/members/{user_id}/setRole",
    post(groups::set_member_role),
)
.route(
    "/v1/groups/{id}/members/{user_id}",
    delete(groups::delete_member),
)
```

Modes 2 and 3: mirror with `unconfigured_handler`.

- [ ] **Step 2: Register in OpenAPI.**

In `openapi.rs`:
- Add to `paths(...)`: `super::groups::set_member_role`, `super::groups::delete_member`.
- Add to `components(schemas(...))`: `SetRoleRequest`, `MemberResponse`.
- Update the `use super::groups::{...}` import block.

- [ ] **Step 3: Verify compilation + lib tests.**

`cargo check -p garraia-gateway && cargo test -p garraia-gateway --lib`. Expected: compiles, lib tests still pass (unit tests from T1 already pass).

- [ ] **Step 4: Commit.**

```bash
git add crates/garraia-gateway/src/rest_v1/mod.rs crates/garraia-gateway/src/rest_v1/openapi.rs
git commit -m "feat(gateway): wire setRole + DELETE member routes + OpenAPI (plan 0020 t4)"
```

---

## Task 5: Integration tests — setRole scenarios (M1–M8)

**Files:**
- Modify: `crates/garraia-gateway/tests/rest_v1_groups.rs`

Scenarios (all bundled into the existing `v1_groups_scenarios` test function or a new sibling — decision: **new sibling** `v1_groups_member_scenarios` because `v1_groups_scenarios` is already dense):

| # | Name | Expected |
|---|---|---|
| M1 | Owner setRole member→admin (happy) | 200, `MemberResponse.role == "admin"` |
| M2 | Admin setRole member→guest | 200 |
| M3 | Admin tries setRole of Owner | 403 "admin cannot modify owners or other admins" |
| M4 | Admin tries setRole of another Admin (non-self) | 403 |
| M5 | Owner self-demote to admin with another owner existing | 200 (requires seeding a 2nd owner via admin_pool) |
| M6 | Owner self-demote to admin without another owner | 409 "cannot leave the group without an owner" |
| M7 | Setrole body `role="owner"` | 400 "cannot promote to owner via setRole" |
| M8 | Member tries setRole of another member (non-self, lacks `MembersManage`) | 403 |
| M9 | Target user_id not a member | 404 |
| M10 | Missing bearer | 401 |

Note: M5 needs a fixture helper `seed_second_owner_via_admin(&h, group_id, email)` because the product API does not expose a way to create two owners (by design). Can be added in `common/fixtures.rs` as a test-only utility.

- [ ] **Step 1: Write all 10 scenarios bundled into one `#[tokio::test]`** (same pattern as plan 0018/0019 — avoids sqlx teardown race).

- [ ] **Step 2: Run**

`cargo test -p garraia-gateway --features test-helpers --test rest_v1_groups`. Expected: all M scenarios + all pre-existing 1..7 scenarios + P1..P6 + I1..I6 pass. Total tests in file: ~30.

- [ ] **Step 3: Commit.**

```bash
git add crates/garraia-gateway/tests/rest_v1_groups.rs crates/garraia-gateway/tests/common/fixtures.rs
git commit -m "test(gateway): setRole integration scenarios M1-M10 + seed_second_owner fixture (plan 0020 t5)"
```

---

## Task 6: Integration tests — DELETE member scenarios (D1–D6)

**Files:**
- Modify: `crates/garraia-gateway/tests/rest_v1_groups.rs`

| # | Name | Expected |
|---|---|---|
| D1 | Owner DELETE member (happy) | 204, admin_pool assertion: `group_members.status = 'removed'` |
| D2 | Admin DELETE guest | 204 |
| D3 | Admin tries DELETE of Owner | 403 |
| D4 | Owner self-delete without another owner | 409 |
| D5 | Owner self-delete with another owner seeded | 204 (owner leaves; co-owner remains) |
| D6 | Member self-delete (leave group) | 204 |
| D7 | DELETE of already-removed member | 404 |
| D8 | DELETE target not a member | 404 |
| D9 | Missing bearer | 401 |

- [ ] **Step 1: Add scenarios to the same bundled test.**

- [ ] **Step 2: Run + verify.**

- [ ] **Step 3: Commit.**

```bash
git add crates/garraia-gateway/tests/rest_v1_groups.rs
git commit -m "test(gateway): DELETE member integration scenarios D1-D9 (plan 0020 t6)"
```

---

## Task 7: authz matrix expansion

**Files:**
- Modify: `crates/garraia-gateway/tests/authz_http_matrix.rs`

Current matrix: 26 cases. Add:

| # | Name | Expected |
|---|---|---|
| 27 | POST setRole as alice (owner) on bob (non-member of alice_group) | 404 |
| 28 | POST setRole as bob (non-member of alice_group) on alice | 403 |
| 29 | POST setRole as eve (no group) on alice | 403 |
| 30 | POST setRole no bearer | 401 |
| 31 | DELETE member as alice on bob (non-member of alice_group) | 404 |
| 32 | DELETE member as bob (non-member) on alice | 403 |
| 33 | DELETE member as eve on alice | 403 |
| 34 | DELETE member no bearer | 401 |

Update `assert_eq!(matrix.len(), 34, ...)`.

- [ ] **Step 1: Append 8 cases, update len assertion.**
- [ ] **Step 2: `cargo test --features test-helpers --test authz_http_matrix`.**
- [ ] **Step 3: Commit.**

```bash
git add crates/garraia-gateway/tests/authz_http_matrix.rs
git commit -m "test(gateway): expand authz matrix with setRole + DELETE cases 27-34 (plan 0020 t7)"
```

---

## Task 8: Full validation pass

**Files:** none — validation only.

- [ ] **Step 1: `cargo fmt --check --all`.** Fix any diffs.
- [ ] **Step 2: `cargo clippy -p garraia-gateway --no-deps --features test-helpers --tests -- -D warnings`.** Must not introduce new warnings in `groups.rs`. Pre-existing warnings in `bootstrap.rs` stay tolerated via CI `continue-on-error`.
- [ ] **Step 3: Full test matrix:**

```bash
cargo test -p garraia-gateway --lib
cargo test -p garraia-gateway --features test-helpers --test rest_v1_groups
cargo test -p garraia-gateway --features test-helpers --test rest_v1_invites
cargo test -p garraia-gateway --features test-helpers --test authz_http_matrix
cargo test -p garraia-gateway --features test-helpers --test rest_v1_me_authed
cargo test -p garraia-gateway --features test-helpers --test rest_v1_me
cargo test -p garraia-gateway --features test-helpers --test harness_smoke
cargo test -p garraia-gateway --features test-helpers --test router_smoke_test
```

Every binary must pass.

- [ ] **Step 4: Commit any cleanups.**

```bash
git add -u
git commit -m "style(gateway): validation pass fixes (plan 0020 t8)"
```

---

## Acceptance criteria

1. `POST /v1/groups/{id}/members/{user_id}/setRole` returns 200 with `MemberResponse` on happy path.
2. `setRole` rejects `role = "owner"` with 400 Bad Request and detail `"cannot promote to owner via setRole"`.
3. `setRole` that would demote the last owner returns 409 Conflict with detail `"cannot leave the group without an owner"` and DB state unchanged.
4. Admin cannot `setRole` or `DELETE` an Owner or another Admin (non-self) — 403 Forbidden.
5. `DELETE /v1/groups/{id}/members/{user_id}` returns 204 No Content and `UPDATE group_members SET status='removed'` succeeded.
6. `DELETE` of the last owner returns 409 with DB unchanged; DELETE of an already-removed member returns 404.
7. Member/Guest/Child can self-remove (DELETE self) with 204 — last-owner rule still applies if they happened to be the sole owner.
8. `Principal` extractor membership check filters `status='active'` — a user with `status='removed'` gets 403 on subsequent requests.
9. All authz matrix cases 1..34 pass.
10. CI goes 9/9 green.
11. `cargo fmt --check --all` clean.
12. OpenAPI spec at `/docs` shows both new endpoints.

## Rollback plan

All changes additive:
- 2 new handlers (`set_member_role`, `delete_member`)
- 1 new request struct (`SetRoleRequest`)
- 1 new response struct (`MemberResponse`)
- 1 new fixture helper (`seed_second_owner_via_admin`)
- route wiring in 3 modes (3 new route calls each for setRole and DELETE)
- test scenarios (additive)

**Rollback via revert:** reverting the squash commit returns the codebase to plan 0019's state. No migrations, no schema changes, no state dependencies outside git.

**Partial rollback:** if only one of setRole/DELETE proves problematic post-merge, a follow-up commit can remove the wiring of the affected route (keeping the handler in code but inaccessible) while a proper fix is authored.

## Open questions

- **OQ-1:** Does `Principal` extractor already filter `status = 'active'`? (Gate 2, T1 Step 0b.) If no, we fix it in the extractor, not in this plan — that's an auth-crate concern.
- **OQ-2:** Does `garraia_app` have UPDATE grant on `group_members`? (Gate 2, T1 Step 0a.)
- **OQ-3:** Should DELETE of last owner be 409 or 422? Chosen **409** (Conflict) because it matches the semantic "you conflict with the invariant" — same reasoning used for double-accept race in plan 0019.
- **OQ-4:** Self-remove: should it require a confirmation header (e.g. `X-Leave-Confirm: true`)? **No for v1** — keeps the API simple. Add if UX feedback suggests accidental self-leaves are a problem.

## Relationship to other plans

- **Plan 0018** (create invite) — produces the invites that plan 0019 accepts.
- **Plan 0019** (accept invite) — produces the `group_members` rows that this plan modifies/soft-deletes.
- **Plan 0021** (TBD, "0020-b") — absorb SEC-01 (rate-limit accept) + SEC-04 (audit_events accept). Ortogonal.
- **Plan 0022+** — ownership transfer endpoint, member reactivation (hard-revive from `status='removed'`), audit trail for membership changes.

Com plan 0020 merged, GAR-393 estará 6/6 conforme a descrição canônica (+ 1 slice adicional accept-invite) e pode ser fechado no Linear.
