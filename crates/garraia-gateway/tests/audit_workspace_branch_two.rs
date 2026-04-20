//! Plan 0022 T7 (GAR-426) — regression guard for the policy branch 2
//! of `audit_events_group_or_self`.
//!
//! The policy (migration 007:161, tightened with `WITH CHECK` in
//! migration 013) has two branches:
//!
//! ```sql
//! (group_id IS NOT NULL
//!  AND group_id = current_setting('app.current_group_id')::uuid)
//! OR
//! (group_id IS NULL
//!  AND actor_user_id = current_setting('app.current_user_id')::uuid)
//! ```
//!
//! Branch 1 (group audit) is exercised by plan 0021's `accept_invite`,
//! `set_member_role`, `delete_member` integration tests — all three
//! set `app.current_group_id` before calling `audit_workspace_event`
//! (plan 0021 T3/T4/T5).
//!
//! Branch 2 (user audit, group_id IS NULL) is **unreachable by the
//! current workspace audit helpers** because `audit_workspace_event`
//! signature requires `group_id: Uuid` (non-optional). But the policy
//! branch is still load-bearing for any future user-scoped audit
//! (e.g. global self-export events). Without `app.current_user_id`
//! set, the WITH CHECK must reject the INSERT with SQLSTATE 42501
//! (RLS policy violation on row-level security).
//!
//! This test pokes the policy directly by opening a tx on the
//! `garraia_app` pool, deliberately NOT setting `app.current_user_id`,
//! and attempting a raw `INSERT INTO audit_events` with
//! `group_id = NULL`. The INSERT must fail.
//!
//! This is a REGRESSION GUARD — if a future migration accidentally
//! relaxes the policy (for example by changing `WITH CHECK` to `TRUE`
//! under a misguided "permissive" refactor), this test catches it.
//! The test lives outside `audit_workspace_event` by design: exercising
//! the helper would require wrapping its `group_id: Uuid` signature
//! (or adding an Option<Uuid> variant) which is out-of-scope for plan
//! 0022. Raw INSERT is the minimal viable regression guard.

mod common;

use common::Harness;
use uuid::Uuid;

#[tokio::test]
async fn audit_events_insert_without_current_user_id_is_rejected_by_rls() {
    let h = Harness::get().await;
    let pool = h.app_pool.pool_for_handlers();

    // Open a tx on the `garraia_app` pool and DELIBERATELY skip
    // `SET LOCAL app.current_user_id`. The policy branch 2
    // (`group_id IS NULL ...`) then has no matching user_id and
    // the WITH CHECK (migration 013) rejects the INSERT.
    let mut tx = pool.begin().await.expect("F-05: begin tx on app_pool");

    let actor = Uuid::new_v4();

    // Raw INSERT with group_id = NULL → exercises policy branch 2.
    // Any `actor_user_id` value works here because the predicate
    // compares against `current_setting('app.current_user_id')`,
    // which is empty → NULLIF returns NULL → comparison is NULL →
    // the OR branch 2 evaluates to NULL → treated as false → the
    // WITH CHECK denies the INSERT.
    let result = sqlx::query(
        "INSERT INTO audit_events \
             (group_id, actor_user_id, action, resource_type, resource_id, metadata) \
         VALUES (NULL, $1, 'test.branch_two_guard', 'test_resource', $2, '{}'::jsonb)",
    )
    .bind(actor)
    .bind(actor.to_string())
    .execute(&mut *tx)
    .await;

    // The expected failure mode is `sqlx::Error::Database` with
    // SQLSTATE 42501 (`insufficient_privilege` — how Postgres
    // reports RLS WITH CHECK rejections).
    match result {
        Err(sqlx::Error::Database(db_err)) if db_err.code().as_deref() == Some("42501") => {
            // Expected — WITH CHECK denies the INSERT because no
            // branch of the policy matches.
        }
        Err(other) => panic!(
            "F-05: expected SQLSTATE 42501 (RLS WITH CHECK rejection), got a different error: {other:?}"
        ),
        Ok(_) => panic!(
            "F-05 REGRESSION: INSERT into audit_events with group_id=NULL and NO \
             app.current_user_id set should be rejected by the RLS policy \
             (migration 013 added explicit WITH CHECK). Accepting this INSERT \
             means the policy was accidentally relaxed — investigate recent \
             changes to migration 007/013 or the RLS setup in garraia-workspace."
        ),
    }

    // Tx is dropped without commit regardless.
}
