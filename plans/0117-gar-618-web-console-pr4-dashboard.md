# Plan 0117 — GAR-618: Web Console PR-4 — Dashboard page + `GET /api/stats`

> **For agentic workers:** implement task-by-task following the checkbox steps in §M1.

**Linear issue:** [GAR-618](https://linear.app/chatgpt25/issue/GAR-618) — "Web Console PR-4: Dashboard page + GET /api/stats (Garra Glass multi-page)" (In Progress, High). Parent: GAR-607 (Garra Glass umbrella).

**Status:** ⏳ Draft — approved 2026-05-14 (Florida).

**Goal:** Add a live Dashboard section to `webchat.html` with 4 glass MetricCards (Gateway Status, Uptime, Version, Active Sessions) powered by a new `GET /api/stats` Rust endpoint. Activates the `.reveal-card` stagger animation hook reserved in PR-C (plan 0116-PR-C / GAR-610). This is the 4th slice of the Garra Glass multi-page console rollout (plan 0116b umbrella).

---

## Architecture

1. **`GET /api/stats` handler** — new `crates/garraia-gateway/src/stats_handler.rs` (~60 LOC):
   - Extracts `uptime_secs` from `state.boot_time.elapsed().as_secs()`
   - Extracts `active_sessions` from `state.sessions.len()`
   - Returns `StatsResponse { version: &str, uptime_secs: u64, active_sessions: usize, gateway_status: &str }`
   - No DB, no auth dependency — pure memory read, suitable as a liveness indicator
   - Registered at `GET /api/stats` in `router.rs` (public, no CSRF — same as `/ping`)

2. **Dashboard section in `webchat.html`** (~150 CSS + ~80 HTML + ~60 JS LOC):
   - Sidebar: new nav item "Dashboard" above "Settings" in `.sidebar-footer`, with grid SVG icon and `data-section="dashboard"` attribute
   - CSS: `.section` display-toggle pattern — `.chat-area { display: none }` when `.dashboard-section` is active; section switching via `showSection(name)` JS function
   - Dashboard HTML: `<section id="dashboard-section" class="dashboard-section" hidden>` containing a `.metric-grid` with 4 `<article class="metric-card glass-panel reveal-card">` cards
   - MetricCard structure: `.metric-icon` (SVG) + `.metric-value` (number/text) + `.metric-label` (caption)
   - Cards: **Gateway Status** (online/offline pill), **Uptime** (formatted "Xh Ym"), **Version** ("v0.2.0"), **Active Sessions** (integer)
   - `.reveal-card` stagger: `nth-child` `animation-delay` 0ms / 80ms / 160ms / 240ms via CSS
   - JS `fetchStats()`: `fetch('/api/stats')` → JSON → populate card values; called on section-switch + every 30s via `setInterval`
   - Sidebar nav active state: `.sidebar-nav-item.active` class on the selected section button

3. **Playwright test** (`tests/playwright/webchat-redesign.spec.ts`):
   - New test case: click Dashboard nav item → verify `#dashboard-section` visible + 4 `.metric-card` present + `.metric-value` non-empty after fetch

---

## Tech stack

- Rust: Axum 0.8, `serde_json`, `crate::state::SharedState`
- HTML/CSS/JS: vanilla, no external deps (Garra Glass ADR 0009 §3 — no CDN imports)
- Tests: Playwright (TypeScript), existing `webchat-redesign.spec.ts` extended

---

## Design invariants

1. **No CDN imports.** All CSS/SVG/JS must be inline in `webchat.html`. ADR 0009 §3.
2. **`GET /api/stats` is public (no auth, no CSRF).** Same as `/ping` and `/health`. Dashboard stats are operational metrics, not user data.
3. **No DB call in stats handler.** `state.sessions.len()` is an O(1) DashMap read. `state.boot_time` is an `Instant`. Zero await needed — handler is sync-compatible.
4. **Section switching must preserve chat state.** `showSection('chat')` hides the dashboard and shows the chat-area; it must NOT clear chat messages or WebSocket state.
5. **`.reveal-card` animation fires only on initial reveal.** CSS `animation-fill-mode: forwards` + `animation-play-state: running` only when `.dashboard-section` is visible. Switching away and back re-triggers the animation (acceptable for this slice; debounce deferred).
6. **Active sessions count uses `state.sessions` (legacy SQLite sessions DashMap), not Postgres `sessions` table.** This is intentional — it reflects active in-memory WebSocket/SSE sessions, not persisted auth sessions. Label says "Active Sessions" with a tooltip "WebSocket/API sessions" to avoid confusion. Postgres session count is deferred to a future Providers page slice.
7. **Garra Glass tokens only.** Colors use `var(--garra-gold)`, `var(--garra-cyan)`, `var(--garra-text)`, `var(--bg-glass)` etc. — never hardcoded hex.

---

## Validações pré-plano

- ✅ `state.boot_time: std::time::Instant` exists in `AppState` (`crates/garraia-gateway/src/state.rs:67`)
- ✅ `state.sessions: DashMap<String, SessionState>` exists (`state.rs:31`) — `.len()` is O(1)
- ✅ `GET /ping` and `GET /health` are registered without auth in `router.rs:98-100` — same pattern for `/api/stats`
- ✅ `.reveal-card` CSS class is already defined in `webchat.html` (PR-C / plan 0116-PR-C) — no new CSS keyframe needed
- ✅ `tests/playwright/webchat-redesign.spec.ts` exists (added in PR-C) — can be extended
- ✅ Garra Glass CSS tokens (`--garra-gold`, `--garra-cyan`, `--bg-glass`, etc.) are defined in `webchat.html` `:root` (PR-A / plan 0116-PR-A)
- ✅ `SharedState` type alias for `Arc<AppState>` available via `crate::state`
- ✅ Workspace version is `"0.2.0"` (root `Cargo.toml`)

---

## Out of scope

- Providers page (PR-5), Channels page (PR-6), Sessions page (PR-7) — separate slices
- Real-time WebSocket push for dashboard metrics — polling every 30s is sufficient for this slice
- Postgres session count from `sessions` table — deferred (requires DB pool access in stats handler)
- Auth-gated `/api/admin/stats` with extended metrics — deferred to admin panel slice
- Dashboard chart/graph — deferred (no charting library; plan 0116b §"Dashboard" shows card-only MVP)
- Light-theme screenshot attachment — manual step post-merge

---

## Rollback

PR is squash-merged; reverting the squash commit on main removes all changes atomically. `webchat.html` and `stats_handler.rs` are the only changed files — no schema migration, no Cargo dependency added.

---

## §12 Open questions

- Q1: Should `GET /api/stats` be authenticated (Bearer token)? **Decision: No** — same policy as `/ping`. Dashboard stats are operational metadata, not user data. If a future operator-only stats endpoint is needed, it goes to `/api/admin/stats` with `require_admin_auth`.
- Q2: Should the uptime be formatted server-side or client-side? **Decision: Client-side** — server returns raw `uptime_secs: u64`; JS formats it as "Xh Ym Zs". Simpler to localize later.

---

## File structure

```
crates/garraia-gateway/src/
  stats_handler.rs         ← NEW (~60 LOC): StatsResponse struct + handler fn
  router.rs                ← MODIFIED: add GET /api/stats route
  webchat.html             ← MODIFIED: Dashboard section CSS/HTML/JS + sidebar nav item
tests/playwright/
  webchat-redesign.spec.ts ← MODIFIED: add Dashboard navigation test case
```

---

## M1 Tasks

### T1 — `GET /api/stats` Rust handler

- [ ] Create `crates/garraia-gateway/src/stats_handler.rs` with:
  - `#[derive(serde::Serialize)] pub struct StatsResponse { version: &'static str, uptime_secs: u64, active_sessions: usize, gateway_status: &'static str }`
  - `pub async fn stats_handler(State(state): State<SharedState>) -> Json<StatsResponse>`
  - Compute `uptime_secs = state.boot_time.elapsed().as_secs()`
  - Compute `active_sessions = state.sessions.len()`
  - Return `Json(StatsResponse { version: env!("CARGO_PKG_VERSION"), uptime_secs, active_sessions, gateway_status: "online" })`
- [ ] Register in `router.rs`: `.route("/api/stats", get(stats_handler::stats_handler))`
- [ ] `cargo check -p garraia-gateway` green

### T2 — Dashboard CSS + sidebar nav

- [ ] In `webchat.html` `<style>` section, add:
  - `.metric-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 16px; padding: 24px; }`
  - `.metric-card { display: flex; flex-direction: column; gap: 8px; padding: 20px 24px; align-items: flex-start; }`
  - `.metric-card .metric-value { font-size: 32px; font-weight: 800; color: var(--garra-gold); font-family: var(--font-mono); }`
  - `.metric-card .metric-label { font-size: 12px; color: var(--garra-muted); text-transform: uppercase; letter-spacing: 0.06em; }`
  - `.reveal-card:nth-child(1) { animation-delay: 0ms; }` ... `:nth-child(4) { animation-delay: 240ms; }`
  - `.dashboard-section { flex: 1; overflow-y: auto; min-height: 0; }`
  - `.chat-area.hidden, .dashboard-section.hidden { display: none !important; }`
- [ ] In sidebar HTML, add Dashboard nav button above the Settings footer button:
  ```html
  <button class="sidebar-footer-btn" id="dashboard-nav-btn" data-section="dashboard" title="Dashboard">
    <svg class="icon-svg" viewBox="0 0 16 16" aria-hidden="true"><!-- grid icon --></svg>
    Dashboard
  </button>
  ```
- [ ] Add `data-testid="garra-dashboard-nav"` attribute to the Dashboard button

### T3 — Dashboard HTML section

- [ ] Insert `<section id="dashboard-section" class="dashboard-section hidden" data-testid="garra-dashboard-section">` after `</main>` (after chat-area):
  ```html
  <section id="dashboard-section" class="dashboard-section hidden" data-testid="garra-dashboard-section">
    <div class="panel-header" style="padding: 16px 24px; border-bottom: 1px solid var(--border);">
      <h2 style="margin:0; font-size:18px; font-weight:700; color:var(--garra-text);">Dashboard</h2>
    </div>
    <div class="metric-grid" id="metric-grid">
      <article class="metric-card glass-panel reveal-card" data-testid="metric-card-status">
        <span class="metric-label">Gateway Status</span>
        <span class="metric-value" id="metric-status">—</span>
      </article>
      <article class="metric-card glass-panel reveal-card" data-testid="metric-card-uptime">
        <span class="metric-label">Uptime</span>
        <span class="metric-value" id="metric-uptime">—</span>
      </article>
      <article class="metric-card glass-panel reveal-card" data-testid="metric-card-version">
        <span class="metric-label">Version</span>
        <span class="metric-value" id="metric-version">—</span>
      </article>
      <article class="metric-card glass-panel reveal-card" data-testid="metric-card-sessions">
        <span class="metric-label">Active Sessions</span>
        <span class="metric-value" id="metric-sessions">—</span>
      </article>
    </div>
  </section>
  ```

### T4 — JavaScript section-switching + stats fetch

- [ ] Add `showSection(name)` function:
  ```js
  function showSection(name) {
    document.querySelector('.chat-area').classList.toggle('hidden', name !== 'chat');
    document.getElementById('dashboard-section').classList.toggle('hidden', name !== 'dashboard');
    document.querySelectorAll('[data-section]').forEach(btn => {
      btn.classList.toggle('active', btn.dataset.section === name);
    });
    if (name === 'dashboard') fetchStats();
  }
  ```
- [ ] Wire click handler: `document.getElementById('dashboard-nav-btn').addEventListener('click', () => showSection('dashboard'));`
- [ ] Wire chat area: make the sidebar "New Chat" button and session list items call `showSection('chat')` when clicked (or ensure chat is default)
- [ ] Add `fetchStats()`:
  ```js
  function formatUptime(secs) {
    const h = Math.floor(secs / 3600), m = Math.floor((secs % 3600) / 60), s = secs % 60;
    return h > 0 ? `${h}h ${m}m` : m > 0 ? `${m}m ${s}s` : `${s}s`;
  }
  async function fetchStats() {
    try {
      const r = await fetch('/api/stats'); if (!r.ok) return;
      const d = await r.json();
      document.getElementById('metric-status').textContent = d.gateway_status ?? '—';
      document.getElementById('metric-uptime').textContent = formatUptime(d.uptime_secs ?? 0);
      document.getElementById('metric-version').textContent = d.version ? `v${d.version}` : '—';
      document.getElementById('metric-sessions').textContent = d.active_sessions ?? '—';
    } catch (_) {}
  }
  ```
- [ ] Schedule: `setInterval(fetchStats, 30_000);`
- [ ] Call `showSection('chat')` on page init to ensure default is chat

### T5 — Playwright test extension

- [ ] In `tests/playwright/webchat-redesign.spec.ts`, add test:
  ```ts
  test('Dashboard section is reachable and shows 4 metric cards', async ({ page }) => {
    await page.goto('/');
    await page.click('[data-testid="garra-dashboard-nav"]');
    await expect(page.locator('[data-testid="garra-dashboard-section"]')).toBeVisible();
    await expect(page.locator('[data-testid^="metric-card-"]')).toHaveCount(4);
    // After fetch, metric-value should not be '—'
    await page.waitForFunction(() =>
      document.getElementById('metric-version')?.textContent !== '—'
    , { timeout: 5000 });
  });
  ```

### T6 — Cargo fmt + Clippy + compile check

- [ ] `cargo fmt -p garraia-gateway`
- [ ] `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` → zero warnings
- [ ] `cargo check -p garraia-gateway` green

### T7 — Commit and push

- [ ] `git add crates/garraia-gateway/src/stats_handler.rs crates/garraia-gateway/src/router.rs crates/garraia-gateway/src/webchat.html tests/playwright/webchat-redesign.spec.ts`
- [ ] Commit: `feat(web-console): plan 0117 — Dashboard page + GET /api/stats (Garra Glass PR-4)`

### T8 — Bookkeeping

- [ ] Add plan 0117 row to `plans/README.md`
- [ ] Update plans/README.md rows for 0116a (mark PR-A merged) and 0116b status
- [ ] Commit: `docs(plans): add plan 0117 + update 0116a/b status`

---

## Risk register

| Risk | Mitigation |
|---|---|
| `state.sessions.len()` is the legacy SQLite session map, not Postgres sessions | Documented in Design invariants §6; label clarifies "WebSocket/API sessions" |
| Dashboard CSS breaks chat layout | `showSection()` uses `display:none` — sections are mutually exclusive; no layout reflow |
| Playwright test flaky on stats fetch | `waitForFunction` with 5s timeout; `fetch` error is silently caught (card stays at "—") |
| PR-C's `reveal-card` CSS may not yet be on main when this branch is cut | Verified: `git grep reveal-card` on origin/main shows the hook (merged in PR #331) |

---

## Acceptance criteria

1. `GET /api/stats` returns HTTP 200 with `{"version":"0.2.0","uptime_secs":N,"active_sessions":N,"gateway_status":"online"}`.
2. Clicking "Dashboard" in the sidebar shows `#dashboard-section` and hides the chat area.
3. After fetch, all 4 MetricCard `.metric-value` elements are non-empty (not "—").
4. `.reveal-card` cards animate in with stagger on Dashboard reveal.
5. Returning to Chat via new-chat-btn or session click restores the chat view.
6. `cargo clippy --workspace ... -D warnings` green.
7. All existing Playwright tests pass + new Dashboard test green.
8. CI: Format + Clippy + Test×3 + Build + Playwright all success.

---

## Cross-references

- GAR-607: Garra Glass umbrella (parent)
- GAR-610: PR-C that shipped `.reveal-card` hook (prerequisite)
- plan 0116a: Chat slice (PR-A through PR-C, now complete)
- plan 0116b: Multi-page vision (this is PR-4 of 10)
- ADR 0009: Garra Glass design system tokens + CDN-free invariant

---

## Estimativa

- Implementação: 1–2h
- Review + CI: 30–60min
- Total: 1.5 / 2.5 / 3h
