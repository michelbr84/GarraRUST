//! GarraIA telemetry crate — OpenTelemetry + tracing baseline.

pub mod config;
pub mod layers;
pub mod metrics;
pub mod redact;
pub mod tracer;

pub use config::TelemetryConfig;
pub use layers::{http_trace_layer, propagate_request_id_layer, request_id_layer};
pub use metrics::{
    debug_assert_route_template, inc_errors, inc_requests, record_latency, set_active_sessions,
};
// Plan 0024 (GAR-412): re-export `PrometheusHandle` so the gateway can
// build a dedicated `/metrics` listener without depending on the
// `metrics-exporter-prometheus` crate directly.
pub use metrics_exporter_prometheus::PrometheusHandle;

/// Backwards-compatible alias for [`TelemetryConfig`].
pub type Config = TelemetryConfig;

/// RAII guard returned by [`init`]. Flushes pipelines on drop.
///
/// Drop order is deliberate: the tracer provider is shut down explicitly
/// (flushes in-flight spans via the OTLP batch processor), then the
/// `metrics_handle` drops implicitly. `metrics-exporter-prometheus` does
/// not need an async flush — the recorder holds metrics in memory — so
/// the implicit drop order is correct and no coordination with the
/// tracer is required.
///
/// Plan 0024 (GAR-412): the guard exposes [`Guard::metrics_handle`]
/// so the gateway can spawn its dedicated `/metrics` listener against
/// the same globally-installed recorder.
pub struct Guard {
    tracer_provider: Option<opentelemetry_sdk::trace::TracerProvider>,
    metrics_handle: Option<PrometheusHandle>,
}

impl Guard {
    /// Return a cloned handle to the globally-installed Prometheus
    /// recorder, if metrics are enabled.
    ///
    /// `PrometheusHandle` is `Clone` by design — the gateway takes one
    /// clone to serve `/metrics` over HTTP (auth'd by the metrics auth
    /// middleware), while the guard keeps the original to tie recorder
    /// shutdown to drop order.
    pub fn metrics_handle(&self) -> Option<PrometheusHandle> {
        self.metrics_handle.clone()
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(provider) = self.tracer_provider.take() {
            let _ = provider.shutdown();
        }
    }
}

/// Telemetry errors.
#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("telemetry init failed: {0}")]
    Init(String),
}

/// Track whether `init` has already installed global providers.
///
/// Plan 0025 (GAR-411 L3): swapped from `AtomicBool` to `OnceLock<()>` so
/// that the second call short-circuits to an empty guard instead of
/// re-attempting provider installation. `metrics-exporter-prometheus`
/// returns `Err("attempted to set a recorder after the metrics system
/// was already initialized")` on double-install; the OTLP tracer pipeline
/// silently clobbers the previous global. Neither is what callers want —
/// the contract is "first call wins, subsequent calls are no-ops."
///
/// # Race semantics (precise)
///
/// Two threads racing on the first call both see `INIT_ONCE.get().is_none()`,
/// so both enter the install block. The outcome then differs per subsystem:
///
/// - **OTLP tracer:** both threads install their own provider and call
///   `global::set_tracer_provider`. The second call *silently clobbers*
///   the winner's provider — a real bug of the upstream `opentelemetry`
///   global, not of this code. The losing thread's guard drops its tracer
///   when it goes out of scope (flushing + shutdown), after which the
///   global points at a shut-down provider. The winner then emits spans
///   into that dead provider.
///
/// - **Prometheus recorder:** the winner's `install_recorder()` succeeds.
///   The loser's `install_recorder()` returns `Err("attempted to set a
///   recorder after the metrics system was already initialized")`. That
///   `Err` propagates up through `?` and the loser's `init()` returns
///   `Err(Error::Init(...))` to its caller — *not* `Ok(empty_guard)` as
///   the docblock on `init()` otherwise promises for repeated calls.
///
/// After either path settles, `INIT_ONCE.set(())` runs twice: one call
/// wins, the other returns `Err(())` which is ignored. Subsequent serial
/// calls correctly short-circuit.
///
/// **Why this is acceptable in practice:** `init()` is called exactly once
/// from the CLI boot path (single thread, before any worker threads spawn).
/// The race is only reachable in parallel test runs or programmatic reuse,
/// both of which are out of the intended call pattern. A stricter single-
/// flight would wrap install work in `OnceLock::get_or_try_init`, but that
/// API is nightly-only as of rustc 1.83; emulating it with a `Mutex` would
/// block worker threads on boot. The current design accepts the benign
/// race on boot in exchange for keeping the fail-soft `Result` contract
/// flowing to the single caller that actually matters.
static INIT_ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();

/// Initialize the telemetry pipeline.
///
/// **Fail-soft contract (plan 0026 / GAR-411 M3):** this function never
/// returns `Err`. Any internal init failure (tracer OTLP endpoint unreachable,
/// Prometheus recorder already installed by a sibling process, validation
/// error) is logged via `tracing::warn!` and converted to `None` fields on
/// the returned `Guard`. The gateway keeps serving traffic with degraded
/// (or absent) telemetry — the invariant of GAR-384 ("telemetry must never
/// crash the main process") applies end-to-end without ceremony at each
/// call site.
///
/// Rationale for dropping `Result`: the 3 real callers (CLI `main.rs`, the
/// `smoke.rs` / `idempotent_init*.rs` integration tests) all either log-and-
/// discard the error or `.expect()` it away. Forcing each caller to spell
/// out fail-soft boilerplate added noise and an opportunity for accidental
/// `?`-propagation that would abort the gateway. The signature change is
/// the M3 follow-up from the plan 0024 / 0025 security review.
///
/// **Idempotency (plan 0025 L3):** subsequent calls after a successful first
/// init return an empty `Guard` (both `tracer_provider` and `metrics_handle`
/// are `None`) and log a warning. No provider or recorder is re-installed.
pub fn init(config: TelemetryConfig) -> Guard {
    if INIT_ONCE.get().is_some() {
        tracing::warn!(
            "garraia_telemetry::init called more than once; returning no-op guard (first call wins)"
        );
        return Guard {
            tracer_provider: None,
            metrics_handle: None,
        };
    }
    let tracer_provider = match tracer::init_tracer(&config) {
        Ok(provider) => provider,
        Err(e) => {
            tracing::warn!(
                error = %e,
                "OTLP tracer init failed; continuing without tracing"
            );
            None
        }
    };
    let metrics_handle = match metrics::init_metrics(&config) {
        Ok(handle) => handle,
        Err(e) => {
            tracing::warn!(
                error = %e,
                "Prometheus recorder init failed; continuing without metrics"
            );
            None
        }
    };
    // `set` returning Err means another thread won the race. This thread
    // still ran the full install block above (subject to the race semantics
    // documented on INIT_ONCE). Either way, mark as initialized so subsequent
    // *serial* callers short-circuit correctly.
    let _ = INIT_ONCE.set(());
    Guard {
        tracer_provider,
        metrics_handle,
    }
}
