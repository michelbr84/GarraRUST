//! Postgres scenarios (B1-B7) against `pgvector/pgvector:pg16`.
//!
//! Implemented for GAR-373 wave 1a. See plans/0002-gar-373-adr-postgres-decision.md §7.

use crate::shared::{
    fake_embedding, fake_message, host_spec, seeded_rng, BenchmarkRun, ScenarioResult,
    ScenarioStatus, EMBEDDING_DIM, NUM_EMBEDDINGS, NUM_GROUPS, NUM_MESSAGES,
};
use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use hdrhistogram::Histogram;
use pgvector::Vector;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::time::Instant;
use testcontainers::runners::AsyncRunner;
use testcontainers::ImageExt;
use testcontainers_modules::postgres::Postgres as PgImage;
use uuid::Uuid;

const RARE_TOKEN: &str = "QUUX-RARE-TOKEN";
const COMMON_TERM: &str = "brasil"; // we inject this into ~15% of messages
const RARE_MESSAGES: usize = 10;
const COMMON_INJECT_EVERY: usize = 7; // ~14% of messages get the common term

const SCHEMA_SQL: &str = r#"
CREATE EXTENSION IF NOT EXISTS vector;
CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE TABLE groups (
    id uuid PRIMARY KEY,
    name text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE messages (
    id uuid PRIMARY KEY,
    group_id uuid NOT NULL REFERENCES groups(id),
    body text NOT NULL,
    body_tsv tsvector GENERATED ALWAYS AS (to_tsvector('portuguese', body)) STORED,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX messages_group_id_idx ON messages(group_id);
CREATE INDEX messages_tsv_idx ON messages USING GIN (body_tsv);

CREATE TABLE memory_embeddings (
    id uuid PRIMARY KEY,
    group_id uuid NOT NULL REFERENCES groups(id),
    content text NOT NULL,
    embedding vector(768) NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX memory_embeddings_hnsw_idx
    ON memory_embeddings USING hnsw (embedding vector_cosine_ops);

ALTER TABLE messages ENABLE ROW LEVEL SECURITY;
-- FORCE ROW LEVEL SECURITY is required so the table OWNER is also subject to
-- the policy. Without FORCE, a superuser or the role that created the table
-- bypasses RLS entirely — which would silently disable isolation in any
-- deployment where the application connection pool uses the owner role.
-- See ADR 0003 §"Schema isolation pattern" for the full rationale.
ALTER TABLE messages FORCE ROW LEVEL SECURITY;
CREATE POLICY messages_group_isolation ON messages
    USING (group_id = current_setting('app.current_group_id', true)::uuid);
"#;

fn new_hist() -> Histogram<u64> {
    Histogram::<u64>::new_with_bounds(1, 60_000_000, 3).expect("histogram bounds")
}

fn median(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = xs.len();
    if n == 0 {
        0.0
    } else if n % 2 == 1 {
        xs[n / 2]
    } else {
        (xs[n / 2 - 1] + xs[n / 2]) / 2.0
    }
}

fn us_to_ms(us: u64) -> f64 {
    (us as f64) / 1000.0
}

pub async fn run_all(iterations: u32) -> Result<BenchmarkRun> {
    tracing::info!("starting pgvector/pgvector:pg16 container");
    let container = PgImage::default()
        .with_name("pgvector/pgvector")
        .with_tag("pg16")
        .start()
        .await
        .context("failed to start pgvector container")?;

    let host = container.get_host().await?;
    let port = container.get_host_port_ipv4(5432).await?;
    let url = format!("postgres://postgres:postgres@{host}:{port}/postgres");
    tracing::info!(%url, "container up");

    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(&url)
        .await
        .context("connect pool")?;

    // Schema
    for stmt in SCHEMA_SQL.split(";\n") {
        let s = stmt.trim();
        if s.is_empty() {
            continue;
        }
        sqlx::query(s).execute(&pool).await.with_context(|| format!("DDL: {s}"))?;
    }
    tracing::info!("schema created");

    // Seed data
    let seed_ctx = seed(&pool).await.context("seed")?;
    tracing::info!(
        groups = seed_ctx.group_ids.len(),
        messages = NUM_MESSAGES,
        embeddings = NUM_EMBEDDINGS,
        "seed complete"
    );

    let mut scenarios = Vec::new();

    scenarios.push(run_or_fail("B1", "Insert throughput (100k msgs batched)", iterations, run_b1(&pool, iterations).await).await);
    scenarios.push(run_or_fail("B2", "FTS common term", iterations, run_b2(&pool, iterations).await).await);
    scenarios.push(run_or_fail("B3", "FTS rare term", iterations, run_b3(&pool, iterations).await).await);
    scenarios.push(run_or_fail("B4", "ANN top-5", iterations, run_b4(&pool, iterations).await).await);
    scenarios.push(run_or_fail("B5", "Hybrid query (FTS+ANN+group)", iterations, run_b5(&pool, &seed_ctx, iterations).await).await);
    scenarios.push(run_or_fail("B6", "RLS cross-group isolation", 1, run_b6(&pool, &seed_ctx).await).await);
    scenarios.push(run_or_fail("B7", "Pool stress (100 concurrent)", 1, run_b7(&pool, &seed_ctx).await).await);

    drop(pool);
    drop(container);

    Ok(BenchmarkRun {
        run_at: Utc::now(),
        host: host_spec(),
        scenarios,
    })
}

/// Wrap each scenario so errors become Failed results rather than aborting the run.
async fn run_or_fail(
    id: &str,
    name: &str,
    iterations: u32,
    result: Result<ScenarioResult>,
) -> ScenarioResult {
    match result {
        Ok(mut r) => {
            if r.id.is_empty() {
                r.id = id.to_string();
            }
            if r.name.is_empty() {
                r.name = name.to_string();
            }
            r
        }
        Err(e) => {
            tracing::error!(scenario = id, error = %e, "scenario failed");
            ScenarioResult {
                id: id.to_string(),
                name: name.to_string(),
                backend: "postgres".to_string(),
                status: ScenarioStatus::Failed,
                p50_ms: None,
                p95_ms: None,
                p99_ms: None,
                throughput: None,
                iterations,
                notes: format!("error: {e:#}"),
            }
        }
    }
}

struct SeedContext {
    group_ids: Vec<Uuid>,
    /// One message uuid known to belong to group_ids[1], used for B6 cross-group leak test.
    group_b_message_id: Uuid,
}

async fn seed(pool: &PgPool) -> Result<SeedContext> {
    let mut rng = seeded_rng();

    // Groups
    let mut group_ids: Vec<Uuid> = Vec::with_capacity(NUM_GROUPS);
    for _ in 0..NUM_GROUPS {
        group_ids.push(Uuid::new_v4());
    }

    // Insert groups via multi-row INSERT chunks
    for chunk in group_ids.chunks(500) {
        let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new("INSERT INTO groups (id, name) ");
        qb.push_values(chunk.iter().enumerate(), |mut b, (i, id)| {
            b.push_bind(*id).push_bind(format!("group-{i}"));
        });
        qb.build().execute(pool).await?;
    }

    // Messages
    let mut group_b_message_id = Uuid::nil();
    let mut batch: Vec<(Uuid, Uuid, String)> = Vec::with_capacity(500);
    for i in 0..NUM_MESSAGES {
        let id = Uuid::new_v4();
        let gidx = i % NUM_GROUPS;
        let gid = group_ids[gidx];
        let mut body = fake_message(&mut rng);
        if i < RARE_MESSAGES {
            body.push(' ');
            body.push_str(RARE_TOKEN);
        }
        if i % COMMON_INJECT_EVERY == 0 {
            body.push(' ');
            body.push_str(COMMON_TERM);
        }
        // Record one message that belongs to group index 1 (group B).
        if gidx == 1 && group_b_message_id.is_nil() {
            group_b_message_id = id;
        }
        batch.push((id, gid, body));
        if batch.len() >= 500 {
            flush_messages(pool, &batch).await?;
            batch.clear();
        }
    }
    if !batch.is_empty() {
        flush_messages(pool, &batch).await?;
    }

    // Embeddings
    let mut ebatch: Vec<(Uuid, Uuid, String, Vector)> = Vec::with_capacity(250);
    for i in 0..NUM_EMBEDDINGS {
        let id = Uuid::new_v4();
        let gid = group_ids[i % NUM_GROUPS];
        let content = fake_message(&mut rng);
        let emb = Vector::from(fake_embedding(&mut rng));
        ebatch.push((id, gid, content, emb));
        if ebatch.len() >= 250 {
            flush_embeddings(pool, &ebatch).await?;
            ebatch.clear();
        }
    }
    if !ebatch.is_empty() {
        flush_embeddings(pool, &ebatch).await?;
    }

    if group_b_message_id.is_nil() {
        return Err(anyhow!("failed to capture group B message id during seed"));
    }

    Ok(SeedContext {
        group_ids,
        group_b_message_id,
    })
}

async fn flush_messages(pool: &PgPool, batch: &[(Uuid, Uuid, String)]) -> Result<()> {
    let mut qb =
        sqlx::QueryBuilder::<sqlx::Postgres>::new("INSERT INTO messages (id, group_id, body) ");
    qb.push_values(batch, |mut b, (id, gid, body)| {
        b.push_bind(*id).push_bind(*gid).push_bind(body.clone());
    });
    qb.build().execute(pool).await?;
    Ok(())
}

async fn flush_embeddings(pool: &PgPool, batch: &[(Uuid, Uuid, String, Vector)]) -> Result<()> {
    let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new(
        "INSERT INTO memory_embeddings (id, group_id, content, embedding) ",
    );
    qb.push_values(batch, |mut b, (id, gid, content, emb)| {
        b.push_bind(*id)
            .push_bind(*gid)
            .push_bind(content.clone())
            .push_bind(emb.clone());
    });
    qb.build().execute(pool).await?;
    Ok(())
}

// ---------- Scenarios ----------

async fn run_b1(pool: &PgPool, iterations: u32) -> Result<ScenarioResult> {
    // Insert 100k fresh messages into a separate table, 500 per batch.
    // Each iteration drops+recreates messages_b1.
    const ROWS: usize = NUM_MESSAGES;
    const BATCH: usize = 500;
    let mut rng = seeded_rng();
    let mut total_latencies_ms: Vec<f64> = Vec::new();
    let mut throughput_samples: Vec<f64> = Vec::new();

    for iter in 0..iterations {
        sqlx::query("DROP TABLE IF EXISTS messages_b1").execute(pool).await?;
        sqlx::query(
            "CREATE TABLE messages_b1 (id uuid PRIMARY KEY, group_id uuid NOT NULL, body text NOT NULL)",
        )
        .execute(pool)
        .await?;

        let start = Instant::now();
        let mut inserted = 0;
        while inserted < ROWS {
            let n = (ROWS - inserted).min(BATCH);
            let batch: Vec<(Uuid, Uuid, String)> = (0..n)
                .map(|_| (Uuid::new_v4(), Uuid::new_v4(), fake_message(&mut rng)))
                .collect();
            let mut qb = sqlx::QueryBuilder::<sqlx::Postgres>::new(
                "INSERT INTO messages_b1 (id, group_id, body) ",
            );
            qb.push_values(&batch, |mut b, (id, gid, body)| {
                b.push_bind(*id).push_bind(*gid).push_bind(body.clone());
            });
            qb.build().execute(pool).await?;
            inserted += n;
        }
        let elapsed = start.elapsed();
        let elapsed_ms = elapsed.as_secs_f64() * 1000.0;
        let throughput = ROWS as f64 / elapsed.as_secs_f64();
        total_latencies_ms.push(elapsed_ms);
        throughput_samples.push(throughput);
        tracing::info!(iter, elapsed_ms, throughput, "B1 iteration");
    }

    sqlx::query("DROP TABLE IF EXISTS messages_b1").execute(pool).await?;

    let med_latency = median(total_latencies_ms.clone());
    let med_thru = median(throughput_samples);
    Ok(ScenarioResult {
        id: "B1".into(),
        name: "Insert throughput (100k msgs batched)".into(),
        backend: "postgres".into(),
        status: ScenarioStatus::Ok,
        p50_ms: Some(med_latency),
        p95_ms: None,
        p99_ms: None,
        throughput: Some(med_thru),
        iterations,
        notes: format!("batch=500; rows={ROWS}; median wall={med_latency:.1}ms"),
    })
}

async fn latency_scenario<F, Fut>(
    id: &str,
    name: &str,
    iterations: u32,
    queries_per_iter: usize,
    mut op: F,
) -> Result<ScenarioResult>
where
    F: FnMut(usize) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    let mut p50s = Vec::new();
    let mut p95s = Vec::new();
    let mut p99s = Vec::new();

    for iter in 0..iterations {
        let mut hist = new_hist();
        for q in 0..queries_per_iter {
            let start = Instant::now();
            op(q).await?;
            let us = start.elapsed().as_micros() as u64;
            hist.record(us.max(1))?;
        }
        p50s.push(us_to_ms(hist.value_at_quantile(0.5)));
        p95s.push(us_to_ms(hist.value_at_quantile(0.95)));
        p99s.push(us_to_ms(hist.value_at_quantile(0.99)));
        tracing::info!(
            scenario = id,
            iter,
            p50_ms = p50s.last().copied().unwrap_or_default(),
            p95_ms = p95s.last().copied().unwrap_or_default(),
            "iteration"
        );
    }

    Ok(ScenarioResult {
        id: id.into(),
        name: name.into(),
        backend: "postgres".into(),
        status: ScenarioStatus::Ok,
        p50_ms: Some(median(p50s)),
        p95_ms: Some(median(p95s)),
        p99_ms: Some(median(p99s)),
        throughput: None,
        iterations,
        notes: format!("{queries_per_iter} queries/iter; median of per-iter percentiles"),
    })
}

async fn run_b2(pool: &PgPool, iterations: u32) -> Result<ScenarioResult> {
    let pool = pool.clone();
    latency_scenario("B2", "FTS common term", iterations, 100, move |_| {
        let pool = pool.clone();
        async move {
            let _rows: Vec<(Uuid,)> = sqlx::query_as(
                "SELECT id FROM messages WHERE body_tsv @@ plainto_tsquery('portuguese', $1) LIMIT 100",
            )
            .bind(COMMON_TERM)
            .fetch_all(&pool)
            .await?;
            Ok(())
        }
    })
    .await
}

async fn run_b3(pool: &PgPool, iterations: u32) -> Result<ScenarioResult> {
    let pool = pool.clone();
    latency_scenario("B3", "FTS rare term", iterations, 100, move |_| {
        let pool = pool.clone();
        async move {
            let _rows: Vec<(Uuid,)> = sqlx::query_as(
                "SELECT id FROM messages WHERE body_tsv @@ plainto_tsquery('portuguese', $1) LIMIT 100",
            )
            .bind(RARE_TOKEN)
            .fetch_all(&pool)
            .await?;
            Ok(())
        }
    })
    .await
}

async fn run_b4(pool: &PgPool, iterations: u32) -> Result<ScenarioResult> {
    let pool_outer = pool.clone();
    // Precompute 100 random query vectors deterministically.
    let mut rng = seeded_rng();
    let queries: Vec<Vector> = (0..100)
        .map(|_| Vector::from(fake_embedding(&mut rng)))
        .collect();

    latency_scenario("B4", "ANN top-5", iterations, 100, move |q| {
        let pool = pool_outer.clone();
        let qv = queries[q].clone();
        async move {
            let _rows: Vec<(Uuid,)> =
                sqlx::query_as("SELECT id FROM memory_embeddings ORDER BY embedding <=> $1 LIMIT 5")
                    .bind(qv)
                    .fetch_all(&pool)
                    .await?;
            Ok(())
        }
    })
    .await
}

async fn run_b5(
    pool: &PgPool,
    seed_ctx: &SeedContext,
    iterations: u32,
) -> Result<ScenarioResult> {
    let pool_outer = pool.clone();
    let mut rng = seeded_rng();
    let queries: Vec<Vector> = (0..100)
        .map(|_| Vector::from(fake_embedding(&mut rng)))
        .collect();
    let group_ids = seed_ctx.group_ids.clone();

    latency_scenario("B5", "Hybrid query (FTS+ANN+group)", iterations, 100, move |q| {
        let pool = pool_outer.clone();
        let qv = queries[q].clone();
        let gid = group_ids[q % group_ids.len()];
        async move {
            // Hybrid: FTS match in messages restricted to group, union with nearest embeddings
            // for the same group. Model's worth lies in the planner handling both in one shot.
            let sql = r#"
                WITH fts AS (
                    SELECT id FROM messages
                    WHERE group_id = $1
                      AND body_tsv @@ plainto_tsquery('portuguese', $2)
                    LIMIT 20
                ),
                ann AS (
                    SELECT id FROM memory_embeddings
                    WHERE group_id = $1
                    ORDER BY embedding <=> $3
                    LIMIT 5
                )
                SELECT id FROM fts
                UNION ALL
                SELECT id FROM ann
            "#;
            let _rows: Vec<(Uuid,)> = sqlx::query_as(sql)
                .bind(gid)
                .bind(COMMON_TERM)
                .bind(qv)
                .fetch_all(&pool)
                .await?;
            Ok(())
        }
    })
    .await
}

async fn run_b6(pool: &PgPool, seed_ctx: &SeedContext) -> Result<ScenarioResult> {
    // Open a dedicated connection so RLS + SET LOCAL behave predictably.
    // Use a non-superuser role because RLS is bypassed for superusers/table owners by default.
    sqlx::query("DROP ROLE IF EXISTS app_user").execute(pool).await.ok();
    sqlx::query("CREATE ROLE app_user LOGIN PASSWORD 'app_pw'")
        .execute(pool)
        .await?;
    sqlx::query("GRANT SELECT, INSERT, UPDATE, DELETE ON messages TO app_user")
        .execute(pool)
        .await?;
    sqlx::query("GRANT SELECT ON groups TO app_user")
        .execute(pool)
        .await?;

    // Fetch the URL components from the existing pool's connect_options is awkward;
    // simplest path: build a new pool against app_user via the same host/port. We don't
    // have direct access to host/port here, so we reuse the existing pool but SET ROLE.
    let mut tx = pool.begin().await?;
    sqlx::query("SET LOCAL ROLE app_user").execute(&mut *tx).await?;
    // Pretend the request came in for group A (index 0).
    let group_a = seed_ctx.group_ids[0];
    let stmt = format!("SET LOCAL app.current_group_id = '{group_a}'");
    sqlx::query(&stmt).execute(&mut *tx).await?;

    // Attempt to SELECT a message known to belong to group B.
    let rows: Vec<(Uuid,)> = sqlx::query_as("SELECT id FROM messages WHERE id = $1")
        .bind(seed_ctx.group_b_message_id)
        .fetch_all(&mut *tx)
        .await?;
    tx.rollback().await?;

    let (status, notes) = if rows.is_empty() {
        (
            ScenarioStatus::Ok,
            "RLS blocked cross-group SELECT (0 rows returned for group-B id while scoped to group A)".to_string(),
        )
    } else {
        (
            ScenarioStatus::Failed,
            format!("RLS LEAK: returned {} rows for cross-group id", rows.len()),
        )
    };

    Ok(ScenarioResult {
        id: "B6".into(),
        name: "RLS cross-group isolation".into(),
        backend: "postgres".into(),
        status,
        p50_ms: None,
        p95_ms: None,
        p99_ms: None,
        throughput: None,
        iterations: 1,
        notes,
    })
}

async fn run_b7(pool: &PgPool, seed_ctx: &SeedContext) -> Result<ScenarioResult> {
    // 100 concurrent tasks, each: 10 reads + 1 write.
    let group_ids = seed_ctx.group_ids.clone();
    let mut handles = Vec::with_capacity(100);
    let start_all = Instant::now();
    for t in 0..100u64 {
        let pool = pool.clone();
        let group_ids = group_ids.clone();
        handles.push(tokio::spawn(async move {
            let task_start = Instant::now();
            let gid = group_ids[(t as usize) % group_ids.len()];
            // Read via a direct connection (pool hands out up to 20).
            for _ in 0..10 {
                let _rows: Vec<(Uuid,)> =
                    sqlx::query_as("SELECT id FROM messages WHERE group_id = $1 LIMIT 10")
                        .bind(gid)
                        .fetch_all(&pool)
                        .await?;
            }
            // Write one message
            let mid = Uuid::new_v4();
            sqlx::query("INSERT INTO messages (id, group_id, body) VALUES ($1, $2, $3)")
                .bind(mid)
                .bind(gid)
                .bind(format!("b7-task-{t}"))
                .execute(&pool)
                .await?;
            Ok::<u64, anyhow::Error>(task_start.elapsed().as_micros() as u64)
        }));
    }
    let mut hist = new_hist();
    let mut failures = 0usize;
    for h in handles {
        match h.await {
            Ok(Ok(us)) => hist.record(us.max(1))?,
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "B7 task failed");
                failures += 1;
            }
            Err(e) => {
                tracing::warn!(error = %e, "B7 join error");
                failures += 1;
            }
        }
    }
    let wall_ms = start_all.elapsed().as_secs_f64() * 1000.0;

    Ok(ScenarioResult {
        id: "B7".into(),
        name: "Pool stress (100 concurrent, 10R+1W each)".into(),
        backend: "postgres".into(),
        status: if failures == 0 {
            ScenarioStatus::Ok
        } else {
            ScenarioStatus::Failed
        },
        p50_ms: Some(us_to_ms(hist.value_at_quantile(0.5))),
        p95_ms: Some(us_to_ms(hist.value_at_quantile(0.95))),
        p99_ms: Some(us_to_ms(hist.value_at_quantile(0.99))),
        throughput: None,
        iterations: 1,
        notes: format!(
            "100 tasks, pool=20; wall={wall_ms:.1}ms; failures={failures}"
        ),
    })
}

// Silence unused warnings for constants imported only by some builds.
#[allow(dead_code)]
const _USE: usize = EMBEDDING_DIM + NUM_EMBEDDINGS + NUM_GROUPS;
