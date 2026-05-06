# Plan 0071 — GAR-523: modeSidebar.js XSS — escapeAttr/escapeHtml on 3 sinks in showCustomModeForm

**Status:** ✅ Merged `659f8184` (PR #160, 2026-05-06)  
**Issue:** [GAR-523](https://linear.app/chatgpt25/issue/GAR-523)  
**Branch:** `health/202605060045-modesidebar-showmodeform-xss`  
**Created:** 2026-05-06 (Florida local time)

---

## 1. Goal

Fix 3 HIGH-severity CodeQL `js/xss` taint paths in `modeSidebar.js`'s
`showCustomModeForm()` function that were **not covered** by GAR-519 (plan 0067).

GAR-519 fixed 7 sinks in `renderModeItem()` and `updateModeBadge()`. The
`showCustomModeForm()` function builds a `modalHtml` template literal that injects
server-fetched `existingMode.name`, `existingMode.description`, and
`existingMode.promptOverride` into HTML attribute values and `<textarea>` content
without escaping — all three are stored XSS paths reachable by any user with access
to `/api/modes/custom`.

## 2. Architecture

Single-file change in `crates/garraia-gateway/assets/modeSidebar.js`.
`escapeHtml` and `escapeAttr` are already imported (added by GAR-519, line 4).
No new imports, no Rust changes, no DB changes, no CI config changes.

## 3. Tech Stack

- JavaScript ES module (`modeSidebar.js`)
- `utils.js` provides `escapeHtml` and `escapeAttr` (added by GAR-517, PR #144)

## 4. Design Invariants

- `escapeAttr` for values injected into HTML attribute context (`value="..."`)
- `escapeHtml` for content injected inside element context (`<textarea>...</textarea>`)
- Numeric fields (`temperature`, `max_tokens`, `top_p`) hardened with `escapeAttr`
  as defensive measure even though backend validates as numbers
- Static strings (`isEdit ? 'Update' : 'Create'`) left unwrapped — no server data

## 5. Out of Scope

- No changes to `/api/modes/custom` backend validation
- No changes to other JS files (covered by prior GAR-510/512/515/517/519)
- No changes to `renderModeItem()` or `updateModeBadge()` (already fixed by GAR-519)
- No Rust, HTML, or CI workflow changes

## 6. Rollback

Revert the single commit on `modeSidebar.js`. No data migration needed.

## 7. Sinks Fixed

| # | Function | Line | Expression | Sink type | Fix |
|---|----------|------|-----------|-----------|-----|
| 1 | `showCustomModeForm` | 394 | `value="${existingMode?.name \|\| ''}"` | HTML attribute | `escapeAttr(existingMode?.name \|\| '')` |
| 2 | `showCustomModeForm` | 399 | `value="${existingMode?.description \|\| ''}"` | HTML attribute | `escapeAttr(existingMode?.description \|\| '')` |
| 3 | `showCustomModeForm` | 419 | `${existingMode?.promptOverride \|\| ''}` inside `<textarea>` | element content | `escapeHtml(existingMode?.promptOverride \|\| '')` |
| 4 | `showCustomModeForm` | 427 | `value="${existingMode?.defaults?.temperature \|\| 0.7}"` | HTML attribute | `escapeAttr(...)` (defensive) |
| 5 | `showCustomModeForm` | 431 | `value="${existingMode?.defaults?.max_tokens \|\| 4096}"` | HTML attribute | `escapeAttr(...)` (defensive) |
| 6 | `showCustomModeForm` | 435 | `value="${existingMode?.defaults?.top_p \|\| 0.9}"` | HTML attribute | `escapeAttr(...)` (defensive) |

## 8. Tasks

- [x] T1 — Create branch `health/202605060045-modesidebar-showmodeform-xss` off main
- [x] T2 — Create this plan file
- [x] T3 — File Linear issue GAR-523 under GAR-486 umbrella
- [x] T4 — Update `plans/README.md` with plan 0071 row
- [x] T5 — Implement fix in `modeSidebar.js` (wrap 6 sinks with escapeAttr/escapeHtml)
- [x] T6 — Commit `fix(gateway): GAR-523 — modeSidebar.js XSS: escapeAttr/escapeHtml on 3 sinks in showCustomModeForm`
- [x] T7 — Push and open PR to main
- [x] T8 — CI green (all 18 checks pass)
- [x] T9 — Squash-merge via GitHub MCP
- [x] T10 — Mark GAR-523 Done in Linear
- [x] T11 — Update plan row with merged SHA + PR number

## 9. Risk Register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|-----------|
| `escapeHtml` in `<textarea>` breaks HTML entities display | Low | Low | `escapeHtml` using `d.textContent = s; return d.innerHTML` produces entities like `&lt;` which textarea renders as literal `<` — correct behaviour |
| `escapeAttr` breaks numeric `type="number"` input pre-fill | Low | Low | `escapeAttr` of `"0.7"` returns `"0.7"` unchanged (no special chars in normal floats) |
| Stored XSS payload already present in DB from a mode created before fix | Medium | High | The fix prevents future injections; existing modes with XSS payloads in `name`/`description`/`promptOverride` will now display as literal text in the edit modal |

## 10. Acceptance Criteria

- `CodeQL Analyze (javascript-typescript)` does not flag `modeSidebar.js:showCustomModeForm` for `js/xss`
- Edit modal correctly pre-fills mode name, description, and prompt override as literal text (HTML entities rendered correctly by browser)
- No regression in mode create/update flow
- All 18 CI checks green

## 11. Open Questions

None. Pattern established by GAR-519 / plan 0067.

## 12. Cross-References

- GAR-519 — modeSidebar.js XSS Wave 1: `renderModeItem` + `updateModeBadge` (plan 0067, Done)
- GAR-517 — api.js/mcpView.js XSS + utils.js creation (plan 0065, Done)
- GAR-510 — admin.html XSS (plan 0057, Done)
- GAR-512 — webchat.html XSS (plan 0061, Done)
- GAR-515 — webchat.html DOMPurify (plan 0063, Done)
- GAR-486 — Green Security Baseline umbrella (In Progress)
- utils.js — `escapeHtml` + `escapeAttr` added by GAR-517 (PR #144)

## 13. Estimativa

0.5h implementation + CI wait (~20min).
