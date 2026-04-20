# Plan 0022 — GAR-426: Workspace security hardening part 2 (rate-limit refinements + audit robustness)

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Linear issue:** [GAR-426](https://linear.app/chatgpt25/issue/GAR-426) — "Workspace security hardening part 2: per-user rate-limit key + trusted proxies + rate-limiter refinements" (In Progress, High, labels `security` + `epic:ws-api`, project Fase 3 — Group Workspace).

**Status:** Draft — pendente aprovação.

**Relationship note:** este plan fecha explicitamente os 9 follow-ups deferidos dos reviews do PR #30 (plan 0021 / GAR-425). Sem ele, as 3 rotas privilegiadas (`accept`, `setRole`, `DELETE member`) não são seguras para produção com carga multi-usuário concorrente.

**Goal:** hardening cirúrgico do `rate_limiter.rs` + robustez adicional em `audit_workspace`, sem novos pilares arquiteturais:

1. Rate-limit key **per-user** via JWT `sub` decoding (fix do shared-bucket F-03).
2. `GARRAIA_TRUSTED_PROXIES` env + stripping de X-Forwarded-For forjado (fix completo F-02).
3. Refactor do `DashMap::entry` lock retention (CR-HIGH).
4. 4 NIT fixes em `rate_limiter.rs` (reset_at, cast, headers duplicados, leak timing Reset header).
5. Assertion test F-05 para o branch 2 da policy `audit_events_group_or_self`.
6. `WorkspaceAuditAction::Display` impl.
7. A7 burst test deterministic (21º request = 429 exact).

## Architecture

### 1. `RateLimitLayerState` + `rate_limit_layer_authenticated`

Novo state struct compondo `limiter` + `jwt_issuer`:

```rust
#[derive(Clone)]
pub struct RateLimitLayerState {
    pub limiter: Arc<RateLimiter>,
    pub jwt_issuer: Arc<JwtIssuer>,
}

impl FromRef<RestV1FullState> for RateLimitLayerState { /* ... */ }
```

Middleware usa `JwtIssuer::verify_access` e keya por `jwt-sub:{uuid}`. Sem fallback token-prefix.

### 2. TRUSTED_PROXIES helper

```rust
// Em rate_limiter.rs (ou trusted_proxy.rs se ficar maior)
pub fn parse_trusted_proxies(env: &str) -> Vec<IpCidr> { /* ... */ }

pub fn real_client_ip(
    headers: &HeaderMap,
    peer_addr: IpAddr,
    trusted: &[IpCidr],
) -> IpAddr {
    if trusted.is_empty() {
        return peer_addr; // fail-closed: ignora XFF completamente
    }
    if !trusted.iter().any(|c| c.contains(&peer_addr)) {
        return peer_addr; // peer não é um proxy confiável
    }
    // peer é um proxy confiável — tenta XFF
    headers.get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.trim().parse::<IpAddr>().ok())
        .unwrap_or(peer_addr)
}
```

### 3. DashMap lock split

```rust
pub fn check(&self, key: &str) -> RateLimitDecision {
    let now = now_secs();

    // Fase 1: read-only lookup
    let (per_minute, per_hour) = self.windows
        .get(key)
        .map(|entry| {
            let mut state = entry.clone();
            state.prune(3600);
            (state.count_in_window(60), state.count_in_window(3600))
        })
        .unwrap_or((0, 0));

    let allowed = per_minute < self.config.requests_per_minute
        && per_hour < self.config.requests_per_hour;

    // Fase 2: breve write se allowed
    if allowed {
        let mut entry = self.windows
            .entry(key.to_string())
            .or_insert_with(WindowState::new);
        entry.prune(3600);
        entry.record();
    }

    let remaining = self.config.requests_per_minute.saturating_sub(
        u32::try_from(per_minute.saturating_add(1)).unwrap_or(u32::MAX)
    );
    let reset_at = now + 60 - (now % 60);
    RateLimitDecision { allowed, limit: self.config.requests_per_minute, remaining, reset_at }
}
```

## Design invariants

1. **Backward-compat HTTP:** zero mudança observável por clientes legítimos, exceto remoção de `X-RateLimit-Reset` (IETF OPTIONAL; `Retry-After` é o canônico).
2. **Middleware legacy preserved:** `rate_limit_layer` continua disponível para rotas não-autenticadas.
3. **Rate-limit presets inalterados:** `auth()`, `read_only()`, `members_manage()`, `default()`.
4. **Fail-closed TRUSTED_PROXIES:** env vazio → XFF ignorado. Dev setups setam `GARRAIA_TRUSTED_PROXIES=127.0.0.1`.
5. **Audit semantics inalteradas:** 1 INSERT por happy-path + 0 em failure paths.
6. **Display impl não-runtime:** só formatação; não toca INSERT.
7. **Double-verify trade-off aceito (v1):** middleware + extractor ambos verificam HMAC. ~µs cost aceitável; optimization via `request.extensions()` fica para plan 0023+ se p95 degradar >5%.

## File structure

| File | Action | Responsibility |
|------|--------|---------------|
| `crates/garraia-auth/src/audit_workspace.rs` | Modify | Add `impl fmt::Display for WorkspaceAuditAction` + unit test |
| `crates/garraia-gateway/src/rate_limiter.rs` | Modify | T2-T5: TRUSTED_PROXIES helper, DashMap split, NIT fixes, remove `X-RateLimit-Reset`, add `RateLimitLayerState` + `rate_limit_layer_authenticated` |
| `crates/garraia-gateway/src/rest_v1/mod.rs` | Modify | Swap the 3 privileged routes from `rate_limit_layer` to `rate_limit_layer_authenticated` |
| `crates/garraia-gateway/tests/rest_v1_invites.rs` | Modify | A7 upgrade to deterministic (21st = 429 exact) |
| `crates/garraia-gateway/tests/audit_workspace_branch_two.rs` | Create | Integration test for F-05 (policy branch 2 fail without `app.current_user_id`) |
| `CLAUDE.md` | Modify | Document `GARRAIA_TRUSTED_PROXIES` env var + fail-closed default |
| `plans/README.md` | Modify | 0022 Draft → Merged lifecycle |

**No migrations. No schema changes.**

---

## Task 1: `WorkspaceAuditAction::Display` impl

**Files:** `crates/garraia-auth/src/audit_workspace.rs`

- [ ] **Step 1:** Write failing test.
  ```rust
  #[test]
  fn workspace_audit_action_display_delegates_to_as_str() {
      use std::fmt::Write;
      let mut buf = String::new();
      write!(&mut buf, "{}", WorkspaceAuditAction::InviteAccepted).unwrap();
      assert_eq!(buf, "invite.accepted");

      let formatted = format!("{} {}",
          WorkspaceAuditAction::MemberRoleChanged,
          WorkspaceAuditAction::MemberRemoved
      );
      assert_eq!(formatted, "member.role_changed member.removed");
  }
  ```

- [ ] **Step 2:** Add `impl fmt::Display`.
  ```rust
  impl std::fmt::Display for WorkspaceAuditAction {
      fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
          f.write_str(self.as_str())
      }
  }
  ```

- [ ] **Step 3:** Run `cargo test -p garraia-auth --lib audit_workspace` — expect PASS.

- [ ] **Step 4:** Commit.

---

## Task 2: `GARRAIA_TRUSTED_PROXIES` env + `real_client_ip` helper

**Files:** `crates/garraia-gateway/src/rate_limiter.rs`

- [ ] **Step 1:** Add `ipnet` crate dependency if not present (check `Cargo.toml`).

- [ ] **Step 2:** Write failing unit tests for `parse_trusted_proxies` + `real_client_ip`:
  ```rust
  #[test]
  fn trusted_proxies_empty_env_ignores_xff() { /* ... */ }
  #[test]
  fn trusted_proxies_peer_in_allowlist_accepts_xff() { /* ... */ }
  #[test]
  fn trusted_proxies_peer_outside_allowlist_ignores_xff() { /* ... */ }
  #[test]
  fn trusted_proxies_cidr_match() { /* ... */ }
  ```

- [ ] **Step 3:** Implement `parse_trusted_proxies` + `real_client_ip`.

- [ ] **Step 4:** Update `extract_rate_limit_key` fallback branch to use `real_client_ip` (requires peer_addr as parameter — may mean changing the function signature; adjust the middleware caller).

- [ ] **Step 5:** Run tests, cargo check, commit.

---

## Task 3: `RateLimitLayerState` + `rate_limit_layer_authenticated`

**Files:** `crates/garraia-gateway/src/rate_limiter.rs`, `crates/garraia-gateway/src/rest_v1/mod.rs`

- [ ] **Step 1:** Add `RateLimitLayerState` struct + `FromRef<RestV1FullState>` impl.

- [ ] **Step 2:** Implement `rate_limit_layer_authenticated` middleware:
  ```rust
  pub async fn rate_limit_layer_authenticated(
      State(state): State<RateLimitLayerState>,
      headers: HeaderMap,
      req: Request,
      next: Next,
  ) -> Response {
      // Extract bearer, verify, key by sub UUID.
      let key = match extract_bearer_and_verify(&headers, &state.jwt_issuer) {
          Some(claims) => format!("jwt-sub:{}", claims.sub),
          None => {
              // No JWT or invalid → fall through to unauthenticated bucket
              // (the Principal extractor downstream will 401 if required).
              extract_rate_limit_key(&headers, peer_addr_from_req(&req), &state.trusted_proxies)
          }
      };
      let decision = state.limiter.check(&key);
      // ... same as rate_limit_middleware ...
  }
  ```

- [ ] **Step 3:** Wire into `rest_v1/mod.rs` mode 1: swap the 3 privileged routes from `rate_limit_layer` to `rate_limit_layer_authenticated`.

- [ ] **Step 4:** `cargo check`, `cargo test` integration tests (rest_v1_invites should still pass A1-A6; A7 updated in T6).

- [ ] **Step 5:** Commit.

---

## Task 4: DashMap lock refactor in `RateLimiter::check()`

**Files:** `crates/garraia-gateway/src/rate_limiter.rs`

- [ ] **Step 1:** Run existing `blocks_requests_over_limit` + `allows_requests_under_limit` + `independent_keys` tests — record baseline pass.

- [ ] **Step 2:** Refactor `check()` to split read/write phases per §Architecture.

- [ ] **Step 3:** Re-run tests. Tolerate race-benign overshoot (the test harness is single-threaded per test, so behavior is deterministic).

- [ ] **Step 4:** Commit.

---

## Task 5: NIT fixes (reset_at, cast, duplicated headers, remove `X-RateLimit-Reset`)

**Files:** `crates/garraia-gateway/src/rate_limiter.rs`

- [ ] **Step 1:** `reset_at` — capture `now_secs()` once.
- [ ] **Step 2:** `count_in_window` — `u32::try_from` instead of `as u32`.
- [ ] **Step 3:** `rate_limit_response` — remove duplicated header application.
- [ ] **Step 4:** `apply_rate_limit_headers` — remove `x-ratelimit-reset`. Keep `Retry-After` in 429 responses.
- [ ] **Step 5:** Update existing tests that assert `X-RateLimit-Reset` presence (if any) — remove the assertion.
- [ ] **Step 6:** Commit.

---

## Task 6: A7 burst deterministic upgrade

**Files:** `crates/garraia-gateway/tests/rest_v1_invites.rs`

- [ ] **Step 1:** Update A7 to:
  - Expect the first 20 requests to return 404 (bogus token, handler runs).
  - Expect the 21st request (exact) to return 429.
  - Assert `Retry-After` + `X-RateLimit-Limit` + `X-RateLimit-Remaining` present.
  - Assert `X-RateLimit-Reset` is **NOT** present (T5 change).
  - Update the docstring + "Known limitations" block to reflect per-user bucket.

- [ ] **Step 2:** Run test, commit.

---

## Task 7: F-05 integration test — policy branch 2 without `app.current_user_id`

**Files:** `crates/garraia-gateway/tests/audit_workspace_branch_two.rs` (NEW)

- [ ] **Step 1:** Create the test. It must:
  - Use harness `admin_pool` wrapped as app_pool context (or manually open a tx on the app_pool).
  - Open tx on `app_pool`, skip `SET LOCAL app.current_user_id`.
  - Call `audit_workspace_event` with `group_id=NULL` emulator (branch 2) — note: current `audit_workspace_event` takes `group_id: Uuid` (not Option), so this test may need a dedicated SQL helper that bypasses the helper and does the raw INSERT with `group_id=NULL` + `actor_user_id=<uuid>` to specifically exercise the branch 2 of the RLS policy.
  - Assert the INSERT fails with SQLSTATE 42501 (RLS rejection).

- [ ] **Step 2:** If the test requires exposing a raw SQL path (bypassing the helper), document in the test file why — it's a regression guard for the policy's branch 2, not an API contract.

- [ ] **Step 3:** Commit.

---

## Task 8: Full validation + review prep

**Files:** none (validation).

- [ ] **Step 1:** `cargo fmt --check --all`.
- [ ] **Step 2:** `cargo clippy -p garraia-gateway -p garraia-auth --no-deps --features test-helpers --tests -- -D warnings`.
- [ ] **Step 3:** Full test matrix:
  ```
  cargo test -p garraia-auth --lib
  cargo test -p garraia-gateway --lib
  cargo test -p garraia-gateway --features test-helpers --test rest_v1_groups
  cargo test -p garraia-gateway --features test-helpers --test rest_v1_invites
  cargo test -p garraia-gateway --features test-helpers --test authz_http_matrix
  cargo test -p garraia-gateway --features test-helpers --test rest_v1_me_authed
  cargo test -p garraia-gateway --features test-helpers --test rest_v1_me
  cargo test -p garraia-gateway --features test-helpers --test harness_smoke
  cargo test -p garraia-gateway --features test-helpers --test router_smoke_test
  cargo test -p garraia-gateway --features test-helpers --test audit_workspace_branch_two
  ```

- [ ] **Step 4:** Run `@code-reviewer` + `@security-auditor` in parallel.

- [ ] **Step 5:** Address blockers / HIGH in a follow-up commit (pattern from 0020/0021).

- [ ] **Step 6:** Open PR with detailed body covering acceptance criteria.

---

## Acceptance criteria

1. CI 9/9 green on the merge commit.
2. A7 test returns 429 **exactly** on the 21st request (not "eventually").
3. `rate_limit_layer_authenticated` middleware keys by verified JWT `sub` UUID.
4. `GARRAIA_TRUSTED_PROXIES` documented in CLAUDE.md + 4 unit tests (empty, matched, unmatched, CIDR).
5. `X-RateLimit-Reset` absent from all response headers.
6. `WorkspaceAuditAction::Display` + F-05 integration test pass.
7. DashMap `check()` split-read-write.
8. 4 NIT fixes applied (reset_at, cast, headers, Reset header).
9. Code review APPROVE + security audit ≥ 8.5/10 without blockers.

## Rollback

All additive + forward-compatible. Revert of the squash commit restores plan 0021 behavior. No migrations. No schema changes.

## Open questions

- **OQ-1:** Does `real_client_ip` need to live in a dedicated `trusted_proxy.rs` module, or stay inline in `rate_limiter.rs`? **Decision: inline for v1** — if 0023+ consolidates XFF handling in `api.rs` + `admin/middleware.rs`, the helper moves to a shared module then.
- **OQ-2:** Should `rate_limit_layer_authenticated` fall back to IP-keying when bearer is absent, or return 401? **Decision: fall back to IP via `real_client_ip`** — the Principal extractor downstream enforces the 401 if the route requires auth. This preserves the defense-in-depth semantics (unauthenticated probes still get rate-limited, not just 401-ed).
- **OQ-3:** Should the F-05 test bypass `audit_workspace_event` helper or exercise it? **Decision: bypass** — the helper's contract (documented) requires `SET LOCAL app.current_user_id` to be already set; testing the policy branch-2 requires crafting a raw INSERT that violates the contract. The test is a regression guard, not a helper contract test.

## Relationship to other plans

- **Plan 0019** — originated SEC-01 (rate-limit) + SEC-04 (audit accept). Endereçados em 0021.
- **Plan 0020** — originated SEC-MEDs (index + audit setRole/DELETE). Endereçados em 0021.
- **Plan 0021** — registered 9 follow-ups deferred to 0022. **Plan 0022 closes them all.**
- **Plan 0023+** — possible consolidação de XFF handling em `api.rs` e `admin/middleware.rs`; potenciais NITs pendentes de 0020 (`assert_group_header_matches` + `resolve_caller_role` helpers, `RETURNING 1 → true`); opcional cache de decoded claims via `request.extensions()` se p95 degradar >5%.

With plan 0022 merged, the 3 privileged Group Workspace REST endpoints become production-ready for multi-user concurrent load.
