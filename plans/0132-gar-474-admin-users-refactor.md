# Plan 0132 — GAR-474: Q9.g extract admin/users.rs

## Goal

Extract the setup, user-management, and danger-zone handlers from
`crates/garraia-gateway/src/admin/handlers.rs` into a new focused module
`crates/garraia-gateway/src/admin/users.rs`.

**Zero behavior change.** Routes, types, and pub symbols remain identical to callers.

## Architecture

```
admin/
  handlers.rs       — re-exports users symbols via `pub use super::users::*`
  users.rs          — NEW: SetupRequest, setup, setup_status,
                           CreateUserRequest, create_user, list_users,
                           UpdateUserRoleRequest, update_user_role, delete_user,
                           DangerZoneRequest, danger_zone
  observability.rs  — already extracted (Q9.e / GAR-473)
  mcp_templates.rs  — already extracted (Q9.d / GAR-472)
  mcp.rs            — already extracted (Q9.c / GAR-471)
  providers.rs      — already extracted (Q9.b / GAR-470)
  shared.rs         — already extracted (Q9.a / GAR-439)
  mod.rs            — adds `pub mod users;`
```

## Tech stack

- Rust stable (1.92 MSRV), Axum 0.8, `serde_json`, `axum::extract::State`/`Path`/`Json`/`Extension`
- No new dependencies

## Design invariants

1. All public function signatures are identical to pre-extraction.
2. `routes.rs` references `handlers::setup`, `handlers::setup_status`, `handlers::create_user`,
   `handlers::list_users`, `handlers::update_user_role`, `handlers::delete_user`,
   `handlers::danger_zone` — satisfied by re-exports in `handlers.rs`.
3. Private helpers (none in this section) stay private.
4. `Role` import remains in `handlers.rs` (used by secrets/config/audit sections).

## Scope

**In:** lines 174–543 of `handlers.rs` (setup + user management + danger zone):
- `SetupRequest` struct + `setup` + `setup_status` pub async fns
- `CreateUserRequest` struct + `create_user` pub async fn
- `list_users` pub async fn
- `UpdateUserRoleRequest` struct + `update_user_role` pub async fn
- `delete_user` pub async fn
- `DangerZoneRequest` struct + `danger_zone` pub async fn

**Out of scope:** Auth endpoints (`login`, `logout`, `me`), audit log, secrets, config,
phases 4+5 ops, glob config.

## Validações pré-plano

- [x] `admin/mod.rs` already has `pub mod observability;` as pattern to follow.
- [x] `routes.rs` references all 7 public functions via `handlers::*` — re-exports preserve these.
- [x] `handlers.rs` is 2102 LOC post-Q9.e; this slice brings it to ~1738 LOC (~364 LOC removed).
- [x] No struct types referenced outside `handlers.rs` (confirmed via grep).

## Rollback

Revert the single commit that creates `users.rs` and modifies `handlers.rs` + `mod.rs`.

## Open questions

None — pattern established by Q9.a through Q9.e.

## File Structure

```
crates/garraia-gateway/src/admin/
  users.rs      ← NEW (382 LOC)
  handlers.rs   ← MODIFIED (-370 lines + 8-line re-export block)
  mod.rs        ← MODIFIED (add 1 line)
```

## M1 tasks

- [x] T1: Create `admin/users.rs` with the extracted code and proper module header
- [x] T2: Remove lines 174–543 from `handlers.rs` and add `pub use` re-exports
- [x] T3: Add `pub mod users;` to `admin/mod.rs`
- [x] T4: `cargo check -p garraia-gateway` green
- [x] T5: `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean
- [x] T6: Commit `refactor(admin): Q9.g — extract admin/users.rs (GAR-474)`
- [ ] T7: Push + open PR + wait for CI green
- [ ] T8: Merge + update ROADMAP + plans/README

## Risk register

| Risk | Mitigation |
|------|-----------|
| Re-export path break | Follow exact pattern of prior Q9 slices |
| `Role` import shadowing | handlers.rs still imports Role for other sections — no conflict |
| `build_session_cookie` in users.rs | Import only what's needed from middleware |

## Acceptance criteria

- `cargo check -p garraia-gateway` passes.
- `cargo clippy ... -D warnings` passes.
- All CI checks green.
- `handlers.rs` is ≤ 1750 LOC post-merge.
- `admin/users.rs` is the canonical home for setup/users/danger-zone handlers.

## Cross-references

- Parent: GAR-474, Epic: GAR-430, GAR-439 (Q9 umbrella)
- Prior slices: plan 0131 (Q9.e/GAR-473), plan 0130 (Q9.d/GAR-472), plan 0129 (Q9.c/GAR-471)
- Next: GAR-475 (Q9.f: admin/secrets.rs — HIGH risk, security-audit required)

## Estimativa

- Implementação: ~30 min
- CI: ~15 min
- Total: ~45 min
