//! SQLite scenarios (B1-B4). B5-B7 are N/A for SQLite.
//!
//! See plans/0002-gar-373-adr-postgres-decision.md §7.
//!
//! Note: sqlite-vec is NOT loaded. We deliberately use a plain cosine-similarity
//! scan over BLOB-serialized embeddings for B4 — this reflects the honest
//! default-SQLite deployment without native extensions. Results are documented
//! in each scenario's `notes`.

use crate::shared::{
    BenchmarkRun, NUM_EMBEDDINGS, NUM_GROUPS, NUM_MESSAGES, ScenarioResult,
    ScenarioStatus, fake_embedding, fake_message, host_spec, seeded_rng,
};
use anyhow::{Context, Result};
use chrono::Utc;
use hdrhistogram::Histogram;
use rusqlite::{Connection, params};
use std::time::Instant;
use uuid::Uuid;

const RARE_TOKEN: &str = "QUUX-RARE-TOKEN";
const NUM_RARE: usize = 10;

fn new_hist() -> Histogram<u64> {
    Histogram::<u64>::new_with_bounds(1, 60_000_000, 3).expect("histogram bounds")
}

fn median_f64(mut xs: Vec<f64>) -> f64 {
    xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
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
    us as f64 / 1000.0
}

fn embedding_to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}

fn blob_to_embedding(b: &[u8]) -> Vec<f32> {
    let mut out = Vec::with_capacity(b.len() / 4);
    for chunk in b.chunks_exact(4) {
        out.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    out
}

fn cosine_sim(a: &[f32], b: &[f32]) -> f32 {
    let mut dot = 0.0f32;
    for i in 0..a.len().min(b.len()) {
        dot += a[i] * b[i];
    }
    // Vectors are unit-normalized by fake_embedding.
    dot
}

fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = WAL;
        PRAGMA synchronous = NORMAL;
        PRAGMA temp_store = MEMORY;

        CREATE TABLE groups (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE messages (
            id TEXT PRIMARY KEY,
            group_id TEXT NOT NULL REFERENCES groups(id),
            body TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE INDEX messages_group_id_idx ON messages(group_id);

        CREATE VIRTUAL TABLE messages_fts USING fts5(
            body,
            content='messages',
            content_rowid='rowid',
            tokenize='unicode61'
        );

        CREATE TRIGGER messages_ai AFTER INSERT ON messages BEGIN
            INSERT INTO messages_fts(rowid, body) VALUES (new.rowid, new.body);
        END;
        CREATE TRIGGER messages_ad AFTER DELETE ON messages BEGIN
            INSERT INTO messages_fts(messages_fts, rowid, body) VALUES('delete', old.rowid, old.body);
        END;
        CREATE TRIGGER messages_au AFTER UPDATE ON messages BEGIN
            INSERT INTO messages_fts(messages_fts, rowid, body) VALUES('delete', old.rowid, old.body);
            INSERT INTO messages_fts(rowid, body) VALUES (new.rowid, new.body);
        END;

        CREATE TABLE memory_embeddings (
            id TEXT PRIMARY KEY,
            group_id TEXT NOT NULL REFERENCES groups(id),
            content TEXT NOT NULL,
            embedding BLOB NOT NULL,
            created_at TEXT NOT NULL
        );
        CREATE INDEX memory_embeddings_group_idx ON memory_embeddings(group_id);

        -- B1 uses a dedicated table with no FTS triggers for pure insert speed.
        CREATE TABLE messages_b1 (
            id TEXT PRIMARY KEY,
            group_id TEXT NOT NULL,
            body TEXT NOT NULL,
            created_at TEXT NOT NULL
        );
        "#,
    )?;
    Ok(())
}

fn seed(conn: &mut Connection) -> Result<Vec<String>> {
    let mut rng = seeded_rng();
    let now = Utc::now().to_rfc3339();

    // Groups
    let mut group_ids: Vec<String> = Vec::with_capacity(NUM_GROUPS);
    {
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare("INSERT INTO groups (id, name, created_at) VALUES (?1, ?2, ?3)")?;
            for i in 0..NUM_GROUPS {
                let id = Uuid::new_v4().to_string();
                let name = format!("group-{i}");
                stmt.execute(params![id, name, now])?;
                group_ids.push(id);
            }
        }
        tx.commit()?;
    }

    // Messages (seeding set for FTS / searches). Rare token injection at fixed positions.
    let rare_positions: std::collections::HashSet<usize> =
        (0..NUM_RARE).map(|i| i * (NUM_MESSAGES / NUM_RARE)).collect();

    {
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO messages (id, group_id, body, created_at) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for i in 0..NUM_MESSAGES {
                let id = Uuid::new_v4().to_string();
                let gid = &group_ids[i % NUM_GROUPS];
                let mut body = fake_message(&mut rng);
                if rare_positions.contains(&i) {
                    body.push(' ');
                    body.push_str(RARE_TOKEN);
                }
                stmt.execute(params![id, gid, body, now])?;
            }
        }
        tx.commit()?;
    }

    // Embeddings
    {
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO memory_embeddings (id, group_id, content, embedding, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for i in 0..NUM_EMBEDDINGS {
                let id = Uuid::new_v4().to_string();
                let gid = &group_ids[i % NUM_GROUPS];
                let content = format!("mem-{i}");
                let emb = fake_embedding(&mut rng);
                let blob = embedding_to_blob(&emb);
                stmt.execute(params![id, gid, content, blob, now])?;
            }
        }
        tx.commit()?;
    }

    Ok(group_ids)
}

