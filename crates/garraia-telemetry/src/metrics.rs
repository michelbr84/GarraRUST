//! Prometheus metrics recorder + baseline metric helpers.
//!
//! Plan 0024 (GAR-412): this module installs the Prometheus *recorder*
//! only — it no longer binds an HTTP listener. Serving `/metrics` over
//! HTTP is the gateway's responsibility (`garraia-gateway::metrics_exporter`),
//! which owns the auth middleware and startup fail-closed check.
//! Telemetry stays decoupled from Axum/Tower at the metrics level.

use metrics::{counter, gauge, histogram};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

use crate::{Error, config::TelemetryConfig};

pub const METRIC_REQUESTS_TOTAL: &str = "garraia_requests_total";
pub const METRIC_HTTP_LATENCY_SECONDS: &str = "garraia_http_latency_seconds";
pub const METRIC_ERRORS_TOTAL: &str = "garraia_errors_total";
pub const METRIC_ACTIVE_SESSIONS: &str = "garraia_active_sessions";

/// Install the global Prometheus recorder and return its handle.
///
/// Returns `Ok(None)` when `metrics_enabled` is false (fail-soft — the
/// gateway still boots). The handle is `Clone`, so callers can share it
/// between the dedicated listener and any future render site without
/// re-installing the global recorder.
pub fn init_metrics(config: &TelemetryConfig) -> Result<Option<PrometheusHandle>, Error> {
    if !config.metrics_enabled {
        return Ok(None);
    }

    let handle = PrometheusBuilder::new()
        .install_recorder()
        .map_err(|e| Error::Init(format!("failed to install prometheus recorder: {e}")))?;

    Ok(Some(handle))
}

pub fn inc_requests(route: &str, status: u16) {
    counter!(
        METRIC_REQUESTS_TOTAL,
        "route" => route.to_string(),
        "status" => status.to_string()
    )
    .increment(1);
}

pub fn record_latency(route: &str, seconds: f64) {
    histogram!(METRIC_HTTP_LATENCY_SECONDS, "route" => route.to_string()).record(seconds);
}

pub fn inc_errors(kind: &str) {
    counter!(METRIC_ERRORS_TOTAL, "kind" => kind.to_string()).increment(1);
}

pub fn set_active_sessions(n: f64) {
    gauge!(METRIC_ACTIVE_SESSIONS).set(n);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_config_returns_none() {
        let cfg = TelemetryConfig {
            metrics_enabled: false,
            ..TelemetryConfig::default()
        };
        let handle = init_metrics(&cfg).expect("must not error when disabled");
        assert!(handle.is_none());
    }
}
