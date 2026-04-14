//! GAR-392 — Pure RLS matrix.
//!
//! Exercises the three dedicated Postgres roles (`garraia_app`,
//! `garraia_login`, `garraia_signup`) against the 10 FORCE RLS tables
//! from migration 007 plus the tenant-root tables they each have grants
//! on. Each case is labeled with a stable `case_id`; failures are
//! collected into a single report at the end of the run instead of
//! aborting on the first mismatch.
//!
//! Plan 0013 path C — Task 8.
//! Design: docs/superpowers/specs/2026-04-14-gar-391d-392-authz-suite-design.md

#![cfg(feature = "test-support")]

mod common;

use common::cases::{DbRole, RlsCase, RlsExpected, SqlOp, TenantCtx};
use common::harness::Harness;
use common::oracle::{classify_count, classify_pg_error};
use common::tenants::Tenant;
use sqlx::PgPool;
use uuid::Uuid;

// The RLS_MATRIX const lives in `common::matrix` so both `rls_matrix.rs`
// (the runner) and `meta_tripwires.rs` (Task 9 counters) can import the
// same data. See plan 0013 path C Task 8/9.

use common::matrix::RLS_MATRIX;

// ─── Runner ────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn matrix_rls() -> anyhow::Result<()> {
    let h = Harness::get().await;
    let tenant = Tenant::new(&h).await?;
    seed_primary_group(&h, &tenant).await?;

    let mut failures: Vec<String> = Vec::new();

    for case in RLS_MATRIX {
        let outcome = execute_case(&h, &tenant, case).await;
        if outcome != case.expected {
            failures.push(format!(
                "[{}] role={} table={} op={:?} ctx={:?}\n  expected={:?}\n  got     ={:?}",
                case.case_id,
                case.db_role.as_str(),
                case.table,
                case.op,
                case.tenant_ctx,
                case.expected,
                outcome,
            ));
        }
    }

    if !failures.is_empty() {
        panic!(
            "rls matrix: {}/{} failures:\n  {}",
            failures.len(),
            RLS_MATRIX.len(),
            failures.join("\n  "),
        );
    }

    println!("rls matrix: {} cases green", RLS_MATRIX.len());
    Ok(())
}

// ─── Pre-seeding ───────────────────────────────────────────────────────────
//
// `seed_primary_group` inserts one representative row into each
// tenant-scoped table in the primary group via the superuser pool.
// This is a fixture, not an RLS test — it runs once, bypasses RLS, and
// is the only legitimate non-admin write to these tables during the run.

async fn seed_primary_group(h: &Harness, t: &Tenant) -> anyhow::Result<()> {
    let admin = PgPool::connect(&h.admin_url).await?;

    // chats: one channel in primary group.
    let chat_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO chats (id, group_id, type, name, created_by) \
         VALUES ($1, $2, 'channel', 'seed', $3)",
    )
    .bind(chat_id)
    .bind(t.group_id)
    .bind(t.owner.user_id)
    .execute(&admin)
    .await?;

    // chat_members: owner joins the seed chat.
    sqlx::query(
        "INSERT INTO chat_members (chat_id, user_id, role) \
         VALUES ($1, $2, 'owner')",
    )
    .bind(chat_id)
    .bind(t.owner.user_id)
    .execute(&admin)
    .await?;

    // messages: one message in the seed chat by member (so the
    // current_user_id = member.user_id also has policy relevance).
    sqlx::query(
        "INSERT INTO messages (chat_id, group_id, sender_user_id, sender_label, body) \
         VALUES ($1, $2, $3, 'Member', 'seed body')",
    )
    .bind(chat_id)
    .bind(t.group_id)
    .bind(t.member.user_id)
    .execute(&admin)
    .await?;

    // memory_items: group-scoped.
    sqlx::query(
        "INSERT INTO memory_items \
            (scope_type, scope_id, group_id, created_by, created_by_label, kind, content) \
         VALUES ('group', $1, $1, $2, 'Owner', 'note', 'seed note')",
    )
    .bind(t.group_id)
    .bind(t.owner.user_id)
    .execute(&admin)
    .await?;

    // audit_events: one event in primary group, actor = member (matches
    // the app.current_user_id GUC under `Correct` ctx).
    sqlx::query(
        "INSERT INTO audit_events \
            (group_id, actor_user_id, actor_label, action, resource_type) \
         VALUES ($1, $2, 'Member', 'seed.action', 'seed')",
    )
    .bind(t.group_id)
    .bind(t.member.user_id)
    .execute(&admin)
    .await?;

    // sessions: one session for the member. `expires_at` is NOT NULL —
    // set it to 1 hour in the future (irrelevant to RLS, just schema).
    sqlx::query(
        "INSERT INTO sessions (user_id, refresh_token_hash, expires_at) \
         VALUES ($1, $2, now() + interval '1 hour')",
    )
    .bind(t.member.user_id)
    .bind(format!("hash-{}", Uuid::now_v7()))
    .execute(&admin)
    .await?;

    admin.close().await;
    Ok(())
}

