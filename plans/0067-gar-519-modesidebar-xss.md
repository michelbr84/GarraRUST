# Plan 0067 — GAR-519: modeSidebar.js XSS — wrap mode.name/description/id in escapeHtml/escapeAttr

**Status:** 🟡 Em execução  
**Issue:** [GAR-519](https://linear.app/chatgpt25/issue/GAR-519)  
**Branch:** `health/202605051500-modesidebar-xss`  
**Created:** 2026-05-05 (Florida local time)

---

## 1. Goal

Fix 7 CodeQL `js/xss` (High severity) taint paths in `modeSidebar.js` where user-controlled
data from `/api/modes/custom` (`mode.name`, `mode.description`, `mode.id`) is injected into
`innerHTML` and HTML attributes without escaping. Same pattern as GAR-510 / GAR-512 / GAR-517.

## 2. Architecture

Single-file change in `crates/garraia-gateway/assets/modeSidebar.js`:
- Add `import { escapeHtml, escapeAttr } from './utils.js';` (utils.js already ships from GAR-517)
- Wrap 7 taint sinks in `renderModeItem()` and `updateModeBadge()`

No Rust changes. No DB changes. No CI config changes.

## 3. Tech Stack

- JavaScript ES module (`modeSidebar.js`)
- `utils.js` already provides `escapeHtml` and `escapeAttr` (added by GAR-517, PR #144)

## 4. Design Invariants

- `escapeHtml` for innerHTML text content (prevents tag injection)
- `escapeAttr` for HTML attribute values (prevents attribute escape)
- `mode.id` used as `data-mode-id` attribute value → `escapeAttr`
- `mode.description` used in `title=""` attribute → `escapeAttr`
- `mode.name`, `mode.description` in text nodes inside innerHTML → `escapeHtml`
- Static/i18n strings that never carry server data are left unwrapped

## 5. Out of Scope

- No changes to `/api/modes` or `/api/modes/custom` backend validation (existing server-side)
- No changes to other files (Rust, HTML, CI workflows)
- No changes to other JS view files already fixed by GAR-517

## 6. Rollback

Revert the single commit on `modeSidebar.js`. No data migration needed.

## 7. Sinks Fixed

| # | Function | Line | Expression | Fix |
|---|----------|------|-----------|-----|
| 1 | `renderModeItem` | 278 | `data-mode-id="${mode.id}"` (edit btn) | `escapeAttr(mode.id)` |
| 2 | `renderModeItem` | 281 | `data-mode-id="${mode.id}"` (delete btn) | `escapeAttr(mode.id)` |
| 3 | `renderModeItem` | 289 | `data-mode-id="${mode.id}"` (item div) | `escapeAttr(mode.id)` |
| 4 | `renderModeItem` | 290 | `title="${mode.description}"` | `escapeAttr(mode.description)` |
| 5 | `renderModeItem` | 293 | `${mode.name}` in innerHTML | `escapeHtml(mode.name)` |
| 6 | `renderModeItem` | 294 | `${mode.description}` in innerHTML | `escapeHtml(mode.description)` |
| 7 | `updateModeBadge` | 530 | `${mode.name}` in innerHTML | `escapeHtml(mode.name)` |

## 8. Tasks

- [x] T1 — Create branch `health/202605051500-modesidebar-xss` off main
- [x] T2 — Create this plan file
- [x] T3 — Update `plans/README.md` with plan 0067 row
- [x] T4 — Implement fix in `modeSidebar.js` (add import + wrap 7 sinks)
- [ ] T5 — Commit `fix(gateway): GAR-519 — modeSidebar.js XSS: escapeHtml/escapeAttr on 7 sinks`
- [ ] T6 — Push and open PR to main
- [ ] T7 — CI green (all 18 checks pass)
- [ ] T8 — Squash-merge via GitHub MCP
- [ ] T9 — Mark GAR-519 Done in Linear

## 9. Risk Register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|-----------|
| Custom mode `name`/`id` contains `<script>` → XSS | High (user-controlled) | High (stored XSS) | **This plan** |
| `escapeAttr` changes `data-mode-id` used in `querySelector` or `dataset.modeId` | Low | Medium | `dataset.modeId` reads back unescaped; `escapeAttr` only affects the attribute string during HTML generation — `dataset.modeId` will still return the original value |

## 10. Acceptance Criteria

- `CodeQL Analyze (javascript-typescript)` does not flag `modeSidebar.js` for `js/xss`
- Mode sidebar renders correctly (names and descriptions display as text, not HTML)
- Mode selection still works (click handler reads `dataset.modeId` correctly)
- All 18 CI checks green

## 11. Cross-References

- GAR-510 — admin.html XSS (plan 0057, Done)
- GAR-512 — webchat.html XSS (plan 0061, Done)
- GAR-517 — api.js/mcpView.js XSS (plan 0065, Done)
- GAR-486 — Green Security Baseline 2026-04-30 (umbrella, In Progress)
- utils.js — `escapeHtml` + `escapeAttr` added by GAR-517 (PR #144, `15ec51e3`)

## 12. Estimativa

0.5h implementation + CI wait.
