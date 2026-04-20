# Plan 0023 — GAR-427: Migrate api.rs session-IP stamp to real_client_ip

> **Small slice:** narrow scope, 1 code site, hybrid PR (plan + README + code in one PR). 3 tasks. Delivered as a single feat/* branch cycle.

**Linear issue:** [GAR-427](https://linear.app/chatgpt25/issue/GAR-427) — "Workspace security hardening part 3: migrate api.rs session-IP stamp to real_client_ip" (In Progress, Medium, labels `security` + `epic:ws-api`, project Fase 3 — Group Workspace).

**Relationship note:** closes 1 of 3 `TODO(plan-0023+)` markers left in the code by plan 0022 (GAR-426). The other 2 (`admin/middleware.rs:168` + 20+ `admin/handlers.rs` call sites) are **explicitly out-of-scope** for this plan and deferred to a dedicated `admin-XFF` plan — they require `connect_info` propagation in 20+ call sites plus a product decision about the admin audit-trail IP format change.

**Goal:** `api.rs::create_session` stops trusting `X-Forwarded-For` blindly and derives the session-token IP via the plan-0022 `real_client_ip` helper with `GARRAIA_TRUSTED_PROXIES` fail-closed default.

## Architecture

### Before (current state)

```rust
// api.rs:70-78 (current TODO(plan-0023+) site)
let ip = headers
    .get("x-forwarded-for")
    .and_then(|v| v.to_str().ok())
    .map(|s| s.split(',').next().unwrap_or(s).trim().to_string());
```

Trusts any XFF header. Spoofable.

### After

```rust
// New site: uses real_client_ip with TRUSTED_PROXIES validation.
let trusted = std::env::var(rate_limiter::TRUSTED_PROXIES_ENV)
    .ok()
    .map(|v| rate_limiter::parse_trusted_proxies(&v))
    .unwrap_or_default();
let ip = peer_info
    .map(|ConnectInfo(sa)| sa.ip())
    .map(|peer| rate_limiter::real_client_ip(&headers, peer, &trusted).to_string());
```

Fail-closed: empty env → XFF ignored, peer_addr wins.

### Handler signature change

```rust
pub async fn create_session(
    State(state): State<SharedState>,
    peer_info: Option<ConnectInfo<SocketAddr>>,   // NEW
    headers: HeaderMap,
    Json(body): Json<CreateSessionRequest>,
) -> impl IntoResponse
```

`Option<ConnectInfo>` because:
- Production Axum wires `ConnectInfo` via the real TCP listener (always present).
- Tests via `oneshot()` may or may not inject it.
- When absent, `ip` becomes `None` — same behavior as "no XFF header" pre-0022.

## Design invariants

1. **Backward-compat HTTP** — zero observable change to clients. `ip` parameter goes into `manager.create_token` which stores it for audit; format unchanged (String).
2. **Fail-closed** — env unset → XFF ignored → peer_addr used. Matches plan 0022 convention.
3. **Per-request parse** — TRUSTED_PROXIES re-parsed on each session creation. Acceptable for this low-traffic endpoint (session creation is not hot-path). Cache optimization deferred.
4. **Zero admin-surface change** — this plan does NOT touch `admin/middleware.rs::extract_ip` or any `admin/handlers.rs` site. Admin audit-trail IP format is unchanged by this plan.

## Tech Stack

- Axum 0.8 `ConnectInfo<SocketAddr>` extractor.
- `rate_limiter::{TRUSTED_PROXIES_ENV, parse_trusted_proxies, real_client_ip}` (pub, from plan 0022).
- No new deps.

## File structure

| File | Action | Responsibility |
|------|--------|---------------|
| `crates/garraia-gateway/src/api.rs` | Modify | `create_session` handler signature + XFF → real_client_ip |
| `plans/0023-gar-427-xff-api-session-ip.md` | Create | This plan |
| `plans/README.md` | Modify | Index entry 0023 |

**No migrations, no schema changes, no new deps, no CLAUDE.md changes.**

## Task 1 — Handler migration

**Files:** `crates/garraia-gateway/src/api.rs`

- [ ] Add `std::net::SocketAddr` + `axum::extract::ConnectInfo` to imports.
- [ ] Import `crate::rate_limiter::{TRUSTED_PROXIES_ENV, parse_trusted_proxies, real_client_ip}`.
- [ ] Add `peer_info: Option<ConnectInfo<SocketAddr>>` to `create_session` signature.
- [ ] Replace the XFF parse block with the `real_client_ip`-based block.
- [ ] Update the outdated `TODO(plan-0023+)` comment → reference plan 0023 fix.
- [ ] `cargo check -p garraia-gateway` OK.

## Task 2 — Validation

- [ ] `cargo fmt --check --all` clean.
- [ ] `cargo clippy -p garraia-gateway --no-deps --features test-helpers --tests -- -D warnings` — zero new warnings in `api.rs`.
- [ ] Full test matrix (9 binaries) — existing integration tests that exercise session creation must pass unchanged.

## Task 3 — Reviews + PR + CI + merge

- [ ] Run `@code-reviewer` + `@security-auditor` in parallel.
- [ ] Address blockers (if any).
- [ ] Hybrid PR — `gh pr create` with plan + README + code in the same PR. Body covers: scope, behavior change, rollback, follow-ups.
- [ ] Monitor CI 9/9.
- [ ] Squash merge + delete branch.
- [ ] Sync main + cleanup worktree.
- [ ] Linear GAR-427 final comment + Done.

## Acceptance criteria

- `create_session` derives `ip` via `real_client_ip` with TRUSTED_PROXIES fail-closed.
- `TODO(plan-0023+)` comment replaced with "fixed in plan 0023".
- Full test matrix green.
- CI 9/9 green.
- Code review APPROVE + security audit no blockers.

## Rollback

Revert of squash commit restores pre-0023 behavior. Zero schema change.

## Follow-ups (explicit)

- **Plan 0024+:** admin XFF consolidation (`admin/middleware.rs::extract_ip` + 20+ `admin/handlers.rs` sites). Requires `connect_info` propagation + product decision about admin audit-trail IP behavior.
- **Plan 0025+:** `/auth/*` routes migration from deprecated `rate_limit_layer` to `rate_limit_layer_authenticated`. Blocked by design (register/login MINT the token).
- **Plan optimization (opportunistic):** cache `GARRAIA_TRUSTED_PROXIES` parse in SharedState if profiling flags session-creation hot-path cost.
