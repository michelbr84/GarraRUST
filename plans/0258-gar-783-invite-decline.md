# Plan 0258 — GAR-783: POST /v1/me/invites/{invite_id}/decline (invitee-side invite decline)

**Status:** Done — merged 2026-06-03 via PR #632 (`593206f`)
**Linear:** [GAR-783](https://linear.app/chatgpt25/issue/GAR-783)
**Branch:** `routine/202506031430-invite-decline`
**Date:** 2026-06-03 (America/New_York)

## Goal

Complete the invite lifecycle by giving the invitee an explicit decline action.
Currently, an invitee can:
- See their pending invites (`GET /v1/me/invites`, plan 0255 / GAR-777)
- Accept via token (`POST /v1/invites/{token}/accept`, plan 0019)

Missing: **decline** — an invitee who doesn't want to join a group must wait for
the invite to expire. This plan adds `POST /v1/me/invites/{invite_id}/decline`
so they can explicitly decline immediately and allow the inviter to re-invite
them with a different role or after reconsideration.

## Architecture

Same pattern as `revoke_invite` (plan 0257 / GAR-780) but from the invitee's
perspective:

- No `X-Group-Id` header required — `group_id` is resolved from the invite row.
- No `Action::MembersManage` capability required — any authenticated user can
  decline their own invite.
- The invitee identity is verified via
  `JOIN users ON users.email = group_invites.invited_email WHERE users.id = $caller`.
- Returns 204 No Content on success; 404 if invite not found, already
  accepted, already revoked, or already declined (no information leak).
- Migration 025 adds `declined_at` + `declined_by` columns and updates the
  partial unique index to also exclude declined rows (enabling re-invite after
  decline without a 409 conflict).

## Tech stack

- Rust + Axum 0.8 + sqlx (`garraia-gateway` crate)
- `garraia-auth::WorkspaceAuditAction::InviteDeclined` (new variant)
- Migration forward-only (additive nullable columns + index recreate)

## Design invariants

1. `declined_at IS NULL` guard in UPDATE prevents double-decline (idempotent
   404 on second call, no double audit event).
2. Audit PII-safe: metadata stores `propose_role` only, never `invited_email`.
3. `SET LOCAL app.current_user_id` AND `app.current_group_id` before any
   FORCE-RLS table write (`audit_events`).
4. `list_invites` (admin view) and `list_my_invites` (inbox) both now filter
   `declined_at IS NULL` in addition to existing `accepted_at IS NULL` and
   `revoked_at IS NULL` guards.
5. `get_invite` (admin single-read) treats declined invites as 404.

## Out of scope

- Decline reason / message field (future UX enhancement).
- Notification to inviter after decline (future channel integration).
- Batch decline of all pending invites.

## Rollback

All schema changes are additive (nullable columns). Rolling back the
migration is safe. Removing the code change leaves the DB columns unused
but harmless.

## File Structure

| File | Change |
|------|--------|
| `crates/garraia-workspace/migrations/025_group_invites_declined_at.sql` | New migration |
| `crates/garraia-auth/src/audit_workspace.rs` | `InviteDeclined` variant + `as_str` + test |
| `crates/garraia-gateway/src/rest_v1/groups.rs` | Add `AND declined_at IS NULL` to `list_invites` + `get_invite` |
| `crates/garraia-gateway/src/rest_v1/me.rs` | Add `AND declined_at IS NULL` + `AND revoked_at IS NULL` to `list_my_invites`; new `decline_invite` handler + tests |
| `crates/garraia-gateway/src/rest_v1/mod.rs` | Route in all 3 branches |
| `crates/garraia-gateway/src/rest_v1/openapi.rs` | Path + handler registration |
| `plans/README.md` | Row 0258 |
| `ROADMAP.md` | Checklist update |
| `TODO.md` | Session bookkeeping |

## M1 Tasks

- [x] T1: Migration 025 — `declined_at` + `declined_by` + updated unique index
- [x] T2: `InviteDeclined` variant in `audit_workspace.rs`
- [x] T3: Update `list_invites` + `get_invite` in `groups.rs`
- [x] T4: Update `list_my_invites` + add `decline_invite` handler in `me.rs`
- [x] T5: Route in `mod.rs` all 3 branches
- [x] T6: OpenAPI registration
- [x] T7: Unit tests (handler serialization, 404 shape, declined-filter)
- [x] T8: Docs — plan README row + ROADMAP checklist + TODO update

## Acceptance criteria

- `POST /v1/me/invites/{invite_id}/decline` returns 204 when the caller is
  the invitee and the invite is pending (accepted_at IS NULL, revoked_at IS
  NULL, declined_at IS NULL, not expired).
- Second decline of same invite returns 404 (no double audit event).
- `GET /v1/me/invites` no longer lists declined invites.
- `GET /v1/groups/{id}/invites` no longer lists declined invites.
- Re-invite after decline does not hit 409 (unique index updated).
- `cargo check -p garraia-gateway -p garraia-auth` clean.
- `cargo clippy --workspace --tests --exclude garraia-desktop --features garraia-gateway/test-helpers --no-deps -- -D warnings` clean.
- Unit tests green.

## Risk register

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Concurrent decline + accept race | Low | UPDATE WHERE guards both accepted_at IS NULL AND declined_at IS NULL |
| Declined invites still visible in admin list | Low | Both list_invites branches updated |
| Re-invite after decline blocked by unique index | Low | Index updated to exclude declined rows |

## Cross-references

- Plan 0019 — `POST /v1/invites/{token}/accept` (accept flow)
- Plan 0255 / GAR-777 — `GET /v1/me/invites` (inbox)
- Plan 0257 / GAR-780 — `DELETE /v1/groups/{id}/invites/{invite_id}` (admin revoke)

## Estimativa

~250 LOC across 7 files. ~30 min.