// --- Scenarios ---

fn bench_b1(conn: &mut Connection, iterations: u32) -> Result<ScenarioResult> {
    let mut p95s: Vec<f64> = Vec::new();
    let mut p50s: Vec<f64> = Vec::new();
    let mut p99s: Vec<f64> = Vec::new();
    let mut throughputs: Vec<f64> = Vec::new();

    for _ in 0..iterations {
        // Clear table for each iteration.
        conn.execute("DELETE FROM messages_b1", [])?;
        let mut rng = seeded_rng();
        let now = Utc::now().to_rfc3339();

        let mut hist = new_hist();
        let overall_start = Instant::now();

        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO messages_b1 (id, group_id, body, created_at) VALUES (?1, ?2, ?3, ?4)",
            )?;
            for i in 0..NUM_MESSAGES {
                let id = Uuid::new_v4().to_string();
                let gid = format!("g-{}", i % NUM_GROUPS);
                let body = fake_message(&mut rng);
                let t0 = Instant::now();
                stmt.execute(params![id, gid, body, now])?;
                let dt_us = t0.elapsed().as_micros() as u64;
                hist.record(dt_us.max(1))?;
            }
        }
        tx.commit()?;
        let elapsed = overall_start.elapsed().as_secs_f64();
        let tput = NUM_MESSAGES as f64 / elapsed;

        p50s.push(us_to_ms(hist.value_at_quantile(0.50)));
        p95s.push(us_to_ms(hist.value_at_quantile(0.95)));
        p99s.push(us_to_ms(hist.value_at_quantile(0.99)));
        throughputs.push(tput);
    }

    Ok(ScenarioResult {
        id: "B1".into(),
        name: "Insert throughput (100k messages, single txn)".into(),
        backend: "sqlite".into(),
        status: ScenarioStatus::Ok,
        p50_ms: Some(median_f64(p50s)),
        p95_ms: Some(median_f64(p95s.clone())),
        p99_ms: Some(median_f64(p99s)),
        throughput: Some(median_f64(throughputs)),
        iterations,
        notes: "WAL mode, synchronous=NORMAL, single transaction, prepared statement. messages_b1 table has no FTS triggers.".into(),
    })
}

fn bench_fts(
    conn: &Connection,
    id: &str,
    name: &str,
    term: &str,
    query_iters: u32,
    notes: &str,
) -> Result<ScenarioResult> {
    let mut stmt =
        conn.prepare("SELECT rowid FROM messages_fts WHERE messages_fts MATCH ?1 LIMIT 100")?;
    let mut hist = new_hist();
    let mut last_rowcount = 0usize;
    for _ in 0..query_iters {
        let t0 = Instant::now();
        let rows: Vec<i64> = stmt
            .query_map(params![term], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let dt_us = t0.elapsed().as_micros() as u64;
        hist.record(dt_us.max(1))?;
        last_rowcount = rows.len();
    }

    Ok(ScenarioResult {
        id: id.into(),
        name: name.into(),
        backend: "sqlite".into(),
        status: ScenarioStatus::Ok,
        p50_ms: Some(us_to_ms(hist.value_at_quantile(0.50))),
        p95_ms: Some(us_to_ms(hist.value_at_quantile(0.95))),
        p99_ms: Some(us_to_ms(hist.value_at_quantile(0.99))),
        throughput: None,
        iterations: query_iters,
        notes: format!("{notes} (last_rowcount={last_rowcount})"),
    })
}

fn bench_b4_ann(conn: &Connection, iterations: u32) -> Result<ScenarioResult> {
    let mut rng = seeded_rng();
    let query_vec = fake_embedding(&mut rng);

    let mut p50s = Vec::new();
    let mut p95s = Vec::new();
    let mut p99s = Vec::new();

    for _ in 0..iterations {
        let mut hist = new_hist();
        // Single iteration = one full O(n) scan.
        let t0 = Instant::now();
        let mut stmt = conn.prepare("SELECT id, embedding FROM memory_embeddings")?;
        let mut rows = stmt.query([])?;
        let mut top: Vec<(f32, String)> = Vec::with_capacity(6);
        while let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            let emb = blob_to_embedding(&blob);
            let sim = cosine_sim(&query_vec, &emb);
            // Keep top-5 via simple insertion.
            if top.len() < 5 {
                top.push((sim, id));
                top.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            } else if sim > top[4].0 {
                top[4] = (sim, id);
                top.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            }
        }
        let dt_us = t0.elapsed().as_micros() as u64;
        hist.record(dt_us.max(1))?;

        p50s.push(us_to_ms(hist.value_at_quantile(0.50)));
        p95s.push(us_to_ms(hist.value_at_quantile(0.95)));
        p99s.push(us_to_ms(hist.value_at_quantile(0.99)));
    }

    Ok(ScenarioResult {
        id: "B4".into(),
        name: "ANN top-5 (plain cosine scan)".into(),
        backend: "sqlite".into(),
        status: ScenarioStatus::Ok,
        p50_ms: Some(median_f64(p50s)),
        p95_ms: Some(median_f64(p95s)),
        p99_ms: Some(median_f64(p99s)),
        throughput: None,
        iterations,
        notes: "plain cosine scan (sqlite-vec not loaded); real production would use sqlite-vec virtual table".into(),
    })
}

