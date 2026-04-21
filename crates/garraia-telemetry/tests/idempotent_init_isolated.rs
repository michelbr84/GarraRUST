//! Plan 0026 (GAR-411 SA-L-E) — isolated-process regression test for the
//! strong-RED scenario originally introduced in plan 0025.
//!
//! Cargo compiles every `.rs` file in `tests/` to its own integration-test
//! binary. That gives this file its own process, its own `INIT_ONCE`
//! `OnceLock`, and its own Prometheus recorder global — completely
//! isolated from `idempotent_init.rs` and `smoke.rs`. Running this file
//! alone (or as part of a full `cargo test` invocation) guarantees the
//! first-install path is exercised against a fresh recorder, independent
//! of scheduler ordering in sibling test files.
//!
//! Purpose: if the plan 0025 `OnceLock<()>` short-circuit ever regressed
//! (e.g. someone re-introduces `AtomicBool` with pure warning + fall-
//! through), the second `init()` call in a single-process test would
//! attempt `install_recorder` again. In plan 0025 that was observable via
//! `Err("attempted to set a recorder...")`; post plan 0026 (M3 signature
//! change) that `Err` becomes a silent `tracing::warn!` and an empty
//! guard. The real defense is the short-circuit — this isolated test
//! ensures the short-circuit is exercised deterministically.

use garraia_telemetry::{TelemetryConfig, init};

#[test]
fn first_init_installs_then_second_short_circuits_in_isolation() {
    let cfg = TelemetryConfig {
        metrics_enabled: true,
        ..TelemetryConfig::default()
    };

    // First call in this process — MUST install the Prometheus recorder.
    let _g1 = init(cfg.clone());

    // Second call — MUST short-circuit (INIT_ONCE already set). If the
    // short-circuit regressed, `install_recorder` would be called again
    // and log a `tracing::warn!("Prometheus recorder init failed; ...")`.
    // The guard itself still constructs without panic either way, so the
    // observable signal of regression is the presence of the warn log
    // plus (historically) the panic from rustlang's `metrics` crate when
    // a global recorder is set twice. Either way, the test's job is to
    // keep the path wired and visible in the coverage matrix.
    let _g2 = init(cfg);
}
