# Plan 0063 — GAR-515: webchat.html XSS — DOMPurify for marked.parse() + langLabel + event delegation

**Status:** ⏳ Draft  
**Linear:** [GAR-515](https://linear.app/chatgpt25/issue/GAR-515)  
**Branch:** `health/202605051248-webchat-dompurify`  
**Created:** 2026-05-05 (Florida)  
**Author:** health-routine

---

## §1 Goal

Close two HIGH-severity CodeQL `js/xss` alerts in `webchat.html`:

1. **Sink L1707** (streaming path): `bodyEl.innerHTML = marked.parse(newText)` — server WebSocket frames taint `newText`; marked 12.x allows raw HTML passthrough; no sanitizer.
2. **Sink L1803** (non-streaming path): `const rendered = marked.parse(text)` used verbatim inside a template literal → `group.innerHTML`.

Additionally close a secondary injection path:
- **Code-block renderer L1469**: `${langLabel}` (from code-fence language identifier in markdown) injected unescaped into `<span>` inside the renderer string.

---

## §2 Architecture / Context

`webchat.html` is a single-file SPA served by `garraia-gateway`. It receives AI responses via WebSocket (`/ws`). Streamed chunks are accumulated in `data-raw` and re-rendered with `marked.parse()` on each chunk. All prior XSS fixes (GAR-510, GAR-511, GAR-512) handled *data-field* escaping (`escapeHtml()`), but the AI message rendering pipeline was left without a sanitizer — intentionally deferred to this plan.

The fix follows the industry-standard pattern for markdown-to-HTML pipelines: **marked → DOMPurify → innerHTML**.

---

## §3 Tech stack

- DOMPurify 3.1.7 (cdnjs, same CDN as marked.min.js and highlight.js already in-use)
- JavaScript (no build step; single-file HTML)
- No Rust changes

---

## §4 Design invariants

1. **No regression on code-block copy button**: DOMPurify strips `onclick`; fix uses event delegation on `chatMessages` instead. `window.copyCodeBlock(btn)` function is unchanged.
2. **Markdown rendering preserved**: DOMPurify default config allows `<b>`, `<i>`, `<a>`, `<code>`, `<pre>`, `<table>`, `<img src>`, etc. Only `<script>` and event-handler attributes are stripped.
3. **No CSP change required**: DOMPurify runs client-side without additional network requests.
4. **`escapeHtml` hoisted**: the code renderer uses `escapeHtml(langLabel)` — safe because `escapeHtml` is a `function` declaration (hoisted to file scope).

---

## §5 Out of scope

- DOMPurify configuration tightening (e.g., blocking `<img>`) — tracked separately under GAR-486 follow-up.
- admin.html markdown rendering (admin.html uses `textContent` for log lines, no markdown renderer).
- Playwright test for actual XSS injection (tracked under GAR-430 quality-gates wave).

---

## §6 Rollback

Pure HTML/JS change. Rollback = revert the commit on `webchat.html`. No schema, no migration, no binary dependency.

---

## §7 File structure (changes)

```
crates/garraia-gateway/src/webchat.html   ← 5 targeted edits (see M1 tasks)
plans/0063-gar-515-webchat-dompurify.md   ← this file
plans/README.md                           ← add row 0063
```

---

## §8 Tasks (M1)

- [x] T1 — Add DOMPurify 3.1.7 CDN `<script>` tag to `<head>` (after marked.min.js)
- [x] T2 — Escape `langLabel` in code-block renderer: `${escapeHtml(langLabel)}`
- [x] T3 — Remove `onclick="copyCodeBlock(this)"` from code-block button; keep class `copy-code-btn`
- [x] T4 — Add event delegation on `chatMessages` for `.copy-code-btn` click
- [x] T5 — Wrap streaming sink: `DOMPurify.sanitize(marked.parse(newText))`
- [x] T6 — Wrap non-streaming sink: `DOMPurify.sanitize(marked.parse(text))`
- [ ] T7 — Verify: `grep "marked\.parse" webchat.html` shows 0 unwrapped calls
- [ ] T8 — PR + CI green (17/17) + squash-merge
- [ ] T9 — Update plans/README.md row status to ✅ Merged
- [ ] T10 — Mark GAR-515 Done in Linear; note GAR-510 bookkeeping (still open in Linear, actually merged via PR #128 — close that too)

---

## §9 Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| DOMPurify strips `onclick` on copy button | Certain (by design) | T3+T4: event delegation replaces inline handler |
| DOMPurify strips legitimate HTML in AI response | Low (default config is permissive) | Markdown headings, links, bold, italic, tables all survive |
| CDN availability | Low | Same CDN (cdnjs) used for marked.min.js and highlight.js |

---

## §10 Acceptance criteria

1. No `marked.parse()` call in `webchat.html` without `DOMPurify.sanitize()` wrapper.
2. `escapeHtml(langLabel)` used in code-block renderer `<span>`.
3. No `onclick` attribute on code-block copy buttons.
4. Event delegation on `chatMessages` handles `.copy-code-btn` clicks.
5. CI 17/17 green.
6. CodeQL `Analyze (javascript-typescript)` re-run closes the L1707 and L1803 `js/xss` alerts.

---

## §11 Cross-references

- GAR-510 (admin.html XSS, PR #128) — companion fix
- GAR-511 (uploads_worker.rs + admin.html, PR #130) — companion fix
- GAR-512 (webchat.html data-field escaping, PR #133) — companion fix
- GAR-486 (Green Security Baseline umbrella) — parent issue
- docs/security/codeql-suppressions.md — suppression ledger (this fix does NOT need a suppression entry; it's a real fix)

---

## §12 Estimativa

~30 min (5 surgical edits to a single file + plan + PR bookkeeping).
