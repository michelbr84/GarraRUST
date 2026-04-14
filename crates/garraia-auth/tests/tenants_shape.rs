//! Task 4 smoke test — a fresh `Tenant` produces 4 distinct users, 2 distinct
//! groups, and the expected `group_members` rows, using only direct inserts
//! via the superuser pool (no auth flow invoked).
//!
//! Plan 0013 path C — Task 4.

#![cfg(feature = "test-support")]

mod common;

use common::harness::Harness;
use common::tenants::Tenant;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tenant_new_creates_two_groups_and_four_users() -> anyhow::Result<()> {
    let h = Harness::get().await;
    let t = Tenant::new(&h).await?;

    // Four distinct user_ids.
    let ids = [
        t.owner.user_id,
        t.member.user_id,
        t.outsider.user_id,
        t.cross_tenant.user_id,
    ];
    for i in 0..ids.len() {
        for j in (i + 1)..ids.len() {
            assert_ne!(ids[i], ids[j], "user ids collided at {i}/{j}");
        }
    }
    assert_ne!(t.group_id, t.cross_group_id);

    // Fixture verification must bypass RLS (user_identities and group_members
    // are RLS-enforced, garraia_app without GUCs sees 0 rows). Use a fresh
    // superuser pool via the admin URL for read-back assertions — this is
    // legitimate for fixture sanity checks, not for the RLS matrix itself.
    let admin = sqlx::PgPool::connect(&h.admin_url).await?;

    let user_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM users WHERE id = ANY($1::uuid[])",
    )
    .bind(&ids[..])
    .fetch_one(&admin)
    .await?;
    assert_eq!(user_count, 4);

    let identity_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM user_identities WHERE user_id = ANY($1::uuid[])",
    )
    .bind(&ids[..])
    .fetch_one(&admin)
    .await?;
    assert_eq!(identity_count, 4);

    let primary_members: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM group_members WHERE group_id = $1",
    )
    .bind(t.group_id)
    .fetch_one(&admin)
    .await?;
    assert_eq!(primary_members, 2, "primary group must have owner + member");

    let cross_members: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM group_members WHERE group_id = $1",
    )
    .bind(t.cross_group_id)
    .fetch_one(&admin)
    .await?;
    assert_eq!(cross_members, 1, "cross group must have only cross owner");

    let outsider_memberships: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM group_members WHERE user_id = $1",
    )
    .bind(t.outsider.user_id)
    .fetch_one(&admin)
    .await?;
    assert_eq!(outsider_memberships, 0, "outsider must have no memberships");

    admin.close().await;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_tenants_are_isolated() -> anyhow::Result<()> {
    let h = Harness::get().await;
    let t1 = Tenant::new(&h).await?;
    let t2 = Tenant::new(&h).await?;

    assert_ne!(t1.group_id, t2.group_id);
    assert_ne!(t1.owner.user_id, t2.owner.user_id);
    assert_ne!(t1.member.user_id, t2.member.user_id);

    Ok(())
}
