# Plan 0129 — GAR-471: Q9.c extract admin/mcp.rs

## Goal

Extract the four MCP server CRUD handlers from `crates/garraia-gateway/src/admin/handlers.rs`
into a new focused module `crates/garraia-gateway/src/admin/mcp.rs`.

**Zero behavior change.** Routes, types, and pub symbols remain identical to callers.

## Architecture

```
admin/
  handlers.rs      — re-exports mcp symbols via `pub use super::mcp::*`
  mcp.rs           — NEW: admin_list_mcp, CreateMcpRequest, admin_create_mcp,
                           admin_restart_mcp, admin_delete_mcp
  mod.rs           — adds `pub mod mcp;`
  providers.rs     — already extracted (Q9.b / GAR-470)
  shared.rs        — already extracted (Q9.a / GAR-439)
```

## Tech stack

- Rust stable (1.92 MSRV), Axum 0.8, `serde_json`, `axum::extract::State`/`Path`/`Json`
- No new dependencies

## Design invariants

1. All public function signatures are identical to pre-extraction.
2. `routes.rs` references `handlers::admin_list_mcp` etc. — satisfied by re-exports.
3. `CreateMcpRequest` public struct moves to `mcp.rs`; re-exported via `handlers.rs`.
4. MCP Templates (`McpTemplate`, `builtin_templates`, `list_mcp_templates`, etc.) are **NOT**
   moved — those belong to Q9.d (GAR-472).

## Scope

**In:** lines 2203-2557 of handlers.rs (section `// ── MCP server management`):
- `admin_list_mcp`
- `CreateMcpRequest`
- `admin_create_mcp`
- `admin_restart_mcp`
- `admin_delete_mcp`

**Out of scope:** Glob config (stays in handlers.rs), MCP Templates (Q9.d / GAR-472),
channels, sessions, memory, tasks, users, secrets, observability handlers.

## Validações pré-plano

- [x] `admin/mod.rs` already has `pub mod providers;` as pattern to follow.
- [x] `admin/providers.rs` has the re-export comment pattern to copy.
- [x] `routes.rs` references all 4 functions via `handlers::*` — re-exports preserve these.
- [x] `handlers.rs` is 2900 LOC post-Q9.b; this slice brings it to ~2555 LOC (~345 LOC removed).

## Rollback

Revert the single commit that creates `mcp.rs` and modifies `handlers.rs` + `mod.rs`.

## Open questions

None — pattern is established by Q9.a and Q9.b.

## File structure

```
crates/garraia-gateway/src/admin/
  mcp.rs     ← NEW (≈355 LOC)
  handlers.rs ← remove 355 LOC, add 5-line re-export block
  mod.rs      ← add `pub mod mcp;`
```

## M1 tasks

- [x] T1: Write plan file (this document)
- [ ] T2: Create `admin/mcp.rs` with extracted functions
- [ ] T3: Replace extracted block in `handlers.rs` with re-exports + remove section
- [ ] T4: Add `pub mod mcp;` to `mod.rs`
- [ ] T5: `cargo check -p garraia-gateway && cargo clippy -p garraia-gateway --no-deps -- -D warnings`
- [ ] T6: Commit, push, open PR
- [ ] T7: Wait for CI green, squash-merge
- [ ] T8: Mark GAR-471 Done in Linear + update plans/README.md

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|-----------|
| Import path resolution error | Low | Follow providers.rs pattern exactly |
| `CreateMcpRequest` used outside handlers | Low | Check with grep before cutting |

## Acceptance criteria

- `cargo check --workspace --exclude garraia-desktop` green.
- `cargo clippy --workspace --tests --exclude garraia-desktop --no-deps -- -D warnings` green.
- `handlers.rs` is ≤ 2560 LOC.
- `admin/mcp.rs` exists with all 4 handlers + `CreateMcpRequest`.
- CI 100% green (all ≥16 workflow checks pass).

## Cross-references

- Parent epic: GAR-430 (Quality Gates Phase 3.6)
- Parent issue: GAR-439 (Q9 refactor admin/handlers.rs)
- Predecessor: plan 0128 / GAR-470 (Q9.b — providers.rs)
- Successor: plan 0130 / GAR-472 (Q9.d — mcp_templates.rs)

## Estimativa

0.5 / 1 / 1.5 hours (pure mechanical extraction, pattern well-established).
