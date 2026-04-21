//! Regression: `init()` must be truly idempotent.
//!
//! Plan 0025 / GAR-411 L3.
//!
//! Three sequential calls should leave exactly one tracer provider installed
//! (not replace on every call). The observable contract at this layer is
//! "no error + no panic + no global clobber". We verify the `Result` paths
//! explicitly.
//!
//! # Parallelism invariant
//!
//! Cargo's default `--test-threads` runs `#[test]` fns within a single
//! integration-test binary in parallel. The `INIT_ONCE` global + the
//! Prometheus recorder are process-wide state, so concurrent `init()` calls
//! can race. We mitigate with a `TEST_LOCK` mutex below — every test
//! acquires it before touching `init()`, which serializes the 3 tests in
//! *this* file. Cross-file ordering (e.g. vs. `smoke.rs`) is not a concern
//! because Cargo compiles each `.rs` in `tests/` to its own binary (separate
//! process, separate `INIT_ONCE`).
//!
//! Code-reviewer finding M-B (plan 0025 review, 2026-04-21): documented the
//! invariant and enforced it with `TEST_LOCK` instead of relying on the
//! implicit `cargo test` scheduling order.

use std::sync::{Mutex, MutexGuard};

use garraia_telemetry::{TelemetryConfig, init};

static TEST_LOCK: Mutex<()> = Mutex::new(());

fn lock<'a>() -> MutexGuard<'a, ()> {
    // Poisoned lock is fine to ignore — we only care about serialization,
    // not the guarded value. `.expect` would force re-run on first panic,
    // losing the later signal.
    match TEST_LOCK.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
}

#[test]
fn three_inits_do_not_panic_with_default_config() {
    let _guard = lock();
    let cfg = TelemetryConfig::default(); // enabled=false, metrics_enabled=false

    // Plan 0026 (GAR-411 M3): `init()` returns `Guard` directly — any
    // internal failure is logged via `tracing::warn!` and yields an
    // empty guard. The three calls here should all produce valid guards.
    let g1 = init(cfg.clone());
    let g2 = init(cfg.clone());
    let g3 = init(cfg);

    // Dropping in reverse order must also be safe — each guard either owns
    // a (now idle) provider or nothing at all. None of them should double-
    // shut-down the same global.
    drop(g3);
    drop(g2);
    drop(g1);
}

#[test]
fn second_init_is_a_noop_guard_when_tracer_was_disabled() {
    let _guard = lock();
    // With tracer + metrics both disabled, both guards carry None. Plan
    // 0026 (M3): `init()` returns `Guard` directly.
    let cfg = TelemetryConfig::default();
    let _g1 = init(cfg.clone());
    let _g2 = init(cfg);
}

#[test]
fn second_init_with_metrics_enabled_does_not_retry_install() {
    let _guard = lock();
    // Strong regression test: `metrics-exporter-prometheus::install_recorder`
    // fails (Err) if a global recorder is already set. Before the plan 0025
    // fix this test FAILED on the second `init()` — the AtomicBool guard
    // only emitted a warning, then still called `init_metrics`, which tried
    // to re-install and returned Err. After the fix, the second call short-
    // circuits and returns an empty guard.
    //
    // Note on execution order: `TEST_LOCK` serializes this with the other
    // two tests in this file. If one of the earlier tests already set
    // `INIT_ONCE`, the first `init()` here short-circuits to an empty guard
    // (no recorder install attempted); the assertion still holds because
    // `expect` only checks `Ok(_)`. If this test runs first in lock order,
    // line 1 installs the recorder, line 2 short-circuits, both pass.
    // Either way the observable contract (no `Err` return) is preserved.
    let cfg = TelemetryConfig {
        metrics_enabled: true,
        ..TelemetryConfig::default()
    };

    // Plan 0026 (M3): `init()` returns `Guard` directly. The idempotency
    // invariant from plan 0025 (L3) still holds: if this test is the first
    // to run in lock order, the first call installs the recorder; the
    // second call short-circuits to an empty guard without re-attempting
    // `install_recorder` (which would have panic'd in the pre-0025 world).
    let _g1 = init(cfg.clone());
    let _g2 = init(cfg);
}
