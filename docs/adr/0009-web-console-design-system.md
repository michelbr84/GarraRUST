# ADR 0009 — Web Console Design System "Garra Glass"

- **Status:** Accepted (2026-05-14, Florida local)
- **Deciders:** michelbr84
- **Plan:** [`plans/0116a-web-console-redesign-garra-glass.md`](../../plans/0116a-web-console-redesign-garra-glass.md) (PR-A foundation slice), [`plans/0116b-transformar-a-tela-atual-do-GarraIA.md`](../../plans/0116b-transformar-a-tela-atual-do-GarraIA.md) (full multi-page vision)
- **Supersedes:** —
- **Related:** ADR 0007 (Desktop frontend) — distinct surface; this ADR governs the *web console* served by `garraia-gateway` only.

## Context

The web console served at `GET /` by `garraia-gateway` (`crates/garraia-gateway/src/router.rs:382-387` → `webchat.html`) historically used a minimal functional palette (`--bg-primary: #f8f9fa` light / `#1a1a1f` dark, blue accent `#3b82f6`). It worked but did not project the GarraIA brand and made the gateway feel like a generic chat shell.

Plan 0116 proposes "Garra Glass" — a design system inspired by the AdminLTE 4 mockup the user provided (`garraia_dashboard_html.html`), characterised by:

- Multi-radial gold/cyan/purple background gradient + 44px grid + noise overlay.
- Glassmorphism (translucent panels with `backdrop-filter: blur(18px)`).
- Gold (`#ffd400`) for CTAs, cyan (`#16d9ff`) for info/focus, semantic red/amber/green for alerts.
- Inter 400-900 + JetBrains Mono for code/IDs.
- Reusable `pill` / `status-dot` / `info-list` / `glass-panel` / `garra-select` / `garra-input` primitives.

The mockup uses Bootstrap 5 + Bootstrap Icons + AdminLTE classes loaded from CDN. Adopting those at runtime would (a) introduce supply-chain risk (`crates/garraia-gateway` is bundled into a single binary that runs in air-gapped Runpod environments), (b) couple the web console to network availability for fonts/icons, and (c) trip Playwright specs that depend on our own classes.

## Decision

**Adopt the `--garra-*` CSS custom-property palette as the canonical design system for the web console**, ported natively into `webchat.html` (and later `admin.html`) without importing Bootstrap / AdminLTE / Animate.css runtime bundles.

Specific rules:

1. **Tokens are the source of truth.** Every new style reads from `--garra-bg`, `--garra-panel`, `--garra-primary`, `--garra-accent`, `--garra-text`, `--garra-radius`, `--garra-shadow`, `--garra-font`, `--garra-mono`, etc. Legacy `--bg-primary` / `--accent` tokens are retained for the still-migrating chat / right-panel sections and will be retired in plan 0118 once every usage has migrated.
2. **No CDN frameworks at runtime.** Google Fonts for Inter / JetBrains Mono remains (already cached). All Bootstrap-Icons-equivalent glyphs ship as inline SVG copied from the Bootstrap Icons MIT-licensed source; no `<link>` to a Bootstrap Icons CDN.
3. **Glassmorphism is opt-in per surface.** `backdrop-filter: blur(18px)` lands only on `.glass-panel`, `.app-header`, `.chat-console`, `.context-panel`. Sidebar and chat scroll area keep solid backgrounds for performance.
4. **Dual theme attributes.** Both `data-theme="light|dark"` (legacy) and `data-bs-theme="light|dark"` (mockup native) drive the Garra tokens, so future migrations to an AdminLTE-derived `admin.html` redesign (plan 0117) need no JS rewrite.
5. **Accessibility minimum.** AA contrast preserved on translucent panels in both themes. Focus rings are explicit (`outline` or `box-shadow`), never `outline: none` without substitute. `@supports not (backdrop-filter)` provides a solid-fill fallback for older browsers.
6. **No backwards-compatibility shims.** The plan 0117 redesign of `admin.html` will adopt this same system. Once `admin.html` is migrated, the legacy `--bg-*` aliases retire (plan 0118).

## Consequences

### Positive

- Single coherent visual identity across `webchat.html`, `admin.html` (plan 0117) and any future surface served by `garraia-gateway`.
- Zero new runtime dependencies: bundle delta is ~+18 KB unminified per surface.
- Works air-gapped (Runpod, offline freezing tests) because everything is `include_str!`-embedded.
- Light / dark parity is built in at the token layer; no theme-specific JS branches.

### Negative

- Hand-porting Bootstrap Icons paths is tedious (~18 icons across the full plan 0116a roll-out). Mitigation: a single sprite sheet considered for plan 0118 cleanup.
- `backdrop-filter: blur(18px)` is GPU-expensive. Mitigation: only on the four glass surfaces above; `@supports not` fallback provided.
- Light-theme contrast on `.pill.success` / `.pill.warning` / `.pill.danger` needs explicit overrides to avoid washing out (handled in plan 0116-PR-B).

### Neutral

- Skin/theme persistence stays in `localStorage` for now (plan 0121 will move it server-side via the settings registry).
- Internationalization remains out of scope (plan 0119 candidate).

## Alternatives considered

1. **Import AdminLTE 4 from CDN.** Rejected — supply-chain surface, runtime network dependency, air-gap incompatibility, hard to reproduce in CI.
2. **Migrate the web console to a SPA framework (React / Svelte / Vue).** Rejected — `deep-research-report.md` §UI explicitly rejects this for the gateway-served console (the Tauri desktop app is the SPA surface). Keeping vanilla JS minimises bundle size and review surface.
3. **Use only the existing `--bg-*` palette plus a darker variant.** Rejected — the mockup deliberately introduces gold/cyan/glass aesthetic; merely retinting existing tokens cannot reach the visual target.

## References

- Plan 0116a (`plans/0116a-web-console-redesign-garra-glass.md`)
- Plan 0116b (`plans/0116b-transformar-a-tela-atual-do-GarraIA.md`)
- Mockup: `garraia_dashboard_html.html` (user-provided, not checked into repo)
- File: `crates/garraia-gateway/src/webchat.html`
- Followup plans: 0117 (admin.html redesign), 0118 (asset extraction + alias retirement), 0119 (i18n), 0120 (a11y), 0121 (settings-backed skin persistence)
