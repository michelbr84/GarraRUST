//! Shared fixtures and result types for the GAR-373 benchmark.
//!
//! Deterministic data generation via seeded RNG, so runs are comparable.

use anyhow::Result;
use chrono::{DateTime, Utc};
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

pub const SEED: u64 = 42;
pub const NUM_GROUPS: usize = 1_000;
pub const MEMBERS_PER_GROUP: usize = 10;
pub const NUM_MESSAGES: usize = 100_000;
pub const NUM_EMBEDDINGS: usize = 100_000;
pub const EMBEDDING_DIM: usize = 768;

/// Result of a single scenario run, reported back from each backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioResult {
    pub id: String,           // B1, B2, ...
    pub name: String,
    pub backend: String,      // "postgres" | "sqlite"
    pub status: ScenarioStatus,
    pub p50_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    pub p99_ms: Option<f64>,
    pub throughput: Option<f64>,
    pub iterations: u32,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ScenarioStatus {
    Ok,
    NotApplicable,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchmarkRun {
    pub run_at: DateTime<Utc>,
    pub host: HostSpec,
    pub scenarios: Vec<ScenarioResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostSpec {
    pub os: String,
    pub arch: String,
    pub cpus: usize,
    pub total_memory_mb: Option<u64>,
}

pub fn seeded_rng() -> ChaCha8Rng {
    ChaCha8Rng::seed_from_u64(SEED)
}

pub fn host_spec() -> HostSpec {
    HostSpec {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        cpus: std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1),
        total_memory_mb: None, // stub; let the runner pick up via sysinfo if desired
    }
}

pub fn write_results(path: &str, results: &BenchmarkRun) -> Result<()> {
    let json = serde_json::to_string_pretty(results)?;
    std::fs::write(path, json)?;
    Ok(())
}

pub fn merge_results(a: BenchmarkRun, b: BenchmarkRun) -> BenchmarkRun {
    BenchmarkRun {
        run_at: a.run_at,
        host: a.host,
        scenarios: a.scenarios.into_iter().chain(b.scenarios).collect(),
    }
}

/// Generate a realistic fake message body in portuguese.
/// Deterministic given the same RNG.
pub fn fake_message(rng: &mut ChaCha8Rng) -> String {
    use fake::faker::lorem::pt_br::*;
    use fake::Fake;
    let words = (3..15).fake_with_rng::<usize, _>(rng);
    Sentence(words..words + 1).fake_with_rng(rng)
}

/// Generate a random unit-normalized embedding of EMBEDDING_DIM dimensions.
pub fn fake_embedding(rng: &mut ChaCha8Rng) -> Vec<f32> {
    use rand::Rng;
    let mut v: Vec<f32> = (0..EMBEDDING_DIM).map(|_| rng.gen_range(-1.0..1.0)).collect();
    let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
    v
}
