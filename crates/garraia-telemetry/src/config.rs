//! Telemetry configuration with env-var loading and validation.

use serde::Deserialize;
use validator::Validate;

use crate::Error;

fn default_service_name() -> String {
    "garraia-gateway".to_string()
}

fn default_sample_ratio() -> f64 {
    1.0
}

fn default_metrics_bind() -> String {
    "127.0.0.1:9464".to_string()
}

#[derive(Debug, Clone, Deserialize, Validate)]
pub struct TelemetryConfig {
    #[serde(default)]
    pub enabled: bool,

    #[validate(url)]
    #[serde(default)]
    pub otlp_endpoint: Option<String>,

    #[serde(default = "default_service_name")]
    pub service_name: String,

    #[validate(range(min = 0.0, max = 1.0))]
    #[serde(default = "default_sample_ratio")]
    pub sample_ratio: f64,

    #[serde(default)]
    pub metrics_enabled: bool,

    #[serde(default = "default_metrics_bind")]
    pub metrics_bind: String,
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            otlp_endpoint: None,
            service_name: default_service_name(),
            sample_ratio: default_sample_ratio(),
            metrics_enabled: false,
            metrics_bind: default_metrics_bind(),
        }
    }
}

impl TelemetryConfig {
    pub fn from_env() -> Result<Self, Error> {
        let mut cfg = Self::default();

        if let Ok(v) = std::env::var("GARRAIA_OTEL_ENABLED") {
            cfg.enabled = parse_bool(&v);
        }
        if let Ok(v) = std::env::var("GARRAIA_OTEL_EXPORTER_OTLP_ENDPOINT") {
            if !v.is_empty() {
                cfg.otlp_endpoint = Some(v);
            }
        }
        if let Ok(v) = std::env::var("GARRAIA_OTEL_SERVICE_NAME") {
            if !v.is_empty() {
                cfg.service_name = v;
            }
        }
        if let Ok(v) = std::env::var("GARRAIA_OTEL_SAMPLE_RATIO") {
            cfg.sample_ratio = v
                .parse::<f64>()
                .map_err(|e| Error::Init(format!("invalid GARRAIA_OTEL_SAMPLE_RATIO: {e}")))?;
        }
        if let Ok(v) = std::env::var("GARRAIA_METRICS_ENABLED") {
            cfg.metrics_enabled = parse_bool(&v);
        }
        if let Ok(v) = std::env::var("GARRAIA_METRICS_BIND") {
            if !v.is_empty() {
                cfg.metrics_bind = v;
            }
        }

        cfg.validate()
            .map_err(|e| Error::Init(format!("invalid telemetry config: {e}")))?;
        Ok(cfg)
    }
}

fn parse_bool(s: &str) -> bool {
    matches!(s.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Single sequential test function — avoids env-var races across parallel tests.
    #[test]
    fn env_loading_and_validation() {
        // Use unique prefix-scoped vars by temporarily setting and removing.
        // Happy path.
        unsafe {
            std::env::set_var("GARRAIA_OTEL_ENABLED", "true");
            std::env::set_var(
                "GARRAIA_OTEL_EXPORTER_OTLP_ENDPOINT",
                "http://collector:4317",
            );
            std::env::set_var("GARRAIA_OTEL_SERVICE_NAME", "unit-test-svc");
            std::env::set_var("GARRAIA_OTEL_SAMPLE_RATIO", "0.5");
            std::env::set_var("GARRAIA_METRICS_ENABLED", "1");
            std::env::set_var("GARRAIA_METRICS_BIND", "127.0.0.1:19464");
        }

        let cfg = TelemetryConfig::from_env().expect("happy path should parse");
        assert!(cfg.enabled);
        assert_eq!(cfg.otlp_endpoint.as_deref(), Some("http://collector:4317"));
        assert_eq!(cfg.service_name, "unit-test-svc");
        assert!((cfg.sample_ratio - 0.5).abs() < f64::EPSILON);
        assert!(cfg.metrics_enabled);
        assert_eq!(cfg.metrics_bind, "127.0.0.1:19464");

        // Invalid URL.
        unsafe {
            std::env::set_var("GARRAIA_OTEL_EXPORTER_OTLP_ENDPOINT", "not-a-url");
        }
        assert!(TelemetryConfig::from_env().is_err());

        // Restore endpoint, break sample ratio.
        unsafe {
            std::env::set_var(
                "GARRAIA_OTEL_EXPORTER_OTLP_ENDPOINT",
                "http://collector:4317",
            );
            std::env::set_var("GARRAIA_OTEL_SAMPLE_RATIO", "2.0");
        }
        assert!(TelemetryConfig::from_env().is_err());

        // Cleanup.
        unsafe {
            std::env::remove_var("GARRAIA_OTEL_ENABLED");
            std::env::remove_var("GARRAIA_OTEL_EXPORTER_OTLP_ENDPOINT");
            std::env::remove_var("GARRAIA_OTEL_SERVICE_NAME");
            std::env::remove_var("GARRAIA_OTEL_SAMPLE_RATIO");
            std::env::remove_var("GARRAIA_METRICS_ENABLED");
            std::env::remove_var("GARRAIA_METRICS_BIND");
        }
    }

    #[test]
    fn default_has_sensible_values() {
        let d = TelemetryConfig::default();
        assert!(!d.enabled);
        assert!(!d.metrics_enabled);
        assert_eq!(d.service_name, "garraia-gateway");
        assert_eq!(d.metrics_bind, "127.0.0.1:9464");
        assert!((d.sample_ratio - 1.0).abs() < f64::EPSILON);
    }
}
