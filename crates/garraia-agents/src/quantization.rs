//! Auto-selection of local model quantization based on available VRAM.
//!
//! Implements ROADMAP §1.1 item "Quantização: suporte a modelos Q4_K_M,
//! Q5_K_M, Q8_0 com auto-seleção por VRAM disponível".
//!
//! The selection is a pure function so it can be unit-tested without any
//! GPU: given the detected VRAM (in bytes) and the model size (in billions
//! of parameters), it picks the highest-fidelity quantization whose weights
//! (plus a safety headroom) fit in VRAM. When VRAM is unknown the caller
//! falls back to [`ModelQuant::default_quant`].

use tracing::{debug, info, warn};

/// GGUF model quantization levels supported by the auto-selection policy.
///
/// Bytes-per-parameter values are empirical for llama.cpp K-quant GGUF
/// files (weights + metadata), used only as an estimate for fitting.
// The GGUF community spells these levels with underscores (Q4_K_M); keeping
// the exact labels avoids mapping bugs at the repo/tag boundary.
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelQuant {
    /// ~4.8 bits/weight. Highest fidelity of the supported set.
    Q8_0,
    /// ~5.5 bits/weight. Balanced quality / footprint.
    Q5_K_M,
    /// ~4.5 bits/weight. Lowest footprint, quality loss still small.
    Q4_K_M,
}

impl ModelQuant {
    /// Empirical bytes per parameter (weights + GGUF metadata), including
    /// the quantization tables emitted per block.
    pub fn bytes_per_param(self) -> f64 {
        match self {
            // 8 bits/weight + block header amortization.
            Self::Q8_0 => 1.07,
            // 5.5 bits/weight ≈ 0.6875 bytes.
            Self::Q5_K_M => 0.70,
            // 4.5 bits/weight ≈ 0.5625 bytes.
            Self::Q4_K_M => 0.57,
        }
    }

    /// Human-readable label, as used by GGUF repos and Ollama tags.
    pub fn label(self) -> &'static str {
        match self {
            Self::Q8_0 => "Q8_0",
            Self::Q5_K_M => "Q5_K_M",
            Self::Q4_K_M => "Q4_K_M",
        }
    }

    /// Estimated size in bytes of the model weights for `params_b`
    /// billions of parameters, plus the safety headroom applied by the
    /// selection policy. Rounds **up**: an underestimate would make the
    /// selection promise more fidelity than the GPU can hold.
    pub fn estimated_weight_bytes(self, params_b: f64) -> u64 {
        (params_b * 1_000_000_000.0 * self.bytes_per_param() * HEADROOM).ceil() as u64
    }

    /// Fallback when VRAM is unknown or below every supported level.
    pub fn default_quant() -> Self {
        Self::Q4_K_M
    }

    /// Numeric fidelity rank (higher = more faithful to fp16).
    pub fn fidelity_rank(self) -> u8 {
        match self {
            Self::Q4_K_M => 0,
            Self::Q5_K_M => 1,
            Self::Q8_0 => 2,
        }
    }
}

/// Safety headroom multiplied over the raw weight estimate to leave room
/// for the KV cache, compute buffers and runtime overhead.
pub const HEADROOM: f64 = 1.25;

/// Pick the highest-fidelity quantization that fits `vram_bytes`.
///
/// `vram_bytes` is the *total* usable VRAM of the target GPU. `params_b`
/// is the model size in billions of parameters (e.g. 8.0 for an 8B model).
/// Returns [`ModelQuant::default_quant`] when `vram_bytes` is `None` or too
/// small for even `Q4_K_M`.
pub fn auto_select_quant(vram_bytes: Option<u64>, params_b: f64) -> ModelQuant {
    let Some(vram) = vram_bytes else {
        debug!(
            "VRAM unknown; defaulting to {}",
            ModelQuant::default_quant().label()
        );
        return ModelQuant::default_quant();
    };
    // Highest fidelity first.
    for quant in [ModelQuant::Q8_0, ModelQuant::Q5_K_M, ModelQuant::Q4_K_M] {
        let needed = quant.estimated_weight_bytes(params_b);
        if needed <= vram {
            info!(
                vram_bytes = vram,
                params_b,
                quant = quant.label(),
                weight_estimate = needed,
                "auto-selected model quantization for local inference"
            );
            return quant;
        }
    }
    warn!(
        vram_bytes = vram,
        params_b,
        "VRAM below the minimum supported quantization; defaulting to {}",
        ModelQuant::default_quant().label()
    );
    ModelQuant::default_quant()
}

