//! Integration test for `audit_workspace_event` (plan 0347 / GAR-891 — Q6.15).
//!
//! # Mutant killed
//!
//! * `audit_workspace.rs:897:5` — `replace audit_workspace_event → Ok(())` (shard 0):
//!   killed by [`audit_workspace_event_inserts_row`].
//!
//! The production function executes one INSERT into `audit_events` inside the
//! caller's transaction. If the body were replaced with `Ok(())`, no row would
//! be inserted. This test commits the transaction, then queries `audit_events`
//! via the admin pool (bypassing RLS) and asserts exactly one row was created.
//!
//! Caller contract (from audit_workspace.rs:871-883):
//! * Transaction must be on a pool with `INSERT` grant on `audit_events`
//!   (`garraia_app` qualifies via migration 007:70).
//! * `SET LOCAL app.current_user_id` and `SET LOCAL app.current_group_id` must
//!   be set in the transaction (required by `audit_events_group_or_self` RLS policy).

mod common;

use common::harness::Harness;
use garraia_auth::{WorkspaceAuditAction, audit_workspace_event};
use serde_json::json;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn audit_workspace_event_inserts_row() -> anyhow::Result<()> {
    let h = Harness::get().await;
    let admin = sqlx::PgPool::connect(&h.admin_url).await?;

    // Seed a user and a group via the superuser pool (bypass auth + RLS for fixture setup).
    let actor_user_id = Uuid::now_v7();
    let group_id = Uuid::now_v7();
    sqlx::query("INSERT INTO users (id, email, display_name) VALUES ($1, $2, $3)")
        .bind(actor_user_id)
        .bind(format!("audit-actor-{}@garraia.test", actor_user_id))
        .bind("Audit Actor")
        .execute(&admin)
        .await?;
    sqlx::query("INSERT INTO groups (id, name, type, created_by) VALUES ($1, $2, 'team', $3)")
        .bind(group_id)
        .bind(format!("audit-group-{group_id}"))
        .bind(actor_user_id)
        .execute(&admin)
        .await?;

    // Begin a tx on app_pool (garraia_app) and satisfy the GUC contract.
    let mut tx = h.app_pool.begin().await?;
    sqlx::query("SET LOCAL app.current_user_id = $1")
        .bind(actor_user_id.to_string())
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL app.current_group_id = $1")
        .bind(group_id.to_string())
        .execute(&mut *tx)
        .await?;

    let resource_id = format!("{group_id}:{actor_user_id}");
    audit_workspace_event(
        &mut tx,
        WorkspaceAuditAction::MemberRemoved,
        actor_user_id,
        group_id,
        "group_members",
        resource_id.clone(),
        json!({ "target_user_id": actor_user_id, "old_role": "member" }),
    )
    .await?;
    tx.commit().await?;

    // Verify via admin pool (bypasses RLS; confirms the INSERT was not silenced).
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events \
         WHERE action = 'member.removed' \
           AND group_id = $1 \
           AND actor_user_id = $2 \
           AND resource_id = $3",
    )
    .bind(group_id)
    .bind(actor_user_id)
    .bind(&resource_id)
    .fetch_one(&admin)
    .await?;
    assert_eq!(
        count, 1,
        "audit_workspace_event must INSERT exactly 1 row — \
         fails if `replace with Ok(())` mutant (audit_workspace.rs:897) is active"
    );
    Ok(())
}
