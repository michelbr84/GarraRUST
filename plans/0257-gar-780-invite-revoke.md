# Plan 0257 — GAR-780: GET + DELETE /v1/groups/{id}/invites/{invite_id} (invite revocation)

**Status:** Merged — PR #625 (`46a8658`) 2026-06-03
**Linear:** [GAR-780](https://linear.app/chatgpt25/issue/GAR-780)
**Branch:** `routine/202506021830-invite-revoke`
**Date:** 2026-06-02 (America/New_York)

## Context

`POST /v1/groups/{id}/invites` (plan 0016/0018) creates pending invites.
`GET /v1/groups/{id}/invites` (plan 0097) lists pending invites.
`POST /v1/invites/{token}/accept` (plan 0019) accepts an invite.

Missing: the ability to READ a single invite by ID or REVOKE it before it is
accepted. This plan adds:

- `GET /v1/groups/{id}/invites/{invite_id}` — single pending invite detail
- `DELETE /v1/groups/{id}/invites/{invite_id}` — soft-revoke a pending invite

Both require `Action::MembersManage` (owner/admin only — `invited_email` is PII).

## Schema change — Migration 024

`group_invites` gains two nullable columns:

| Column | Type | Notes |
|--------|------|-------|
| `revoked_at` | `timestamptz` | NULL = not revoked |
| `revoked_by` | `uuid REFERENCES users(id)` | NULL = not revoked |

The partial unique index `group_invites_pending_unique` (migration 011) is
dropped and recreated with predicate `WHERE accepted_at IS NULL AND revoked_at IS NULL`
so re-invite is possible after revocation without a 409 conflict.

## Audit event — `WorkspaceAuditAction::InviteRevoked`

New variant in `garraia-auth::audit_workspace.rs`. String: `"invite.revoked"`.

Metadata: `{ proposed_role }` — never `invited_email` (PII).

## Handler design

### `GET /v1/groups/{id}/invites/{invite_id}`

1. Path/header coherence.
2. `can(&principal, Action::MembersManage)` → 403 if not.
3. Open tx + `SET LOCAL app.current_user_id`.
4. `SELECT ... FROM group_invites WHERE id = $1 AND group_id = $2 AND accepted_at IS NULL AND revoked_at IS NULL`.
5. 404 if no row.
6. Commit, return `InviteSummary` (200).

### `DELETE /v1/groups/{id}/invites/{invite_id}`

1. Path/header coherence.
2. `can(&principal, Action::MembersManage)` → 403 if not.
3. Open tx + `SET LOCAL app.current_user_id` AND `SET LOCAL app.current_group_id` (audit_events FORCE-RLS).
4. `UPDATE group_invites SET revoked_at = now(), revoked_by = $caller WHERE id = $2 AND group_id = $3 AND accepted_at IS NULL AND revoked_at IS NULL RETURNING proposed_role`.
5. `rows_affected == 0` → 404.
6. Emit `InviteRevoked` audit event.
7. Commit, return 204.

## Idempotency

Already-revoked or already-accepted invites return 404 (not 409).
Re-invite after revocation is now possible (unique index updated).

## Files changed

| File | Change |
|------|--------|
| `crates/garraia-workspace/migrations/024_group_invites_revoked_at.sql` | New migration |
| `crates/garraia-auth/src/audit_workspace.rs` | `InviteRevoked` variant + `as_str` + test |
| `crates/garraia-gateway/src/rest_v1/groups.rs` | `get_invite` + `revoke_invite` + updated `list_invites` WHERE + 5 unit tests |
| `crates/garraia-gateway/src/rest_v1/mod.rs` | Route in all 3 branches |
| `crates/garraia-gateway/src/rest_v1/openapi.rs` | Paths + schemas |
| `plans/README.md` | Row 0257 |
| `ROADMAP.md` | Checklist update |
