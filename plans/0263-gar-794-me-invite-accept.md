# Plan 0263 — GAR-794: POST /v1/me/invites/{invite_id}/accept (invitee-side authenticated invite acceptance)

**Status:** In Progress
**Linear:** [GAR-794](https://linear.app/chatgpt25/issue/GAR-794)
**Branch:** `routine/202606050615-me-invite-accept`
**Date:** 2026-06-05 (America/New_York)

## Goal

Complete the invite inbox UX by adding the **accept** action alongside the existing
`decline` (plan 0258 / GAR-783).

Currently, a logged-in user can:
- See pending invites (`GET /v1/me/invites`, plan 0255 / GAR-777)
- Decline an invite (`POST /v1/me/invites/{id}/decline`, plan 0258 / GAR-783)
- Accept via raw token (`POST /v1/invites/{token}/accept`, plan 0019)

Missing: **UUID-based authenticated accept** — a user viewing their invites inbox
in the UI has no way to accept without the raw plaintext token (which is only
safe to expose in email links, not in API responses). This plan closes that gap.

## Architecture

Same pattern as `decline_invite` (plan 0258 / GAR-783):

- No `X-Group-Id` header required — `group_id` is resolved from the invite row.
- No `Action::MembersManage` capability — any authenticated user can accept their own invite.
- Invitee identity verified via `JOIN users ON users.email = group_invites.invited_email WHERE users.id = $caller`.
- Returns 200 with `AcceptMyInviteResponse { group_id, role, invite_id }`.
- Returns 404 if invite not found, belongs to another user, or is terminal (accepted/revoked/declined).
- Returns 410 if invite is expired.
- Returns 409 if caller is already a member of the group.

### Atomicity

A single `BEGIN`/`COMMIT` transaction:
1. `SET LOCAL app.current_user_id` (FORCE-RLS for audit_events)
2. `UPDATE group_invites SET accepted_at = now(), accepted_by = u.id … RETURNING group_id, proposed_role, invited_by`
3. `INSERT INTO group_members (group_id, user_id, role, status, joined_at, invited_by)`
   — with `ON CONFLICT DO NOTHING`, checking rows_affected to detect already-member (→ 409)
4. `SET LOCAL app.current_group_id` (required for audit INSERT)
5. `audit_workspace_event(InviteAccepted, …)`
6. `COMMIT`

The UPDATE WHERE clause guards every terminal state:
```sql
accepted_at IS NULL AND revoked_at IS NULL AND declined_at IS NULL AND expires_at >= now()
```
Expired invites return NULL from `fetch_optional` → treated as 404, then the caller
checks the separate expiry query to distinguish 404 vs 410.

**Simpler approach used**: Since the UPDATE embeds all guards, a NULL result means
either "not found/terminal" OR "expired". We distinguish by doing a follow-up read
only when NULL, checking if the invite row exists at all — if it exists but
`expires_at < now()`, return 410; otherwise 404. This avoids a RETURNING that
leaks state via timing.

## Tech stack

- `crates/garraia-gateway/src/rest_v1/me.rs` — handler `accept_invite`
- `crates/garraia-gateway/src/rest_v1/mod.rs` — route registration (3 branches)
- `crates/garraia-gateway/src/openapi.rs` — schema registration
- No migration — `accepted_at`/`accepted_by` columns exist since migration 001.
- `WorkspaceAuditAction::InviteAccepted` already defined in `garraia_auth::audit_workspace`.

## Design invariants

- **No PII in audit metadata** — carry `proposed_role` only; no `invited_email`.
- **FORCE-RLS compliance** — SET LOCAL `app.current_user_id` + `app.current_group_id` before any RLS table DML.
- **No raw token in response** — UUID-only, never exposes `token_hash`.
- **Idempotent guard** — double-accept attempt returns 404 (first accept set `accepted_at IS NOT NULL`).
- **Cross-group isolation** — invitee email match is the auth boundary; no group_id parameter accepted from caller.

## Out of scope

- `POST /v1/invites/{token}/accept` (token-based) — already exists (plan 0019).
- Sending notifications/emails on acceptance — future slice.
- Removing the group_invites row — soft-accept only (row kept for audit trail).

## Rollback

Revert `me.rs` hunk + remove route from `mod.rs` + remove schema from `openapi.rs`. No migration to revert.

## Task list

- [x] T1 — Write handler `accept_invite` in `me.rs` with error matrix
- [x] T2 — Register route in all 3 `mod.rs` branches
- [x] T3 — Register `AcceptMyInviteResponse` in `openapi.rs`
- [x] T4 — Unit tests (≥ 6): serialization, happy-path shape, expired guard, already-member guard, terminal-invite guard, no-PII-in-response
- [x] T5 — Update ROADMAP.md + plans/README.md + TODO.md
- [x] T6 — Commit, push, open PR, wait for CI green, squash-merge

## Acceptance criteria

- `POST /v1/me/invites/{id}/accept` → 200 `{group_id, role, invite_id}` on happy path.
- → 404 when not found / not caller's / terminal.
- → 410 when expired.
- → 409 when caller already a group member.
- `InviteAccepted` audit event with `proposed_role` metadata, no email PII.
- `group_members` row inserted with correct `role`, `status = 'active'`.
- `GET /v1/me/invites` excludes the accepted invite afterward.
- CI ≥ 16 checks green.

## Cross-references

- GAR-783 (decline) — plan 0258
- GAR-777 (invites inbox) — plan 0255
- GAR-780 (revoke) — plan 0257
- Plan 0019 — token-based accept (existing)

## Estimativa

~200 LOC, 1-2 hours.
