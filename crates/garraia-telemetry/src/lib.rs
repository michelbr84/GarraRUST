//! GarraIA telemetry crate — OpenTelemetry + tracing baseline.

pub mod config;
pub mod layers;
pub mod metrics;
pub mod redact;
pub mod tracer;

pub use config::TelemetryConfig;
pub use layers::{http_trace_layer, propagate_request_id_layer, request_id_layer};
pub use metrics::{inc_errors, inc_requests, record_latency, set_active_sessions};
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

/// Track whether `init` has already installed global providers, so repeated
/// calls log a warning instead of silently clobbering an existing exporter.
/// Idempotency is not enforced (the second guard still owns a provider), but
/// the warning gives operators a breadcrumb for unexpected double-init paths.
static INIT_ONCE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Initialize the telemetry pipeline.
///
/// Fail-soft by contract: callers should treat an `Err` as "telemetry is off,
/// keep serving traffic." The CLI wraps this in `unwrap_or_else(|e| { log; None })`.
pub fn init(config: TelemetryConfig) -> Result<Guard, Error> {
    if INIT_ONCE.swap(true, std::sync::atomic::Ordering::SeqCst) {
        tracing::warn!(
            "garraia_telemetry::init called more than once; replacing global tracer provider"
        );
    }
    let tracer_provider = tracer::init_tracer(&config)?;
    let metrics_handle = metrics::init_metrics(&config)?;
    Ok(Guard {
        tracer_provider,
        metrics_handle,
    })
}