/// Detected GPU memory in bytes, when a supported probe succeeds.
///
/// Probes, in order:
/// 1. `nvidia-smi --query-gpu=memory.total --format=csv,noheader,nounits`
///    (NVIDIA, returns MiB),
/// 2. `rocm-smi --showmeminfo vram --csv` (AMD ROCm).
///
/// Any failure (missing tool, non-Linux, parse error) yields `None`, which
/// the caller must treat as "unknown VRAM".
pub fn detect_vram_bytes() -> Option<u64> {
    if let Some(bytes) = probe_nvidia_smi() {
        return Some(bytes);
    }
    probe_rocm_smi()
}

fn probe_nvidia_smi() -> Option<u64> {
    let out = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // First GPU line; values are MiB. Field 0 of the CSV row.
    let line = String::from_utf8_lossy(&out.stdout);
    let mib: u64 = line
        .lines()
        .next()?
        .split(',')
        .next()?
        .trim()
        .parse()
        .ok()?;
    mib.checked_mul(1024 * 1024)
}

fn probe_rocm_smi() -> Option<u64> {
    let out = std::process::Command::new("rocm-smi")
        .args(["--showmeminfo", "vram", "--csv"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    // CSV: gpu, vram_used, vram_total (bytes). Take the max total seen.
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .skip(1) // header row
        .filter_map(|l| l.split(',').nth(2))
        .filter_map(|f| f.trim().replace('"', "").parse::<u64>().ok())
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn unknown_vram_defaults_to_q4() {
        assert_eq!(auto_select_quant(None, 8.0), ModelQuant::Q4_K_M);
    }

    #[test]
    fn small_vram_defaults_to_q4() {
        // 2 GiB cannot fit even an 8B Q4_K_M (~4.6 GiB with headroom).
        assert_eq!(auto_select_quant(Some(2 * GIB), 8.0), ModelQuant::Q4_K_M);
    }

    #[test]
    fn q4_fits_modest_vram() {
        // 8B model: Q5_K_M needs ~7.0 GB with headroom; 6.5 GiB (6.98 GB)
        // is just below it, so the policy picks Q4_K_M (~5.7 GB).
        assert_eq!(
            auto_select_quant(Some(6 * GIB + GIB / 2), 8.0),
            ModelQuant::Q4_K_M
        );
    }

    #[test]
    fn q5_fits_when_headroom_allows() {
        // 14 GiB fits Q5_K_M (~5.7 GiB) but not Q8_0 (~8.6 GiB) for 8B?
        // 8 * 1.07 * 1.25 = 10.7 GiB -> does NOT fit in 14? It does.
        // Recompute: Q8_0 needs 8*1.07*1.25 = 10.7 GiB <= 14 -> picks Q8_0.
        assert_eq!(auto_select_quant(Some(14 * GIB), 8.0), ModelQuant::Q8_0);
        // 9 GiB: Q8_0 (10.7 GiB) no, Q5_K_M (5.6 GiB) yes.
        assert_eq!(auto_select_quant(Some(9 * GIB), 8.0), ModelQuant::Q5_K_M);
    }

    #[test]
    fn large_model_downgrades() {
        // 24 GiB GPU with a 27B model: Q8_0 needs 27*1.07*1.25 = 36.1 GiB (no),
        // Q5_K_M needs 23.6 GiB (yes).
        assert_eq!(auto_select_quant(Some(24 * GIB), 27.0), ModelQuant::Q5_K_M);
    }

    #[test]
    fn labels_and_ratios_are_stable() {
        assert_eq!(ModelQuant::Q8_0.label(), "Q8_0");
        assert_eq!(ModelQuant::Q5_K_M.label(), "Q5_K_M");
        assert_eq!(ModelQuant::Q4_K_M.label(), "Q4_K_M");
        assert!(ModelQuant::Q8_0.bytes_per_param() > ModelQuant::Q5_K_M.bytes_per_param());
        assert!(ModelQuant::Q5_K_M.bytes_per_param() > ModelQuant::Q4_K_M.bytes_per_param());
    }

    #[test]
    fn estimates_scale_linearly() {
        let small = ModelQuant::Q4_K_M.estimated_weight_bytes(8.0);
        let big = ModelQuant::Q4_K_M.estimated_weight_bytes(16.0);
        assert_eq!(big, small * 2);
    }

    #[test]
    fn monotonically_higher_vram_never_chooses_lower_fidelity() {
        let mut vram = GIB;
        let mut last = ModelQuant::Q4_K_M.fidelity_rank();
        for _ in 0..64 {
            let q = auto_select_quant(Some(vram), 8.0).fidelity_rank();
            assert!(q >= last, "fidelity must not decrease as VRAM grows");
            last = q;
            vram += GIB;
        }
    }
}
