# Plan 0131 — GAR-473: Q9.e extract admin/observability.rs

## Goal

Extract the observability/UI handlers (logs, metrics, Prometheus, alerts, themes,
layout preferences, prompt templates, about) from
`crates/garraia-gateway/src/admin/handlers.rs` into a new focused module
`crates/garraia-gateway/src/admin/observability.rs`.

**Zero behavior change.** Routes, types, and pub symbols remain identical to callers.

## Architecture

```
admin/
  handlers.rs         — re-exports observability symbols via `pub use super::observability::*`
  observability.rs    — NEW: admin_logs, admin_metrics, admin_prometheus, admin_alerts,
                              list_themes, get_layout_preferences, list_templates, about
  mcp_templates.rs    — already extracted (Q9.d / GAR-472)
  mcp.rs              — already extracted (Q9.c / GAR-471)
  providers.rs        — already extracted (Q9.b / GAR-470)
  shared.rs           — already extracted (Q9.a / GAR-439)
  mod.rs              — adds `pub mod observability;`
```

## Tech stack

- Rust stable (1.92 MSRV), Axum 0.8, `serde_json`, `dirs` crate
- No new dependencies

## Design invariants

1. All public function signatures are identical to pre-extraction.
2. `routes.rs` references `handlers::admin_logs`, `handlers::admin_metrics`,
   `handlers::admin_prometheus`, `handlers::admin_alerts`, `handlers::list_themes`,
   `handlers::get_layout_preferences`, `handlers::list_templates`, `handlers::about`
   — satisfied by re-exports in `handlers.rs`.
3. Private helpers (none in this section) stay private.
4. `memory_entry_to_json` stays in `handlers.rs` (used by Phase 5 Ops memory handlers).
5. Glob config handlers (`admin_glob_config`, `admin_glob_test`) stay in `handlers.rs`
   — out of scope for this slice.

## Scope

**In:** Phase 6 section of `handlers.rs` (lines 1985–2213, ~229 LOC):
- `// Phase 6: Observability/UI` section header
- `admin_logs` pub async fn
- `admin_metrics` pub async fn
- `admin_prometheus` pub async fn
- `admin_alerts` pub async fn
- `list_themes` pub async fn
- `get_layout_preferences` pub async fn
- `list_templates` pub async fn (prompt/persona templates — NOT MCP templates)
- `about` pub async fn

**Out of scope:** Glob config handlers (`admin_glob_config`, `admin_glob_test`),
memory handlers (Phase 5 Ops), users, secrets, auth handlers.

## Validações pré-plano

- [x] `admin/mod.rs` already has `pub mod mcp_templates;` as pattern to follow.
- [x] `admin/mcp_templates.rs` has the re-export comment pattern to copy.
- [x] `routes.rs` references all 8 public functions via `handlers::*` — re-exports preserve these.
- [x] `handlers.rs` is 2326 LOC post-Q9.d; this slice brings it to ~2107 LOC (~229 LOC removed
      + ~10 LOC re-export block added).
- [x] No struct types to re-export (all 8 items are functions only).

## Rollback

Revert the single commit that creates `observability.rs` and modifies `handlers.rs` + `mod.rs`.

## Open questions

None — pattern established by Q9.a through Q9.d.

## File Structure

```
crates/garraia-gateway/src/admin/
  observability.rs  ← NEW (~244 LOC)
  handlers.rs       ← MODIFIED (-229 lines + 9-line re-export block)
  mod.rs            ← MODIFIED (add 1 line)
```

## M1 tasks

- [ ] T1: Create `admin/observability.rs` with the extracted Phase 6 handlers
- [ ] T2: Remove Phase 6 lines 1985–2213 from `handlers.rs` and add `pub use` re-exports
- [ ] T3: Add `pub mod observability;` to `admin/mod.rs`
- [ ] T4: `cargo check -p garraia-gateway` green
- [ ] T5: `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean
- [ ] T6: Commit `refactor(admin): Q9.e — extract admin/observability.rs (GAR-473)`
- [ ] T7: Push + open PR + wait for CI green
- [ ] T8: Merge + update ROADMAP + plans/README

## Risk register

| Risk | Mitigation |
|------|-----------|
| Re-export path break | Follow exact pattern of mcp_templates.rs / mcp.rs / providers.rs |
| `list_templates` name clash with `list_mcp_templates` | Different functions — confirmed by grep |
| `dirs` crate already a dep | Confirmed in Cargo.toml of garraia-gateway |
| `crate::observability` module reference | Import via `crate::observability::global_metrics()` |

## Acceptance criteria

- `cargo check -p garraia-gateway` passes.
- `cargo clippy ... -D warnings` passes.
- All CI checks green.
- `handlers.rs` is ≤ 2120 LOC post-merge.
- `admin/observability.rs` is the canonical home for logs/metrics/prometheus/alerts/
  themes/layout/templates/about handlers.

## Cross-references

- Parent: GAR-473, Epic: GAR-430, GAR-439 (Q9 umbrella)
- Prior slices: plan 0128 (Q9.b/GAR-470), plan 0129 (Q9.c/GAR-471), plan 0130 (Q9.d/GAR-472)
- Next: GAR-474 (Q9.g: admin/users.rs) or GAR-475 (Q9.f: admin/secrets.rs)

## Estimativa

- Implementação: ~30 min
- CI: ~15 min
- Total: ~45 min
