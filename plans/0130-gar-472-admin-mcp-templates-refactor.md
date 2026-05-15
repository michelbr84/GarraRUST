# Plan 0130 — GAR-472: Q9.d extract admin/mcp_templates.rs

## Goal

Extract the MCP template CRUD handlers and supporting types from
`crates/garraia-gateway/src/admin/handlers.rs` into a new focused module
`crates/garraia-gateway/src/admin/mcp_templates.rs`.

**Zero behavior change.** Routes, types, and pub symbols remain identical to callers.

## Architecture

```
admin/
  handlers.rs       — re-exports mcp_templates symbols via `pub use super::mcp_templates::*`
  mcp_templates.rs  — NEW: McpTemplate, builtin_templates (priv), load/save helpers (priv),
                           list_mcp_templates, save_mcp_template, delete_mcp_template
  mcp.rs            — already extracted (Q9.c / GAR-471)
  providers.rs      — already extracted (Q9.b / GAR-470)
  shared.rs         — already extracted (Q9.a / GAR-439)
  mod.rs            — adds `pub mod mcp_templates;`
```

## Tech stack

- Rust stable (1.92 MSRV), Axum 0.8, `serde_json`, `axum::extract::State`/`Path`/`Json`
- No new dependencies

## Design invariants

1. All public function signatures are identical to pre-extraction.
2. `routes.rs` references `handlers::list_mcp_templates`, `handlers::save_mcp_template`,
   `handlers::delete_mcp_template` — satisfied by re-exports in `handlers.rs`.
3. `McpTemplate` public struct moves to `mcp_templates.rs`; re-exported via `handlers.rs`.
4. Private helpers (`builtin_templates`, `user_templates_path`, `load_user_templates`,
   `save_user_templates`) remain private in the new module.

## Scope

**In:** lines 2309–2537 of `handlers.rs` (section `// ── MCP Templates (GAR-296 / GAR-297)`):
- `McpTemplate` struct
- `builtin_templates()` private fn
- `user_templates_path()` private fn
- `load_user_templates()` private fn
- `save_user_templates()` private fn
- `list_mcp_templates` pub async fn
- `save_mcp_template` pub async fn
- `delete_mcp_template` pub async fn

**Out of scope:** Glob config (stays in handlers.rs), MCP server CRUD (mcp.rs / Q9.c),
prompt/persona templates (`list_templates`, stays in handlers.rs), memory handlers,
channels, sessions, users, secrets, observability handlers.

## Validações pré-plano

- [x] `admin/mod.rs` already has `pub mod mcp;` as pattern to follow.
- [x] `admin/mcp.rs` has the re-export comment pattern to copy.
- [x] `routes.rs` references all 3 public functions via `handlers::*` — re-exports preserve these.
- [x] `handlers.rs` is 2550 LOC post-Q9.c; this slice brings it to ~2321 LOC (~229 LOC removed).
- [x] `McpTemplate` only referenced in `handlers.rs` (confirmed via grep).

## Rollback

Revert the single commit that creates `mcp_templates.rs` and modifies `handlers.rs` + `mod.rs`.

## Open questions

None — pattern established by Q9.a, Q9.b, Q9.c.

## File Structure

```
crates/garraia-gateway/src/admin/
  mcp_templates.rs  ← NEW (229 LOC)
  handlers.rs       ← MODIFIED (-229 lines + 7-line re-export block)
  mod.rs            ← MODIFIED (add 1 line)
```

## M1 tasks

- [x] T1: Create `admin/mcp_templates.rs` with the extracted code and proper module header
- [x] T2: Remove lines 2309–2537 from `handlers.rs` and add `pub use` re-exports
- [x] T3: Add `pub mod mcp_templates;` to `admin/mod.rs`
- [x] T4: `cargo check -p garraia-gateway` green
- [x] T5: `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean
- [x] T6: Commit `refactor(admin): Q9.d — extract admin/mcp_templates.rs (GAR-472)`
- [x] T7: Push + open PR + wait for CI green
- [x] T8: Merge + update ROADMAP + plans/README

## Risk register

| Risk | Mitigation |
|------|-----------|
| Re-export path break | Follow exact pattern of mcp.rs / providers.rs |
| `McpTemplate` visibility | pub struct, pub fields — unchanged |
| `routes.rs` breakage | All callers use `handlers::*` which still resolve via re-export |

## Acceptance criteria

- `cargo check -p garraia-gateway` passes.
- `cargo clippy ... -D warnings` passes.
- All CI checks green.
- `handlers.rs` is ≤ 2330 LOC post-merge.
- `admin/mcp_templates.rs` is the canonical home for `McpTemplate` + CRUD handlers.

## Cross-references

- Parent: GAR-472, Epic: GAR-430, GAR-439 (Q9 umbrella)
- Prior slices: plan 0127 (Q9.a), plan 0128 (Q9.b/GAR-470), plan 0129 (Q9.c/GAR-471)
- Next: GAR-473 (Q9.e: admin/observability.rs)

## Estimativa

- Implementação: ~30 min
- CI: ~15 min
- Total: ~45 min
