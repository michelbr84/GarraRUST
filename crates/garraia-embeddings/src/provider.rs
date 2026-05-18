//! [`EmbeddingProvider`] trait — the abstraction over an embedding model.
//!
//! Implementations live in their own crates / slices:
//!
//! - `MxbaiProvider` via `candle` (ADR 0001) — future PR.
//! - `OllamaProvider` for self-hosted Ollama endpoints — future PR.
//! - [`DeterministicProvider`] (this module, gated by `testing-provider`
//!   feature) — used by `[dev-dependencies]` consumers to write unit tests
//!   that don't need a real model.

use async_trait::async_trait;

use crate::error::EmbeddingError;
use crate::types::EmbeddingVector;

/// An embedding model — produces a fixed-dimension vector from input text.
///
/// Implementations must be `Send + Sync` so they can be stored as
/// `Arc<dyn EmbeddingProvider>` in shared application state.
///
/// `embed_batch` is on the trait by design: single-shot embeds are an
/// anti-pattern for RAG latency budgets. Callers MUST batch; providers MAY
/// fan out internally.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// A short, stable identifier for the model — goes into
    /// `memory_embeddings.model`. Examples: `"mxbai-embed-large-v1"`,
    /// `"deterministic-sha256-768"`.
    fn model_id(&self) -> &str;

    /// Embed a single piece of text. Default implementation defers to
    /// [`embed_batch`](EmbeddingProvider::embed_batch) with a single-item
    /// slice — most providers will want to override this for the trivial
    /// path.
    async fn embed(&self, text: &str) -> Result<EmbeddingVector, EmbeddingError> {
        let mut out = self.embed_batch(&[text]).await?;
        out.pop().ok_or(EmbeddingError::ProviderRejected {
            reason: "batch returned empty result",
        })
    }

    /// Embed a batch of texts in one round-trip / model invocation.
    ///
    /// The output `Vec<EmbeddingVector>` MUST have the same length as the
    /// input slice and be in the same order.
    async fn embed_batch(&self, texts: &[&str]) -> Result<Vec<EmbeddingVector>, EmbeddingError>;
}

// =========================================================================
// DeterministicProvider — testing only.
// =========================================================================

#[cfg(feature = "testing-provider")]
mod deterministic {
    use super::*;
    use crate::types::EMBEDDING_DIM;
    use sha2::{Digest, Sha256};

    /// A pseudo-random, **deterministic** [`EmbeddingProvider`] for tests.
    ///
    /// Same input → same output, byte-for-byte. Different inputs → different
    /// outputs (collisions are theoretically possible at the SHA-256 level
    /// but cosmically unlikely; we don't depend on this cryptographically).
    ///
    /// **NOT for production.** Vectors produced here have no semantic
    /// meaning — nearest-neighbor results are gibberish. The provider exists
    /// so downstream crates can unit-test their consumption of the
    /// [`EmbeddingProvider`] trait without bringing up a real model.
    ///
    /// The vector is built by repeatedly hashing `seed_index || text`, mapping
    /// each 4-byte SHA-256 window into a float in `[-1.0, 1.0]`. After
    /// `EMBEDDING_DIM` floats are produced the vector is L2-normalized.
    #[derive(Debug, Default, Clone)]
    pub struct DeterministicProvider;

    impl DeterministicProvider {
        /// Construct.
        pub fn new() -> Self {
            Self
        }

        fn embed_one(text: &str) -> EmbeddingVector {
            let mut out: Vec<f32> = Vec::with_capacity(EMBEDDING_DIM);
            // We need EMBEDDING_DIM floats; each SHA-256 hash gives 8 floats
            // (8 × 4 bytes = 32 bytes). 768 / 8 = 96 rounds.
            let rounds = EMBEDDING_DIM.div_ceil(8);
            for seed in 0..rounds {
                let mut hasher = Sha256::new();
                hasher.update(seed.to_le_bytes());
                hasher.update(text.as_bytes());
                let digest = hasher.finalize();
                for chunk in digest.chunks_exact(4) {
                    if out.len() == EMBEDDING_DIM {
                        break;
                    }
                    let bytes: [u8; 4] = chunk.try_into().expect("chunks_exact(4) yields [u8;4]");
                    // Map u32 to (-1.0, 1.0).
                    let n = u32::from_le_bytes(bytes);
                    let f = (n as f64 / u32::MAX as f64) * 2.0 - 1.0;
                    out.push(f as f32);
                }
            }
            // L2-normalize so the vectors live on the unit sphere — closer to
            // what a real embedding model produces.
            let norm = out.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
            if norm > 0.0 {
                for v in out.iter_mut() {
                    *v = ((*v as f64) / norm) as f32;
                }
            }
            EmbeddingVector::try_from_vec(out).expect("constructed with EMBEDDING_DIM elements")
        }
    }

    #[async_trait]
    impl EmbeddingProvider for DeterministicProvider {
        fn model_id(&self) -> &str {
            "deterministic-sha256-768"
        }

        async fn embed_batch(
            &self,
            texts: &[&str],
        ) -> Result<Vec<EmbeddingVector>, EmbeddingError> {
            Ok(texts.iter().map(|t| Self::embed_one(t)).collect())
        }
    }
}

#[cfg(feature = "testing-provider")]
pub use deterministic::DeterministicProvider;

#[cfg(all(test, feature = "testing-provider"))]
mod tests {
    use super::*;
    use crate::types::EMBEDDING_DIM;

    #[tokio::test(flavor = "current_thread")]
    async fn deterministic_provider_is_deterministic() {
        let p = DeterministicProvider::new();
        let a = p.embed("hello world").await.unwrap();
        let b = p.embed("hello world").await.unwrap();
        assert_eq!(a, b);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn different_inputs_yield_different_outputs() {
        let p = DeterministicProvider::new();
        let a = p.embed("alpha").await.unwrap();
        let b = p.embed("beta").await.unwrap();
        assert_ne!(a, b);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn batch_preserves_order_and_length() {
        let p = DeterministicProvider::new();
        let inputs = ["one", "two", "three"];
        let out = p.embed_batch(&inputs).await.unwrap();
        assert_eq!(out.len(), inputs.len());

        // Verify positional correspondence by recomputing index 1 via embed().
        let solo = p.embed("two").await.unwrap();
        assert_eq!(out[1], solo);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn output_dimension_is_fixed() {
        let p = DeterministicProvider::new();
        let v = p.embed("anything").await.unwrap();
        assert_eq!(v.as_slice().len(), EMBEDDING_DIM);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn model_id_is_stable() {
        // Real consumers will write this to memory_embeddings.model, so we
        // pin it down by a test.
        let p = DeterministicProvider::new();
        assert_eq!(p.model_id(), "deterministic-sha256-768");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn vectors_are_approximately_unit_norm() {
        let p = DeterministicProvider::new();
        let v = p
            .embed("a longer string that pushes more entropy through")
            .await
            .unwrap();
        let norm: f64 = v
            .as_slice()
            .iter()
            .map(|x| (*x as f64).powi(2))
            .sum::<f64>()
            .sqrt();
        // f32 round-trip + multi-round hashing — give it a 1e-3 tolerance.
        assert!((norm - 1.0).abs() < 1e-3, "expected unit norm, got {norm}");
    }
}
