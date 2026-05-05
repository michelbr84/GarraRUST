# Plan 0059 — GAR-511: uploads_worker.rs SET LOCAL format! → set_config() + admin.html XSS cleanup

**Status:** 🟡 In Progress (health routine 2026-05-05)
**Linear:** [GAR-511](https://linear.app/chatgpt25/issue/GAR-511)
**Branch:** `health/202605050446-uploads-worker-set-config`
**Parent:** GAR-508 (closed, missed `uploads_worker.rs`)

---

## 1. Goal

Close the CodeQL `rust/sql-injection` taint path in `emit_expiration_audit`
(`crates/garraia-gateway/src/uploads_worker.rs:208-215`) that GAR-508's scope
missed, and apply defense-in-depth `escapeHtml()` to 3 remaining metric-card
`innerHTML` sinks in `admin.html` that GAR-510 did not reach (lines 760, 1221,
1338).

## 2. Architecture

No schema, no migration, no API change.

- `uploads_worker.rs::emit_expiration_audit` — replace 2 `sqlx::query(&format!("SET LOCAL ..."))` calls with `sqlx::query("SELECT set_config(..., $1, true)").bind(...)`.
- `admin.html` — add `escapeHtml()` wrapper around dynamic values injected via `innerHTML` in `pageDashboard` (line 760), `pageSessions` (line 1221), and `pageMetrics` (line 1338).

## 3. Tech stack

Rust / sqlx 0.8 / Axum 0.8, vanilla JS (admin.html).

## 4. Design invariants

- `set_config('app.current_X_id', $1, true)` is transaction-scoped — identical RLS semantics to `SET LOCAL` (same as GAR-508 §5.1).
- `escapeHtml()` helper is already defined in `admin.html:639` (GAR-510). No new dependency.
- No behavior change for metric card rendering — all current values are numeric strings; escapeHtml is a no-op over them. Defense-in-depth only.

## 5. Out of scope

- Introducing a `BYPASSRLS` worker role (future improvement noted in `uploads_worker.rs:21-24`).
- Other CodeQL rules.
- `deny.toml` / `audit.toml` changes.

## 6. Rollback

`git revert <merge-sha>` — zero schema / migration risk.

## 7. File structure

```
crates/garraia-gateway/src/uploads_worker.rs   ← T1: 2-line SQL fix
crates/garraia-gateway/src/admin.html          ← T2: 3-sink escapeHtml
plans/0059-gar-511-uploads-worker-set-config.md ← this file
plans/README.md                                ← update row
```

## 8. Tasks (M1)

- [x] T1 — `uploads_worker.rs:208-215`: replace both `format!("SET LOCAL …")` with `set_config()` bind-parameter queries
- [x] T2 — `admin.html` lines 760, 1221, 1338: wrap `innerHTML` dynamic values with `escapeHtml()`
- [ ] T3 — `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` → 0 warnings
- [ ] T4 — commit + push + open PR
- [ ] T5 — CI green; merge; mark GAR-511 Done; update plans/README.md

## 9. Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| `set_config` changes session config scope | Negligible | `is_local=true` means tx-scoped — same as `SET LOCAL` |
| escapeHtml breaks metric display | None | All current values are numeric; escapeHtml is identity on them |
| CI flaky (E2E/Playwright) | Low | No gateway logic changed |

## 10. Acceptance criteria

- `grep -n "SET LOCAL\|format!(.*current" crates/garraia-gateway/src/uploads_worker.rs` → 0 hits outside comments
- `grep -n "innerHTML.*item\.value\|innerHTML.*c\.value" crates/garraia-gateway/src/admin.html | grep -v escapeHtml` → 0 hits
- Clippy strict → 0 warnings
- CI ≥16 checks pass; PR squash-merged to main
- GAR-511 moved to Done in Linear

## 11. Estimativa

< 1h end-to-end (pure mechanical substitution, no test authoring needed — existing tests in `rest_v1_uploads_delete_worker.rs` cover the audit path).

## 12. Cross-references

- GAR-508 / plan 0056 — original `set_config()` wave (scope: `rest_v1/*.rs`)
- GAR-510 / plan 0057 — `escapeHtml` introduction + first 3 XSS sinks
- GAR-395 / plan 0047 — `uploads_worker.rs` original implementation (slice 3)
- CodeQL rule `rust/sql-injection`; rule `js/xss`
