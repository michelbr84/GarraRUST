//! Tenant fixture for the GAR-392 RLS matrix.
//!
//! The RLS matrix never exercises the login or signup code paths — it only
//! needs seed data that the `garraia_app` role can observe (or fail to
//! observe) through RLS policies. So this module bypasses `signup_user`
//! entirely and inserts `users`, `user_identities`, `groups`, and
//! `group_members` rows directly via the superuser pool.
//!
//! This is a deliberate deviation from plan 0013 Task 4 Steps 4.3-4.4
//! which prescribed `signup_user` + `login_user` + parallel `try_join!`.
//! The path C re-scope removed the need for JWTs (there is no HTTP
//! fixture consuming them), so the simpler direct-insert approach is
//! strictly better: deterministic, serial by construction, zero reliance
//! on the auth flow's internal locking, and measurably faster.
//!
//! Plan 0013 path C — Task 4.

use sqlx::PgPool;
use uuid::Uuid;

use super::harness::Harness;

pub struct Tenant {
    /// The "primary" group — owner + member + outsider cases all reference
    /// resources scoped to this group_id.
    pub group_id: Uuid,

    /// Owner of `group_id`. Cases that want "the subject can see/edit their
    /// own stuff" target this user.
    pub owner: TestUser,

    /// Plain member of `group_id`. Cases that want "another user in the
    /// same tenant" target this user. The RLS executor in Task 8 uses
    /// `member.user_id` as the `app.current_user_id` GUC under
    /// `TenantCtx::Correct`.
    pub member: TestUser,

    /// Authenticated user with no group membership at all. Used for cases
    /// that test "stranger cannot see anything" against RLS.
    pub outsider: TestUser,

    /// Owner of a *different* group. Used for `TenantCtx::WrongGroupCorrectUser`
    /// and `CorrectRoleWrongTenant` — a real user who legitimately belongs
    /// somewhere, just not here.
    pub cross_tenant: TestUser,

    /// The "other" group, owned by `cross_tenant`. Mostly referenced via
    /// its id when the executor picks a wrong-tenant GUC value.
    pub cross_group_id: Uuid,
}

pub struct TestUser {
    pub user_id: Uuid,
    pub email: String,
}

impl Tenant {
    /// Build a fresh tenant: 2 groups, 4 users, matching `group_members`
    /// rows, and one `user_identities` row per user (internal provider
    /// with a dummy hash — the RLS matrix cannot execute login so the
    /// hash value is irrelevant).
    ///
    /// Uses `harness.admin_url` (superuser) because `groups` /
    /// `group_members` / `users` / `user_identities` are tenant-root
    /// tables outside the `garraia_app` RLS surface for setup purposes,
    /// and because serial `INSERT`s via one connection are faster than
    /// parallelizing through the auth flow.
    pub async fn new(h: &Harness) -> anyhow::Result<Self> {
        let admin: PgPool = sqlx::PgPool::connect(&h.admin_url).await?;

        let owner_id = Uuid::now_v7();
        let member_id = Uuid::now_v7();
        let outsider_id = Uuid::now_v7();
        let cross_id = Uuid::now_v7();

        let owner_email = format!("rls-owner-{owner_id}@garraia.test");
        let member_email = format!("rls-member-{member_id}@garraia.test");
        let outsider_email = format!("rls-out-{outsider_id}@garraia.test");
        let cross_email = format!("rls-cross-{cross_id}@garraia.test");

        insert_user(&admin, owner_id, &owner_email, "Owner").await?;
        insert_user(&admin, member_id, &member_email, "Member").await?;
        insert_user(&admin, outsider_id, &outsider_email, "Outsider").await?;
        insert_user(&admin, cross_id, &cross_email, "Cross").await?;

        insert_identity(&admin, owner_id).await?;
        insert_identity(&admin, member_id).await?;
        insert_identity(&admin, outsider_id).await?;
        insert_identity(&admin, cross_id).await?;

        let group_id = Uuid::now_v7();
        let cross_group_id = Uuid::now_v7();

        insert_group(&admin, group_id, "test-group-primary", owner_id).await?;
        insert_group(&admin, cross_group_id, "test-group-cross", cross_id).await?;

        insert_member(&admin, group_id, owner_id, "owner").await?;
        insert_member(&admin, group_id, member_id, "member").await?;
        insert_member(&admin, cross_group_id, cross_id, "owner").await?;
        // `outsider` is intentionally NOT a member of any group.

        admin.close().await;

        Ok(Self {
            group_id,
            owner: TestUser {
                user_id: owner_id,
                email: owner_email,
            },
            member: TestUser {
                user_id: member_id,
                email: member_email,
            },
            outsider: TestUser {
                user_id: outsider_id,
                email: outsider_email,
            },
            cross_tenant: TestUser {
                user_id: cross_id,
                email: cross_email,
            },
            cross_group_id,
        })
    }
}

async fn insert_user(
    admin: &PgPool,
    id: Uuid,
    email: &str,
    display_name: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO users (id, email, display_name) \
         VALUES ($1, $2, $3)",
    )
    .bind(id)
    .bind(email)
    .bind(display_name)
    .execute(admin)
    .await?;
    Ok(())
}

async fn insert_identity(admin: &PgPool, user_id: Uuid) -> anyhow::Result<()> {
    // `provider_sub` must be unique across (provider, provider_sub); for the
    // internal provider, ADR 0003 says it equals `users.id::text`, so we mirror
    // that convention here.
    sqlx::query(
        "INSERT INTO user_identities (user_id, provider, provider_sub, password_hash) \
         VALUES ($1, 'internal', $2, '$argon2id$v=19$m=65536,t=3,p=4$dummy$dummy')",
    )
    .bind(user_id)
    .bind(user_id.to_string())
    .execute(admin)
    .await?;
    Ok(())
}

async fn insert_group(
    admin: &PgPool,
    id: Uuid,
    name: &str,
    created_by: Uuid,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO groups (id, name, type, created_by) \
         VALUES ($1, $2, 'team', $3)",
    )
    .bind(id)
    .bind(name)
    .bind(created_by)
    .execute(admin)
    .await?;
    Ok(())
}

async fn insert_member(
    admin: &PgPool,
    group_id: Uuid,
    user_id: Uuid,
    role: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO group_members (group_id, user_id, role) \
         VALUES ($1, $2, $3)",
    )
    .bind(group_id)
    .bind(user_id)
    .bind(role)
    .execute(admin)
    .await?;
    Ok(())
}
