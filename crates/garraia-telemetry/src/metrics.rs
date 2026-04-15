//! Prometheus metrics exporter + baseline metric helpers.

use std::net::SocketAddr;

use metrics::{counter, gauge, histogram};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

use crate::{Error, config::TelemetryConfig};

pub const METRIC_REQUESTS_TOTAL: &str = "garraia_requests_total";
pub const METRIC_HTTP_LATENCY_SECONDS: &str = "garraia_http_latency_seconds";
pub const METRIC_ERRORS_TOTAL: &str = "garraia_errors_total";
pub const METRIC_ACTIVE_SESSIONS: &str = "garraia_active_sessions";

pub fn init_metrics(config: &TelemetryConfig) -> Result<Option<PrometheusHandle>, Error> {
    if !config.metrics_enabled {
        return Ok(None);
    }

    let addr: SocketAddr = config.metrics_bind.parse().map_err(|e| {
        Error::Init(format!(
            "invalid metrics_bind '{}': {e}",
            config.metrics_bind
        ))
    })?;

    let handle = PrometheusBuilder::new()
        .with_http_listener(addr)
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