pub fn run_all(iterations: u32) -> Result<BenchmarkRun> {
    let tmp = tempfile::tempdir().context("tempdir")?;
    let db_path = tmp.path().join("bench.db");
    tracing::info!("sqlite db at {}", db_path.display());

    let mut conn = Connection::open(&db_path)?;
    init_schema(&conn)?;
    tracing::info!("sqlite schema initialized");

    let t0 = Instant::now();
    let _group_ids = seed(&mut conn)?;
    tracing::info!("sqlite seed complete in {:?}", t0.elapsed());

    let mut scenarios = Vec::new();

    tracing::info!("sqlite B1 insert throughput");
    scenarios.push(bench_b1(&mut conn, iterations)?);

    // Probe for a common term that actually exists in the corpus. fake::lorem::pt_br
    // generates latin-ish lorem-ipsum tokens, so we try a few candidates.
    let candidates = ["ut", "et", "est", "in", "a*"];
    let mut common_term = "a*".to_string();
    for c in candidates {
        let count: i64 = conn.query_row(
            "SELECT count(*) FROM messages_fts WHERE messages_fts MATCH ?1",
            params![c],
            |r| r.get(0),
        ).unwrap_or(0);
        tracing::info!("sqlite B2 probe term {c:?} -> {count} rows");
        if count >= 100 {
            common_term = c.to_string();
            break;
        }
    }
    tracing::info!("sqlite B2 FTS common term = {common_term}");
    scenarios.push(bench_fts(
        &conn,
        "B2",
        &format!("FTS common term ('{common_term}')"),
        &common_term,
        100,
        "unicode61 tokenizer, LIMIT 100",
    )?);

    tracing::info!("sqlite B3 FTS rare term");
    scenarios.push(bench_fts(
        &conn,
        "B3",
        "FTS rare term (QUUX-RARE-TOKEN)",
        "\"QUUX-RARE-TOKEN\"",
        100,
        "unicode61 tokenizer, LIMIT 100, injected 10 matches",
    )?);

    tracing::info!("sqlite B4 ANN plain scan");
    scenarios.push(bench_b4_ann(&conn, iterations.min(10))?);

    // N/A scenarios
    scenarios.push(ScenarioResult {
        id: "B5".into(),
        name: "Hybrid FTS + ANN".into(),
        backend: "sqlite".into(),
        status: ScenarioStatus::NotApplicable,
        p50_ms: None,
        p95_ms: None,
        p99_ms: None,
        throughput: None,
        iterations: 0,
        notes: "Hybrid FTS+ANN requires both subsystems to be queryable in one SQL; SQLite with plain cosine scan cannot express this without application-side merging".into(),
    });
    scenarios.push(ScenarioResult {
        id: "B6".into(),
        name: "Row-level security / tenant isolation".into(),
        backend: "sqlite".into(),
        status: ScenarioStatus::NotApplicable,
        p50_ms: None,
        p95_ms: None,
        p99_ms: None,
        throughput: None,
        iterations: 0,
        notes: "SQLite has no row-level security equivalent; isolation must be enforced at application layer".into(),
    });
    scenarios.push(ScenarioResult {
        id: "B7".into(),
        name: "Connection-pool / concurrent writer stress".into(),
        backend: "sqlite".into(),
        status: ScenarioStatus::NotApplicable,
        p50_ms: None,
        p95_ms: None,
        p99_ms: None,
        throughput: None,
        iterations: 0,
        notes: "SQLite WAL mode allows concurrent readers but serializes writers; meaningful pool stress comparison requires Postgres".into(),
    });

    Ok(BenchmarkRun {
        run_at: Utc::now(),
        host: host_spec(),
        scenarios,
    })
}
