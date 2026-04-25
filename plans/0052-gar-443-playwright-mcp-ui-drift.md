# Plan 0052 — GAR-443 Lote 4: Playwright MCP UI drift + remove CoE

> **Linear:** [GAR-443](https://linear.app/chatgpt25/issue/GAR-443) (Q17, parent [GAR-430](https://linear.app/chatgpt25/issue/GAR-430) Quality Gates Phase 3.6)
> **Session plan:** `C:/Users/miche/.claude/plans/contexto-de-retomada-sparkling-torvalds.md`
> **Branch:** `fix/quality-gates-lote-4-playwright-mcp-ui-drift` (worktree `.claude/worktrees/playwright-mcp-ui-drift`)
> **Base:** `main @ 49d52c8`
> **Date:** 2026-04-24 (Florida local)
> **Approved:** 2026-04-24 with explicit scope expansion to use `data-testid` (see §2)

## 1. Goal

Make the `playwright` CI job a **genuine blocking gate**. Remove the `continue-on-error: true` from `.github/workflows/ci.yml` (Plan 0050 Lote 2 marker, GAR-NEW-Q17) so all 9 Playwright tests in `tests/playwright/mcp-manager.spec.ts` must pass for `main` to stay green. CoE count drops **2 → 1** (only `cargo audit` / RUSTSEC remains).

## 2. Why `data-testid` (approved scope expansion)

The original GAR-443 description says: *"Não reescrever os testes para uma abordagem diferente (ex.: data-testid) — se for útil, vira GAR follow-up separado."* The owner approved an **explicit scope expansion** during plan review (2026-04-24 ET) — registered as a comment on GAR-443. Reasons:

1. Read-only diagnosis revealed **two simultaneous drifts** in the same `showMcpForm()` call site, not just one:
   - `input[placeholder*="Command"]` does not match the placeholder `e.g. npx -y my-mcp-server` (`admin.html:1134`).
   - `getByRole('button', { name: 'Add Server' })` does not match the button text `Add` (`admin.html:1152`).
   Plus 2 secondary mismatches discovered during the rewrite: button text `Save` vs. spec's `Save Changes`, and `<h3>` text `Edit: <name>` vs. spec's `Edit MCP Server`.
2. Trading 6 fragile `placeholder*=` selectors for 6 different fragile `placeholder*=` selectors only delays the next drift.
3. `data-*` is already an established convention in `admin.html` (`data-page="..."` at l. 464-533, l. 1094).
4. Killing `.locator('..').locator('..')` in tests 4/5/6 costs **+1 line** at the table-row generator (`admin.html:1107`).
5. Patch UI stays minimal — ~30 lines edited inside the already-localized `showMcpForm` + `renderMcpPage` block.

## 3. Scope

### Admin UI (`crates/garraia-gateway/src/admin.html`)

- `showMcpForm` (l. 1127): add `data-testid` on form root, h3 title, error banner, name/transport/command/args/url/token/timeout/submit/cancel + new `mcp-form-save-template` (Add mode only).
- `mcpStatusBadge` (l. 1079): add `data-testid="mcp-status-badge"` to both branches.
- `renderMcpPage` (l. 1105-1107): add `data-testid` on `<tr>` (`mcp-row-${name}`) plus the Edit/Restart/Delete row buttons.
- `showMcpForm` submit handler:
  - Empty-name path now sets `errorEl` text (`'Name is required.'`) and toggles `display:block` instead of returning silently.
  - Toast text becomes contextual: `'Added'` (create) / `'Updated'` (edit) / `'Template saved'` (template) — satisfies the specs' regex-based assertions.
- `showMcpForm` h3 in edit mode reads `'Edit MCP Server: <name>'` (was `'Edit: <name>'`) — keeps the server name visible while satisfying the spec's `toContainText('Edit MCP Server')`.
- Submit button text in edit mode reads `'Save Changes'` (was `'Save'`).
- New "Save as Template" button (Add mode only) wires to the existing `POST /api/mcp/templates` endpoint (`admin/handlers.rs:3216 save_mcp_template`) — no backend changes.

### Playwright spec (`tests/playwright/mcp-manager.spec.ts`)

- Migrate every form/row interaction selector to `getByTestId(...)`.
- Replace `.locator('..').locator('..')` parent climbs with the new `rowFor(page, name)` helper (`page.getByTestId('mcp-row-' + name)`).
- Test 1, login helper, `[data-page="mcp"]` / `[data-page="dashboard"]` navigation, and toast assertions kept as-is — they already use stable IDs/classes.
- Test 6's "row removed" check changes from `not.toContainText` to `toHaveCount(0)` — clearer intent, no false positive on text fragments.

### CI (`.github/workflows/ci.yml`)

- Drop the temporary `# TEMPORARY (plan 0050 Lote 2…)` comment block (l. 416-423).
- Drop `continue-on-error: true` on the `Run Playwright tests` step (l. 425).
- Add `Write Playwright gateway config` step that writes a tiny `/tmp/garraia-test-config/config.yml` raising `gateway.rate_limit` to `per_second=1000, burst_size=5000`. The production default (`per_second=1, burst_size=60` in `garraia-config::model`) is exceeded by 9 sequential tests plus the admin UI's 10-second auto-refresh poll on `/admin/api/mcp` (`admin.html:1086`). The override is **CI-test-gateway-scoped** (env `GARRAIA_CONFIG_DIR=/tmp/garraia-test-config`) — production defaults stay intact. Discovered locally during plan 0052 verification: with the drift now fixed, tests reach the rate-limited code path that the 9 timeouts had previously masked.

### Plan/docs

- New plan file `plans/0052-gar-443-playwright-mcp-ui-drift.md` (this file).
- Append index row in `plans/README.md`.
- Append minimal `data-testid` convention note in `CLAUDE.md` `garraia-gateway` section.

## 4. Non-scope (hard blocks)

- **Backend handlers** (`crates/garraia-gateway/src/admin/handlers.rs`, `bootstrap.rs`) — untouched. The `POST /api/mcp/templates` endpoint already exists; we only wire UI to it.
- **MSRV bump** ([GAR-441](https://linear.app/chatgpt25/issue/GAR-441)).
- **RUSTSEC / `cargo audit`** — its CoE (`ci.yml:457` after this plan) stays.
- **Coverage / mutation / hotspot refactors** — out for this lote.
- **No new dependencies** in `tests/package.json` or any `Cargo.toml`.
- **No new feature flag**, no new env var, no migration.
- **No mobile / Flutter changes**, no providers, no auth changes.

## 5. Acceptance criteria

1. `cargo fmt --all -- --check` clean.
2. `cargo clippy --workspace --exclude garraia-desktop --all-targets -- -D warnings` clean.
3. `cargo test --workspace --exclude garraia-desktop` clean.
4. Local: `cargo build --bin garraia --release` + `./target/release/garraia start --port 3888` + `npx playwright test` → **9/9 passed** (no retries, no `--retries`).
5. CI run on the PR: step `Run Playwright tests` is **green** without `continue-on-error`.
6. `grep -cE "^\s*continue-on-error:\s*true\s*$" .github/workflows/ci.yml` → **1**.
7. `grep -n "GAR-NEW-Q17" .github/workflows/ci.yml` → empty (tag removed with the comment).
8. `grep -n "GAR-NEW-Q18" .github/workflows/ci.yml` → empty (already was).
9. No new dependency, no new feature flag, no backend touch.

## 6. Verification commands

```bash
cargo fmt --all -- --check
cargo clippy --workspace --exclude garraia-desktop --all-targets -- -D warnings
cargo build --bin garraia --release
./target/release/garraia start --host 127.0.0.1 --port 3888 &
echo $! > /tmp/garraia.pid
for i in $(seq 1 30); do curl -sf http://localhost:3888/health && break; sleep 1; done
cd tests
npm ci
npx playwright install chromium --with-deps
GARRAIA_BASE_URL=http://localhost:3888 GARRAIA_SKIP_SERVER=1 npx playwright test
kill "$(cat /tmp/garraia.pid)" || true
grep -cE "^\s*continue-on-error:\s*true\s*$" .github/workflows/ci.yml   # → 1
grep -n "GAR-NEW-Q17" .github/workflows/ci.yml                          # → empty
```

## 7. Risks & mitigations

| Risk | Mitigation |
|---|---|
| Tests 7 or 8 still fail after the migration for an unrelated reason. | Plan §5.4 forbids `test.skip` shortcut and forbids re-introducing CoE. If a new bug is found, fix it in the same lote. If it grows into a refactor, stop and ask the owner. |
| `data-testid` introduces a contract that downstream automation depends on. | Convention lives in the admin UI HTML (closed surface). `data-*` is HTML5-standard, ignored by CSS/screen readers — zero a11y impact. Documented in `CLAUDE.md`. |
| `Save as Template` button accidentally persists tokens. | Backend `save_mcp_template` strips non-placeholder env values to `<redacted>` (`handlers.rs:3234-3239`). UI passes `env: {}` for the template anyway. Token-leak test 3 still asserts the absence in the rendered page. |
| Playwright timing flake. | No `--retries` introduced. Existing 8s/10s waits kept. If a real flake appears in CI, fix the cause (not mask it). |

## 8. Rollback plan

- Single PR, fully reversible by `git revert <pr-sha>`.
- No schema, no migration, no provider, no secret, no key.
- The plan file stays as historical record post-revert (per `plans/` convention).

## 9. Out-of-scope clarifications captured during execution

- Test 9 ("Save as Template") originally referenced a button that **did not exist** in the UI. The backend endpoint (`POST /api/mcp/templates`) and `McpTemplate` schema already existed — only the button was missing. Wiring the existing endpoint is **not** a feature add and stays in scope under §3.
- Test 4 expected `'Save Changes'` button text and `'Edit MCP Server'` h3 prefix that the UI did not emit. These are minor copy fixes co-located with the `data-testid` migration in `showMcpForm`.
- Empty-name validation now returns explicit user feedback instead of silent no-op. Text `'Name is required.'` matches the spec's `/name.*required|required.*name/i`.

## 10. Files touched

| Path | Lines |
|---|---|
| `crates/garraia-gateway/src/admin.html` | ~30 (form + row + status badge) |
| `tests/playwright/mcp-manager.spec.ts` | full rewrite of selectors (~140 effective lines) |
| `.github/workflows/ci.yml` | -9 (CoE comment + flag) |
| `plans/0052-gar-443-playwright-mcp-ui-drift.md` | new |
| `plans/README.md` | +1 row |
| `CLAUDE.md` | +2 lines (`data-testid` convention) |

## 11. Done definition

- 9/9 Playwright tests pass locally with `npx playwright test` (no retry).
- CI run on the PR shows the `playwright` job verde without CoE.
- CoE counter is `1` post-merge.
- GAR-443 is marked Done with the PR attached.
- GAR-430 still In Progress (RUSTSEC / Q19 / Q20 follow-ups).
