# Plan 0021 — GAR-425: Workspace security hardening (audit events + rate-limit + single-owner index amendment)

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Linear issue:** [GAR-425](https://linear.app/chatgpt25/issue/GAR-425) — "Workspace security hardening: audit events + members/invites rate limit + single-owner index amendment" (In Progress, High, labels `security` + `epic:ws-api`, project Fase 3 — Group Workspace).

**Status:** Draft — pendente aprovação.

**Relationship note:** este plan é o "slice 0020-b" do split acordado durante o review de PR #25. O escopo consolida 3 follow-ups deferidos dos reviews de plan 0019 (PR #25) e plan 0020 (PR #27) em um slice pequeno e cirúrgico, fechando as dívidas técnica e de segurança abertas antes de avançar para os próximos pilares (GAR-410 CredentialVault, GAR-379 garraia-config crate, GAR-400 LGPD endpoints).

**Goal:** fechar os 3 follow-ups de segurança do Group Workspace REST surface:

1. **Migration 012** — amend do partial UNIQUE index `group_members_single_owner_idx` para filtrar `status = 'active'` (alinha constraint DB ao invariant app-layer).
2. **Audit trail** — integração de `audit_events` em `accept_invite`, `set_member_role`, `delete_member`.
3. **Rate-limit dedicado** — preset `members_manage` (stricter que o global governor) para as 3 rotas privilegiadas.

## Architecture

### 1. Migration 012 (schema)

Nova migration `012_single_owner_idx_active_only.sql` em `crates/garraia-workspace/migrations/`. Forward-only:

```sql
-- Drop the old predicate (WHERE role = 'owner' only).
DROP INDEX IF EXISTS group_members_single_owner_idx;

-- Recreate with status filter so soft-deleted owners no longer
-- consume the single-owner slot. Aligns the DB-level constraint
-- with the app-layer last-owner invariant in set_member_role /
-- delete_member (which filters status = 'active' in the COUNT).
CREATE UNIQUE INDEX group_members_single_owner_idx
    ON group_members (group_id)
    WHERE role = 'owner' AND status = 'active';

COMMENT ON INDEX group_members_single_owner_idx IS
    'Partial UNIQUE — at most one ACTIVE owner per group. Plan 0021 amended '
    'the predicate from WHERE role = owner to WHERE role = owner AND status = active '
    'so soft-deleted owner rows do not block reactivation or leave-group flows. '
    'In production the original gap was unreachable (API rejects promote-to-owner '
    'and the app-layer COUNT filters status=active), but the test suite D5 had '
    'to hard-delete soft-deleted owner rows to rebuild the index.';
```

### 2. Audit trail (`garraia-auth` extension)

Duas opções consideradas:

- **(a) Renomear `audit_login` → módulo genérico `audit` com função `insert_event`.** Mais limpo a longo prazo, mas quebra call sites no login flow.
- **(b) Adicionar um módulo irmão `audit_workspace` em `garraia-auth/src/audit/` separado de `audit_login`.** Mantém o login flow intacto, isolado o novo código, e evita ripple changes.

**Decisão: (b).** Login flow já tem 6 variants estáveis e testes próprios. Misturar agora só adiciona risco. A refatoração (a) pode vir em um plan futuro se valer a pena.

Nova estrutura proposta:

```text
crates/garraia-auth/src/audit/
  mod.rs              — re-exports the two sub-modules below
  login.rs            — (renamed from audit.rs) AuditAction + audit_login (unchanged behavior)
  workspace.rs        — NEW: WorkspaceAuditAction + audit_workspace_event helper
```

Actually even simpler: create `audit_workspace.rs` as a sibling of the existing `audit.rs` file. No module restructuring — just two flat files, each `pub use`d from `lib.rs`. Prevents unnecessary churn.

```rust
// crates/garraia-auth/src/audit_workspace.rs
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::error::AuthError;

#[derive(Debug, Clone, Copy)]
pub enum WorkspaceAuditAction {
    /// Invite accepted: new group_members row created via POST /v1/invites/{token}/accept.
    InviteAccepted,
    /// Role changed via POST /v1/groups/{id}/members/{user_id}/setRole.
    MemberRoleChanged,
    /// Member soft-deleted via DELETE /v1/groups/{id}/members/{user_id}.
    MemberRemoved,
}

impl WorkspaceAuditAction {
    pub fn as_str(self) -> &'static str {
        match self {
            WorkspaceAuditAction::InviteAccepted => "invite.accepted",
            WorkspaceAuditAction::MemberRoleChanged => "member.role_changed",
            WorkspaceAuditAction::MemberRemoved => "member.removed",
        }
    }
}

/// Insert a workspace audit_events row inside the caller's transaction.
///
/// `actor_user_id` is always Some — all three workspace actions have a
/// resolved caller (Principal extractor already ran).
/// `group_id` is always Some — all three actions are group-scoped.
/// `resource_type` is the semantic table ("group_invites" for accept,
/// "group_members" for setRole/delete).
/// `resource_id` is the primary key of the resource acted upon.
/// `metadata` carries the diff (old_role, new_role, target_user_id, etc.).
pub async fn audit_workspace_event(
    tx: &mut Transaction<'_, Postgres>,
    action: WorkspaceAuditAction,
    actor_user_id: Uuid,
    group_id: Uuid,
    resource_type: &'static str,
    resource_id: String,
    metadata: Value,
) -> Result<(), AuthError> {
    sqlx::query(
        "INSERT INTO audit_events \
             (group_id, actor_user_id, action, resource_type, resource_id, metadata) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(group_id)
    .bind(actor_user_id)
    .bind(action.as_str())
    .bind(resource_type)
    .bind(resource_id)
    .bind(metadata)
    .execute(&mut **tx)
    .await
    .map_err(|e| AuthError::Database(e.into()))?;
    Ok(())
}
```

Integração nos 3 handlers — 1 INSERT por happy path, dentro da tx existente, depois da mutation bem-sucedida mas antes do COMMIT.

### 3. Rate-limit dedicado

Novo preset em `rate_limiter.rs`:

```rust
impl RateLimitConfig {
    /// Strict config for privileged members management endpoints.
    /// Stricter than default (60/min) to defend against enumeration/
    /// brute-force of invite tokens (SEC-01 from plan 0019) and
    /// excessive role-changes (plan 0020 security review).
    pub fn members_manage() -> Self {
        Self {
            requests_per_minute: 20,
            requests_per_hour: 200,
            burst_size: 5,
        }
    }
}
```

Wire via `axum::middleware::from_fn` em `rest_v1/mod.rs`. Preserve a key-extractor hierarchy: JWT user_id quando disponível, fallback para IP. Headers `X-RateLimit-*` já emitidos pelo middleware existente.

## Tech Stack

- Rust (Axum 0.8, sqlx 0.8, postgres 16 + pgvector).
- Migration harness já existente (`crates/garraia-workspace`).
- `garraia-auth` (audit_login pattern, 2 novos arquivos irmãos).
- `garraia-gateway::rate_limiter` (sliding-window DashMap).
- testcontainers + harness (inalterado desde plan 0016 M2).

## Design invariants

1. **Audit atomicity:** INSERT audit acontece DENTRO da tx do handler, depois da mutation bem-sucedida. Rollback da tx descarta o audit também — consistência total. Mesma abordagem do login flow.
2. **No PII leakage:** `metadata` jsonb carrega apenas UUIDs e role strings. Email apenas em `actor_label` quando disponível (consistente com o login flow). Nenhum payload bruto (ex: token plaintext) nunca é logado.
3. **Rate-limit key preference:** `user_id` do JWT > IP. Evita NAT-bucketing onde múltiplos users atrás do mesmo IP compartilham a mesma window.
4. **Migration idempotência:** `DROP IF EXISTS` + `CREATE`. Re-run seguro.
5. **Backward compatibility:** zero mudança no contrato HTTP. Adições apenas no comportamento lateral (audit row + 429 em excessive calls).
6. **RLS compliance:** `audit_events` está sob FORCE RLS (migration 007). Handler deve `SET LOCAL app.current_user_id` antes do INSERT (pattern já existente, nenhum código novo necessário — INSERT herda a tx do handler).
7. **Test isolation:** testes de audit-row presence usam `admin_pool` (bypass RLS) para SELECT de confirmação, consistente com o pattern `seed_*_via_admin`.
8. **Rate-limit test isolation:** testes de 429 são isolados (novo user_id por cenário) para evitar contaminação entre testes.

## Pré-plan validations (gate 2)

- [ ] **Gate 0a: `audit_events` tem grants corretos para `garraia_app`?** Migration 007 linha 70 já dá `INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO garraia_app` — cobre `audit_events`. A ser re-verificado em T1 com `has_table_privilege('garraia_app', 'audit_events', 'INSERT')`.
- [ ] **Gate 0b: RLS policy em `audit_events` permite INSERT pelo `garraia_app` com `current_user_id` setado?** Migration 007 adiciona policy; ler `audit_events_insert_policy` e confirmar `WITH CHECK` permite INSERT quando `group_id` pertence ao user. Se a policy for mais restrita, ajustar antes de T2.
- [ ] **Gate 0c: `rate_limiter::GovernorMiddleware` (ou nome equivalente) aceita múltiplas instâncias com configs diferentes por rota?** Ler `rate_limiter.rs` + `auth_routes.rs` para entender wire pattern. Se todas as rotas compartilharem uma única instância global, precisamos refatorar antes de T6. (Expectativa: aceita por design; verificado na auditoria inicial.)
- [ ] **Gate 0d: Principal extractor expõe `actor_label` (email) para audit?** Se não, audit rows terão `actor_label = NULL` (aceito v1, documentado como limitação).

Se qualquer gate falhar, T1 reporta e ajusta o plano antes de avançar.

## File structure

| File | Action | Responsibility |
|------|--------|---------------|
| `crates/garraia-workspace/migrations/012_single_owner_idx_active_only.sql` | Create | New migration — amend partial UNIQUE index |
| `crates/garraia-auth/src/audit_workspace.rs` | Create | `WorkspaceAuditAction` enum + `audit_workspace_event` helper |
| `crates/garraia-auth/src/lib.rs` | Modify | `pub mod audit_workspace;` + re-exports |
| `crates/garraia-gateway/src/rest_v1/invites.rs` | Modify | Call `audit_workspace_event` in `accept_invite` happy path |
| `crates/garraia-gateway/src/rest_v1/groups.rs` | Modify | Call `audit_workspace_event` in `set_member_role` + `delete_member` happy paths |
| `crates/garraia-gateway/src/rate_limiter.rs` | Modify | `RateLimitConfig::members_manage()` preset |
| `crates/garraia-gateway/src/rest_v1/mod.rs` | Modify | Apply the new rate-limit middleware to 3 routes (full mode only) |
| `crates/garraia-gateway/tests/rest_v1_groups.rs` | Modify | Add audit-row assertions to M1/D1/D5; simplify D5 restore |
| `crates/garraia-gateway/tests/rest_v1_invites.rs` | Modify | Add audit-row assertion + new A1 (429 burst) scenario |
| `crates/garraia-gateway/tests/common/fixtures.rs` | Modify | Add `fetch_audit_events_for_group` helper |
| `plans/README.md` | Modify | Mark 0021 Draft → In Execution → Merged over lifecycle |

**No changes to:** handler logic outside the audit/rate-limit additions, OpenAPI (no contract changes), auth extractor, RLS policies.

## Task 1: Pré-plan gates + migration 012 + schema assertion test

**Files:**
- Create: `crates/garraia-workspace/migrations/012_single_owner_idx_active_only.sql`

**Gate (must run before migration creation):**

- [ ] **Step 0a: Verify `garraia_app` has INSERT on `audit_events`.**
  ```sql
  SELECT has_table_privilege('garraia_app', 'audit_events', 'INSERT');
  ```
  Expected: `true` via schema-wide grant from migration 007:70.

- [ ] **Step 0b: Verify `audit_events` RLS policy allows app INSERT with current_user_id set.**
  Read `crates/garraia-workspace/migrations/007_row_level_security.sql`, find `audit_events_*_policy` definitions, confirm INSERT is allowed when `group_id = current_setting(app.current_group_id)::uuid` OR when `actor_user_id = current_setting(app.current_user_id)::uuid`. Document which predicate fires.

- [ ] **Step 0c: Write failing integration test for the new index predicate.**
  ```rust
  // In tests/rest_v1_groups.rs or a new schema_smoke.rs:
  let (indexdef,): (String,) = sqlx::query_as(
      "SELECT indexdef FROM pg_indexes \
       WHERE indexname = 'group_members_single_owner_idx'"
  )
  .fetch_one(&h.admin_pool)
  .await
  .expect("index must exist");
  assert!(
      indexdef.contains("status = 'active'"),
      "plan 0021 migration 012 must amend predicate to include status='active'; got: {indexdef}"
  );
  ```
  Run: expect FAIL (migration not yet created).

- [ ] **Step 1: Create `012_single_owner_idx_active_only.sql`** with the SQL shown in §Architecture.

- [ ] **Step 2: Run harness `cargo test --features test-helpers --test harness_smoke`.** Migration auto-applies on harness boot. Run Step 0c assertion — expect PASS now.

- [ ] **Step 3: Commit.**
  ```bash
  git add crates/garraia-workspace/migrations/012_single_owner_idx_active_only.sql \
          crates/garraia-gateway/tests/<test_file>.rs
  git commit -m "feat(workspace): migration 012 — amend single-owner idx predicate (plan 0021 t1)"
  ```

---

## Task 2: `WorkspaceAuditAction` enum + `audit_workspace_event` helper

**Files:**
- Create: `crates/garraia-auth/src/audit_workspace.rs`
- Modify: `crates/garraia-auth/src/lib.rs`

- [ ] **Step 1: Write failing unit tests.**
  ```rust
  #[test]
  fn workspace_audit_action_as_str_stable() {
      assert_eq!(WorkspaceAuditAction::InviteAccepted.as_str(), "invite.accepted");
      assert_eq!(WorkspaceAuditAction::MemberRoleChanged.as_str(), "member.role_changed");
      assert_eq!(WorkspaceAuditAction::MemberRemoved.as_str(), "member.removed");
  }
  ```
  Run: FAIL (enum missing).

- [ ] **Step 2: Implement `audit_workspace.rs`** with the code in §Architecture.

- [ ] **Step 3: Re-export from `lib.rs`:**
  ```rust
  pub mod audit_workspace;
  pub use audit_workspace::{WorkspaceAuditAction, audit_workspace_event};
  ```

- [ ] **Step 4: Run tests — expect PASS.**

- [ ] **Step 5: `cargo check -p garraia-auth` + `cargo check -p garraia-gateway`.**

- [ ] **Step 6: Commit.**
  ```bash
  git add crates/garraia-auth/src/audit_workspace.rs crates/garraia-auth/src/lib.rs
  git commit -m "feat(auth): WorkspaceAuditAction + audit_workspace_event helper (plan 0021 t2)"
  ```

---

## Task 3: Integrate audit in `accept_invite`

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/invites.rs`

- [ ] **Step 1:** Add call to `audit_workspace_event` after the INSERT into `group_members` in the happy path, before `tx.commit()`.
  ```rust
  // After INSERT INTO group_members succeeded:
  garraia_auth::audit_workspace_event(
      &mut tx,
      WorkspaceAuditAction::InviteAccepted,
      principal.user_id,
      invite.group_id,
      "group_invites",
      invite.id.to_string(),
      json!({
          "invited_email": invite.invited_email,
          "proposed_role": invite.proposed_role,
      }),
  )
  .await
  .map_err(|e| RestError::Internal(e.into()))?;
  ```

- [ ] **Step 2:** `cargo check -p garraia-gateway`.

- [ ] **Step 3: Commit.**
  ```bash
  git add crates/garraia-gateway/src/rest_v1/invites.rs
  git commit -m "feat(gateway): audit invite.accepted row in accept_invite (plan 0021 t3)"
  ```

---

## Task 4: Integrate audit in `set_member_role`

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/groups.rs`

- [ ] **Step 1:** Call `audit_workspace_event` after the UPDATE succeeds but before the last-owner COUNT (so a 409 rolls back both UPDATE and audit — consistent atomicity).

Alternatively: after the COUNT guard passes. The trade-off is whether a rolled-back 409 leaves an audit row or not. **Decision: after the COUNT guard passes**, so only committed mutations produce audit rows. This matches the login-flow pattern (audit only for events that stick).

  ```rust
  // After the COUNT guard passes, before tx.commit():
  garraia_auth::audit_workspace_event(
      &mut tx,
      WorkspaceAuditAction::MemberRoleChanged,
      principal.user_id,
      id, // group_id
      "group_members",
      format!("{}:{}", id, target_user_id),
      json!({
          "target_user_id": target_user_id,
          "old_role": target_role,  // captured from the FOR UPDATE SELECT
          "new_role": body.role,
      }),
  )
  .await
  .map_err(|e| RestError::Internal(e.into()))?;
  ```

- [ ] **Step 2:** `cargo check -p garraia-gateway`.

- [ ] **Step 3: Commit.**

---

## Task 5: Integrate audit in `delete_member`

**Files:**
- Modify: `crates/garraia-gateway/src/rest_v1/groups.rs`

- [ ] **Step 1:** Same pattern as T4 — after COUNT guard passes, before COMMIT.
  ```rust
  garraia_auth::audit_workspace_event(
      &mut tx,
      WorkspaceAuditAction::MemberRemoved,
      principal.user_id,
      id, // group_id
      "group_members",
      format!("{}:{}", id, target_user_id),
      json!({
          "target_user_id": target_user_id,
          "old_role": target_role,
      }),
  )
  .await
  .map_err(|e| RestError::Internal(e.into()))?;
  ```

- [ ] **Step 2:** `cargo check -p garraia-gateway`.

- [ ] **Step 3: Commit.**

---

## Task 6: `RateLimitConfig::members_manage` + wire middleware

**Files:**
- Modify: `crates/garraia-gateway/src/rate_limiter.rs`
- Modify: `crates/garraia-gateway/src/rest_v1/mod.rs`

- [ ] **Step 1:** Add `members_manage()` preset to `RateLimitConfig`.

- [ ] **Step 2:** Wire middleware on the 3 routes in mode 1 (full state). Modes 2/3 (fail-soft) skip the middleware — no router, no rate-limit concern.
  ```rust
  // In rest_v1/mod.rs mode 1:
  let members_manage_rl = RateLimiter::new(RateLimitConfig::members_manage());
  Router::new()
      // ...
      .route(
          "/v1/groups/{id}/members/{user_id}/setRole",
          post(groups::set_member_role).layer(members_manage_rl.clone().into_layer()),
      )
      .route(
          "/v1/groups/{id}/members/{user_id}",
          delete(groups::delete_member).layer(members_manage_rl.clone().into_layer()),
      )
      .route(
          "/v1/invites/{token}/accept",
          post(invites::accept_invite).layer(members_manage_rl.into_layer()),
      )
      // ...
  ```
  *(Exact middleware wiring to be confirmed against current `rate_limiter.rs` API during T6 — may differ from the snippet above. If the current limiter is a `from_fn` middleware and not a tower layer, adjust accordingly.)*

- [ ] **Step 3:** `cargo check -p garraia-gateway`.

- [ ] **Step 4: Commit.**

---

## Task 7: Simplify D5 test restore

**Files:**
- Modify: `crates/garraia-gateway/tests/rest_v1_groups.rs`

With migration 012 applied, the partial UNIQUE index filters `WHERE role = 'owner' AND status = 'active'`. A soft-deleted owner (`status = 'removed'`) no longer counts toward uniqueness, so recreating the index with 1 active owner + 1 removed owner works without hard-deleting the removed row.

- [ ] **Step 1:** Remove the `DELETE FROM group_members WHERE ...` step from D5's restore block.
- [ ] **Step 2:** Keep only `restore_single_owner_idx(&h).await.expect(...)`.
- [ ] **Step 3:** Run `cargo test --test rest_v1_groups` — expect PASS.
- [ ] **Step 4: Commit.**

---

## Task 8: Integration tests — audit rows + 429 burst

**Files:**
- Modify: `crates/garraia-gateway/tests/common/fixtures.rs`
- Modify: `crates/garraia-gateway/tests/rest_v1_groups.rs`
- Modify: `crates/garraia-gateway/tests/rest_v1_invites.rs`

- [ ] **Step 1:** Add fixture helper `fetch_audit_events_for_group(h, group_id) -> Vec<(String, Uuid, Value)>` that reads via admin_pool (bypassing RLS) and returns `(action, actor_user_id, metadata)` triples.

- [ ] **Step 2:** Add audit assertions in existing scenarios:
  - M1 (setRole owner→admin happy): `fetch_audit_events_for_group` yields exactly 1 row with `action = "member.role_changed"`, `actor_user_id = m_owner_id`, `metadata.old_role = "member"`, `metadata.new_role = "admin"`.
  - M6 (setRole last-owner 409): `fetch_audit_events_for_group` yields 0 new rows — tx rolled back.
  - D1 (DELETE member happy): 1 row with `action = "member.removed"`, `metadata.target_user_id = d1_target_id`.
  - D4 (DELETE last owner 409): 0 new rows.
  - I1 (accept invite happy — adjust in `rest_v1_invites.rs`): 1 row with `action = "invite.accepted"`, `metadata.invited_email = invite_email`.

- [ ] **Step 3:** Add new `A1` scenario in `rest_v1_invites.rs` — rate-limit burst:
  ```rust
  // A1: accept invite rate-limit burst -> 429 on 21st request within 1 minute.
  //
  // Seeds 1 user, sends 20 bogus-token POST /v1/invites/bogus/accept requests
  // (all 404), then the 21st returns 429. The per-user-id key extractor
  // ensures the test is isolated from other scenarios running in the same
  // harness.
  ```

- [ ] **Step 4:** Run full test matrix — expect all PASS.

- [ ] **Step 5: Commit.**

---

## Task 9: Full validation pass + review prep

**Files:** none (validation only).

- [ ] **Step 1:** `cargo fmt --check --all`. Fix diffs if any.
- [ ] **Step 2:** `cargo clippy -p garraia-gateway -p garraia-auth --no-deps --features test-helpers --tests -- -D warnings`. Must not introduce new warnings in files touched by this plan.
- [ ] **Step 3:** Full test matrix:
  ```bash
  cargo test -p garraia-auth --lib
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

- [ ] **Step 4:** Commit any cleanups.

- [ ] **Step 5:** Run `@code-reviewer` + `@security-auditor` on the branch (parallel).

- [ ] **Step 6:** Address blockers/HIGH/MEDIUMs in review follow-up commits.

- [ ] **Step 7:** Open PR `feat(gateway,auth,workspace): audit events + rate-limit + single-owner idx amendment — plan 0021 (GAR-425)`.

---

## Acceptance criteria

1. Migration 012 applied; `pg_indexes.indexdef` for `group_members_single_owner_idx` contains `status = 'active'`.
2. 3 new variants in `WorkspaceAuditAction` with unit tests.
3. `accept_invite` writes exactly 1 `audit_events` row on happy path (`action = "invite.accepted"`).
4. `set_member_role` writes exactly 1 `audit_events` row after COUNT guard passes (`action = "member.role_changed"`).
5. `delete_member` writes exactly 1 `audit_events` row after COUNT guard passes (`action = "member.removed"`).
6. All failure paths (400/403/404/409) write 0 audit rows (tx rollback).
7. `RateLimitConfig::members_manage()` returns 20/min, 200/h, burst 5.
8. POST `/v1/invites/{token}/accept` returns 429 on the 21st request/min under A1 scenario.
9. D5 test cleanup simplified — no hard-delete needed.
10. CI 9/9 green.
11. `cargo fmt --check --all` clean.
12. `cargo clippy` zero new warnings in plan 0021 files.

## Rollback plan

All additive + forward-only:
- Migration 012 is a schema change — revert requires `CREATE UNIQUE INDEX ... WHERE role = 'owner'` migration OR a downgrade migration applied manually. **Risk: low** because the old predicate is a superset of the new one; rolling back via re-creation works without data migration.
- Audit writes + rate-limit middleware + preset are pure code additions; `git revert` of the squash commit restores previous behavior.

**Partial rollback:** if only rate-limit proves problematic post-merge (e.g. false-positive 429 in production), a follow-up commit can remove the three `.layer(...)` wires while keeping the audit events and migration.

## Open questions

- **OQ-1:** Should audit INSERT failures abort the tx (current design — treats audit as load-bearing) or be fire-and-forget (accept missing audit for ops that succeeded)? **Chosen: abort** (matches login flow), consistent with "no mutation without audit" policy. Can be revisited if contention becomes a problem.
- **OQ-2:** Should the rate-limit key fall back to IP when JWT is absent (e.g. 401 path)? **Yes**, but only after JWT preference is exhausted. IP fallback is the existing behavior of `rate_limiter.rs`.
- **OQ-3:** Should `actor_label` (email snapshot) be included in workspace audit rows? **Not in v1.** The `Principal` extractor currently does not carry email into handlers (only user_id). Adding requires a small extractor change; deferred to plan 0022+ unless review flags it as a blocker.
- **OQ-4:** Which `resource_type` string for setRole/delete? **`"group_members"`** (singular table, composite PK encoded as `"{group_id}:{user_id}"` in `resource_id`). Alternative was a custom `"members"` string; chose table name for consistency with the login flow pattern (`"user_identities"`).

## Relationship to other plans

- **Plan 0019** (accept invite) — SEC-01 + SEC-04 of this plan's review are its direct source.
- **Plan 0020** (setRole + DELETE) — SEC-MED (index + audit) of this plan's review are its direct source. D5 test restore simplification is a direct consequence of the index amendment.
- **Plan 0022+** — possible admin `/v1/audit/events` listing endpoint (needs new issue; GAR-87 was canceled); possible audit retention/purge (aligned with GAR-400 LGPD export/delete flow).

Com plan 0021 merged, as 3 dívidas de segurança dos reviews de PRs #25 e #27 são fechadas e o Group Workspace REST surface (Fase 3.4) fica production-ready antes de avançar para GAR-410 (CredentialVault final) ou GAR-379 (garraia-config crate).
