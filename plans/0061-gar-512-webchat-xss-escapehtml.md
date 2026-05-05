# Plan 0061 — GAR-512: webchat.html XSS — fix 4 unescaped innerHTML sinks

**Status:** Em execução
**Autor:** Claude Sonnet 4.6 (health routine 2026-05-05, America/New_York)
**Data:** 2026-05-05 (America/New_York)
**Issue:** [GAR-512](https://linear.app/chatgpt25/issue/GAR-512)
**Branch:** `health/202605050652-gar-512-webchat-xss`
**CodeQL rule:** `js/xss` (high severity)

## §1 Goal

Close `js/xss` CodeQL alerts in `crates/garraia-gateway/src/webchat.html` by wrapping
all server-API-returned strings injected via `innerHTML` with the existing `escapeHtml()`
helper. Companion fix to GAR-510 (admin.html, PR #128, 2026-05-05).

## §2 Architecture

`webchat.html` is a single-file chat SPA served by `garraia-gateway`. It fetches data from
`/api/status`, `/api/mcp`, `/api/sessions`, and the WebSocket stream, then renders responses
via JavaScript DOM manipulation. The `escapeHtml()` and `escapeAttr()` helpers are already
defined (lines 1841–1849) and correctly applied to user-message bodies, system messages,
error messages, project names, file names, and skill names. However, four blocks that
render server-API metadata (API status string, channel names, MCP server names/transports,
and session channel badges) are missing the wrapper.

## §3 Tech stack

Pure client-side JavaScript in a single HTML file. No build step, no npm. The fix uses
only the `escapeHtml()` DOM-trick already present in the same file.

## §4 Design invariants

- `escapeHtml()` only wraps string values going into `innerHTML`. Template literals that
  contain only hardcoded markup (no user/API data) are left as-is.
- `marked.parse()` calls (AI response markdown rendering, lines 1707 and 1803) are
  intentional by-design and are NOT changed in this plan — they require a DOMPurify
  integration decision (tracked separately).
- Zero logic changes; zero Rust changes; zero schema changes.

## §5 Scope

**In scope:**
- 5 specific innerHTML injection sites listed in §7.

**Out of scope:**
- `marked.parse()` XSS (DOMPurify integration — separate plan needed).
- Refactoring other HTML files or template rendering.
- Any Rust / database changes.

## §6 Affected files

```
crates/garraia-gateway/src/webchat.html   (5 targeted line changes)
plans/README.md                            (add row 0061)
```

## §7 Vulnerable sinks → fix

| Line (pre-fix) | Sink | Source | Fix |
|---|---|---|---|
| 1888 | `'...' + j.status` | `/api/status` → `j.status` | `escapeHtml(j.status)` |
| 1892 | `'...' + ch + '...'` | `/api/status` → `j.channels[i]` | `escapeHtml(ch)` |
| 1967 | `${s.name}`, `${s.transport \|\| ''}` | `/api/mcp` → `s.name`, `s.transport` | `${escapeHtml(s.name)}`, `${escapeHtml(s.transport \|\| '')}` |
| 2123 | `${sessionId.slice(0, 16)}` | WebSocket / server-assigned ID | `${escapeHtml(sessionId.slice(0, 16))}` |
| 2653 | `channel` in `channelBadge` template | `/api/sessions` → `s.channel_id` | `escapeHtml(channel)` |

## §8 Rollback plan

Revert the single commit on `webchat.html`. No data migration required.
The change is purely additive (wrapping calls); reverting is a one-line
git revert. No service restart needed (static file hot-reload).

## §9 Risk register

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Visual regression if a valid channel name contains `&` or `<` | Low | Low | `escapeHtml` uses DOM `textContent` trick — correct HTML entity encoding |
| CodeQL re-scan delay (alerts stay open 15-30 min after merge) | Certain | Cosmetic | Not actionable; alerts auto-close on next CodeQL run |

## §10 Acceptance criteria

- `grep -n "innerHTML\s*=" crates/garraia-gateway/src/webchat.html` shows no
  API-data concatenation without `escapeHtml()`
- `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` passes
- All CI checks pass (Format, Clippy, Tests, Build, MSRV, cargo-deny, Security Audit,
  Coverage, Analyze rust, Analyze js-ts, Playwright, E2E, Secret Scan, Dependency Review)

## §11 Cross-references

- GAR-510 — companion fix for `admin.html` (merged PR #128)
- GAR-511 — companion fix for `uploads_worker.rs` SQL injection (merged PR #130)
- GAR-486 — CodeQL umbrella
- CodeQL rule: `js/xss`

## §12 Open questions

None — fix is structurally identical to GAR-510.

## §13 Estimativa

T1: Implement 5 escapeHtml wraps in webchat.html — 15 min
T2: Update plans/README.md — 5 min
Total: ~20 min

## M1 Tasks

- [x] T1: Fix 5 innerHTML sinks in `webchat.html`
- [x] T2: Update `plans/README.md` with row 0061
- [x] T3: CI green + merge