// ─── Executor ──────────────────────────────────────────────────────────────

async fn execute_case(h: &Harness, t: &Tenant, case: &RlsCase) -> RlsExpected {
    let pool: &PgPool = match case.db_role {
        DbRole::App => &h.app_pool,
        DbRole::Login => h.login_pool.raw(),
        DbRole::Signup => h.signup_pool.raw(),
    };

    let mut conn = match pool.acquire().await {
        Ok(c) => c,
        Err(e) => {
            panic!("[{}] acquire connection failed: {e}", case.case_id);
        }
    };

    // Set GUCs. `set_config('key', 'value', true)` means "set local to
    // the current transaction"; we wrap the op in an explicit
    // transaction so the GUCs take effect.
    sqlx::query("BEGIN").execute(&mut *conn).await.ok();

    match case.tenant_ctx {
        TenantCtx::Correct => {
            set_guc(&mut conn, "app.current_user_id", &t.member.user_id.to_string()).await;
            set_guc(&mut conn, "app.current_group_id", &t.group_id.to_string()).await;
        }
        TenantCtx::WrongGroupCorrectUser => {
            set_guc(&mut conn, "app.current_user_id", &t.member.user_id.to_string()).await;
            set_guc(&mut conn, "app.current_group_id", &Uuid::now_v7().to_string()).await;
        }
        TenantCtx::BothUnset => {
            // Intentionally nothing.
        }
        TenantCtx::CorrectRoleWrongTenant => {
            let other_user = Uuid::now_v7().to_string();
            let other_group = Uuid::now_v7().to_string();
            set_guc(&mut conn, "app.current_user_id", &other_user).await;
            set_guc(&mut conn, "app.current_group_id", &other_group).await;
        }
    }

    let outcome = match case.op {
        SqlOp::Select => execute_select(&mut conn, case.table).await,
        SqlOp::Insert => execute_insert(&mut conn, case, t).await,
        SqlOp::Update => unimplemented!("no UPDATE cases in the RLS matrix yet"),
        SqlOp::Delete => unimplemented!("no DELETE cases in the RLS matrix yet"),
    };

    // Roll back the transaction regardless — we never want side-effects
    // from matrix execution to contaminate the next case.
    sqlx::query("ROLLBACK").execute(&mut *conn).await.ok();

    outcome
}

async fn execute_select(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    table: &str,
) -> RlsExpected {
    let sql = format!("SELECT count(*) FROM {table}");
    match sqlx::query_scalar::<_, i64>(&sql).fetch_one(&mut **conn).await {
        Ok(n) => classify_count(n),
        Err(e) => classify_pg_error(&e).unwrap_or_else(|| {
            panic!("unclassified error on SELECT {table}: {e}");
        }),
    }
}

async fn execute_insert(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    case: &RlsCase,
    t: &Tenant,
) -> RlsExpected {
    let result = match case.table {
        "chats" => {
            sqlx::query(
                "INSERT INTO chats (group_id, type, name, created_by) \
                 VALUES ($1, 'channel', 'matrix-insert', $2)",
            )
            .bind(t.group_id)
            .bind(t.owner.user_id)
            .execute(&mut **conn)
            .await
        }
        "memory_items" => {
            sqlx::query(
                "INSERT INTO memory_items \
                    (scope_type, scope_id, group_id, created_by, created_by_label, kind, content) \
                 VALUES ('group', $1, $1, $2, 'Owner', 'note', 'matrix-insert')",
            )
            .bind(t.group_id)
            .bind(t.owner.user_id)
            .execute(&mut **conn)
            .await
        }
        "audit_events" => {
            sqlx::query(
                "INSERT INTO audit_events \
                    (group_id, actor_user_id, actor_label, action, resource_type) \
                 VALUES ($1, $2, 'Member', 'matrix.insert', 'seed')",
            )
            .bind(t.group_id)
            .bind(t.member.user_id)
            .execute(&mut **conn)
            .await
        }
        other => panic!("execute_insert: no template for table `{other}`"),
    };

    match result {
        Ok(qr) => classify_count(qr.rows_affected() as i64),
        Err(e) => classify_pg_error(&e).unwrap_or_else(|| {
            panic!(
                "[{}] unclassified error on INSERT {}: {e}",
                case.case_id, case.table
            );
        }),
    }
}

async fn set_guc(
    conn: &mut sqlx::pool::PoolConnection<sqlx::Postgres>,
    key: &str,
    val: &str,
) {
    sqlx::query("SELECT set_config($1, $2, true)")
        .bind(key)
        .bind(val)
        .execute(&mut **conn)
        .await
        .expect("set_config");
}
