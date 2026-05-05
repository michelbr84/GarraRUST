# Plan 0057 — GAR-510: admin.html XSS — add escapeHtml, fix 3 unescaped innerHTML sinks

**Status:** Em execução
**Autor:** Claude Sonnet 4.6 (health routine 2026-05-05, America/New_York)
**Data:** 2026-05-05 (America/New_York)
**Issue:** [GAR-510](https://linear.app/chatgpt25/issue/GAR-510)
**Branch:** `health/202605050444-admin-xss`
**CodeQL rule:** `js/xss` (high severity)

## §1 Goal

Close the `js/xss` CodeQL alerts in `crates/garraia-gateway/src/admin.html` by:
1. Adding an `escapeHtml` helper function (same DOM-trick already in `webchat.html:1841`).
2. Wrapping all three API-data sinks that use raw `innerHTML` string concatenation.

## §2 Architecture

`admin.html` is a single-file admin SPA served by `garraia-gateway`. It fetches data from
the admin REST API (channels, sessions, logs, metrics) and renders it via JavaScript DOM
manipulation. The three vulnerable sinks inject data returned from those API calls directly
into `innerHTML` without HTML-escaping, enabling stored XSS if an attacker can write a
malicious channel name or cause the server to log HTML-tagged content.

## §3 Tech stack

Pure client-side JavaScript in a single HTML file. No build step, no npm. The same
`escapeHtml` DOM-trick is already present and tested in `webchat.html` — we copy the
pattern verbatim.

## §4 Design invariants

1. **No logic change**: only the rendering layer changes; all data fetching, routing, and API
   calls are untouched.
2. **escapeHtml reuse**: identical function body to `webchat.html:1841` — creates a temporary
   `<div>`, sets `textContent`, returns `innerHTML` (browser encodes `&`, `<`, `>`, `"`, `'`).
3. **Log viewer**: the timestamp portion (`tsMatch[0]`, digits + `-:T` only) is safe; only
   `line.slice(tsMatch[0].length)` needs escaping.
4. **No regression**: `ch.enabled`, `ch.connected` boolean branches produce only static
   badge class names — not user-controlled, no change needed there.

## §5 Out of scope

- Rewriting all 10 `innerHTML` uses in admin.html to `createElement` (GAR-83 full scope —
  too large for a health-routine slice; deferred).
- Fixing `webchat.html` (already has `escapeHtml` applied correctly at all user-data sinks).
- Any Rust, schema, or migration change.

## §6 Rollback

`git revert <merge-sha>` — pure HTML/JS change, no server-side impact.

## §7 Risk register

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|-----------|
| escapeHtml double-encodes data already HTML-encoded server-side | Very low | Low | Admin API returns raw text (channel names, log lines); no pre-encoding observed |
| Missed sink introduced after this PR | Low | Medium | CodeQL re-scan catches future regressions |

## §8 Rollback plan

Revert commit. Zero server-side dependency.

## §9 File structure (changes)

```
crates/garraia-gateway/src/admin.html
  — Add escapeHtml function (5 lines) after the existing `el()` helper
  — Line 794: wrap ch.name + ch.type with escapeHtml()
  — Line 1185: same
  — Line 1186-1187: badge text is static ('Enabled'/'Disabled' etc.) — safe, no change
  — Line 1298: wrap line.slice(...) with escapeHtml()
plans/
  0057-gar-510-admin-xss-escapehtml.md   ← this file
  README.md                               ← +1 row
```

## §10 M1 tasks

- [x] M1: Write plan 0057 + create GAR-510 + branch health/202605050444-admin-xss
- [ ] M2: Add `escapeHtml` function to admin.html after `el()` helper
- [ ] M3: Fix line 794 — dashboard channel row
- [ ] M4: Fix line 1185 — channels page row
- [ ] M5: Fix line 1298 — log viewer
- [ ] M6: Verify no remaining user-data innerHTML sinks
- [ ] M7: Update plans/README.md row 0057
- [ ] M8: Commit + push + open PR
- [ ] M9: CI green (≥16 checks)
- [ ] M10: Squash-merge + mark GAR-510 Done

## §11 Acceptance criteria

1. `grep -n "ch\.name\|ch\.type\|line\.slice" crates/garraia-gateway/src/admin.html` shows
   all occurrences wrapped in `escapeHtml(...)`.
2. `grep -n "function escapeHtml" crates/garraia-gateway/src/admin.html` returns 1 result.
3. CI ≥16 checks all green on PR head commit.
4. CodeQL `js/xss` alerts for `admin.html` auto-close after next scan.

## §12 Open questions

None.

## §13 Cross-references

- GAR-83 — "ADM-4 Hardening de render (no innerHTML)" — cancelled 2026-02-27; this is the
  narrow security-critical slice of that broader task.
- GAR-490/491 — previous CodeQL wave triage (path-injection + hard-coded crypto).
- GAR-508 / plan 0056 — previous health routine (sql-injection, merged 2026-05-05).
- `docs/security/codeql-suppressions.md` — suppression ledger (not touched; real fix avoids dismissal).

## §14 Estimativa

~30 min — 3 surgical string replacements + 1 function addition.
