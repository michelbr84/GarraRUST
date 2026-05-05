# Plan 0065 — GAR-517: api.js + mcpView.js XSS — add escapeHtml utils module

**Status:** Em execução  
**Issue Linear:** [GAR-517](https://linear.app/chatgpt25/issue/GAR-517)  
**Branch:** `health/202605051300-api-js-xss`  
**Criado em:** 2026-05-05 (Florida time)

---

## 1. Goal

Close 7 CodeQL `js/xss` HIGH-severity taint paths in the ES-module assets layer
(`assets/api.js`, `assets/views/mcpView.js`, `assets/views/memoryView.js`) that were
missed by the previous XSS rounds (GAR-510, GAR-512, GAR-515 targeted admin.html and
webchat.html but not the ES-module layer).

---

## 2. Architecture

```
assets/
  utils.js          ← NEW: export escapeHtml(s), escapeAttr(s)
  api.js            ← FIX: import utils, wrap 3 innerHTML sinks
  views/
    mcpView.js      ← FIX: import utils, wrap 3 innerHTML sinks
    memoryView.js   ← FIX: import utils, wrap 1 innerHTML sink
```

`escapeHtml` and `escapeAttr` implementations are identical to the inline helpers
already in `webchat.html` (DOM-based, no regex, safe against all bypass vectors).

---

## 3. Tech stack

- Vanilla ES2020 modules (no bundler, no TypeScript)
- CodeQL JavaScript/TypeScript analysis (`js/xss` rule)
- Axum static file serving (no build step — files served as-is)

---

## 4. Design invariants

1. `utils.js` is a **pure** module — no side effects, no imports, no DOM access.
2. Every value sourced from `fetch().json()` that flows into `innerHTML` MUST be
   wrapped with `escapeHtml()` or the element must be built via DOM API (`textContent`).
3. Static string literals (e.g. `'<span class="online">Connected</span>'`) do NOT
   need escaping — they are not tainted.
4. Boolean ternaries producing known static strings do NOT need escaping.
5. `e.message` from caught `Error` objects is treated as potentially tainted
   (user-supplied error text can contain HTML) — wrap with `escapeHtml`.

---

## 5. Out of scope

- Rust / backend code changes
- Admin.html, webchat.html (already fixed in prior GAR rounds)
- chatView.js (`connPill.innerHTML` uses static strings / i18n only — no tainted data)
- logsView.js (static loading string only)
- TypeScript migration
- Content-Security-Policy headers (separate epic)

---

## 6. Rollback

Pure JS change — revert commit `git revert <sha>` and push. No schema or binary impact.

---

## 7. Open questions

None — pattern is identical to GAR-510/512/515; implementation is mechanical.

---

## 8. File structure

```
plans/0065-gar-517-api-js-xss-utils.md           ← this file
crates/garraia-gateway/assets/utils.js            ← NEW
crates/garraia-gateway/assets/api.js              ← MODIFIED
crates/garraia-gateway/assets/views/mcpView.js    ← MODIFIED
crates/garraia-gateway/assets/views/memoryView.js ← MODIFIED
plans/README.md                                   ← row added
```

---

## 9. Tasks

- [x] T1 — Create `assets/utils.js` exporting `escapeHtml(s)` + `escapeAttr(s)`
- [x] T2 — Fix `api.js`: import utils, wrap `j.status`, `j.latest_version`/`j.version`, `ch`
- [x] T3 — Fix `mcpView.js`: import utils, wrap `s.name`, `s.tools`, `e.message`
- [x] T4 — Fix `memoryView.js`: import utils, wrap `e.message` in error innerHTML
- [x] T5 — `cargo check -p garraia-gateway` + clippy (verifies no Rust breakage from asset serving)
- [x] T6 — Commit, push, open PR
- [x] T7 — CI green (≥16 checks), squash-merge
- [x] T8 — Update `plans/README.md`, mark GAR-517 Done in Linear

---

## 10. Risk register

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| `escapeHtml` double-encoding if called on already-safe string | Low | Low | All sinks are raw API data, not pre-escaped strings |
| i18n `window.t()` returns HTML — escaping its inputs may break formatting | Low | Low | Escape values passed TO `window.t`, not the result; static replacement stays |
| Module import path wrong (relative vs absolute) | Low | Low | Use `../utils.js` from views/, `./utils.js` from api.js |

---

## 11. Acceptance criteria

- [ ] `cargo check -p garraia-gateway` exits 0
- [ ] `cargo clippy --workspace --tests --exclude garraia-desktop --no-deps -- -D warnings` exits 0
- [ ] All 7 sinks wrapped with `escapeHtml()` or equivalent DOM API
- [ ] No `innerHTML` assignment in api.js / mcpView.js / memoryView.js injects raw API response data
- [ ] CI ≥16 checks green on PR
- [ ] CodeQL `Analyze (javascript-typescript)` re-run closes the alerts

---

## 12. Cross-references

- GAR-486 (security hardening umbrella — parent)
- GAR-510 (admin.html XSS — precedent)
- GAR-512 (webchat.html XSS — precedent)
- GAR-515 (webchat.html DOMPurify — precedent)
- `docs/adr/0005-identity-provider.md` (out of scope for this fix)

---

## 13. Estimativa

2 story points — pure JS, no schema, no Rust changes, clear precedent pattern.
