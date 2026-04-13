//! GarraIA telemetry crate — OpenTelemetry + tracing baseline.

pub mod config;
pub mod layers;
pub mod metrics;
pub mod redact;
pub mod tracer;

pub use config::TelemetryConfig;
pub use layers::{http_trace_layer, propagate_request_id_layer, request_id_layer};
pub use metrics::{inc_errors, inc_requests, record_latency, set_active_sessions};

/// Backwards-compatible alias for [`TelemetryConfig`].
pub type Config = TelemetryConfig;

/// RAII guard returned by [`init`]. Flushes pipelines on drop.
///
/// Drop order is deliberate: the tracer provider is shut down explicitly
/// (flushes in-flight spans via the OTLP batch processor), then the
/// `metrics_handle` drops implicitly. `metrics-exporter-prometheus` does not
/// need an async flush — the HTTP listener exposes a live snapshot — so the
/// implicit drop order is correct and no coordination with the tracer is
/// required.
pub struct Guard {
    tracer_provider: Option<opentelemetry_sdk::trace::TracerProvider>,
    #[allow(dead_code)]
    metrics_handle: Option<metrics_exporter_prometheus::PrometheusHandle>,
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
