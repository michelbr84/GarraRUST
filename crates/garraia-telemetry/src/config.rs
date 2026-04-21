//! Telemetry configuration with env-var loading and validation.

use std::fmt;

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

#[derive(Clone, Deserialize, Validate)]
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

    /// Optional Bearer token for authenticating `/metrics` requests.
    ///
    /// Plan 0024 / GAR-412. When `Some`, the metrics auth middleware
    /// requires `Authorization: Bearer <token>` and compares in
    /// constant time. When `None`, the middleware falls back to the
    /// allowlist (if any) and loopback-only dev ergonomics. Redacted
    /// from the `Debug` impl below.
    #[serde(default)]
    pub metrics_token: Option<String>,

    /// Optional comma-separated CIDR allowlist for `/metrics` requests.
    ///
    /// Plan 0024 / GAR-412. Parsed by the gateway via
    /// `rate_limiter::parse_trusted_proxies` so any malformed entry is
    /// logged and dropped, never poisoning the whole list. An empty
    /// `Vec` is semantically "no allowlist configured".
    #[serde(default)]
    pub metrics_allowlist: Vec<String>,
}

// Custom `Debug` impl redacts `metrics_token` to keep startup logs and
// panic dumps safe — aligned with regra absoluta #6 (never log secrets).
impl fmt::Debug for TelemetryConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelemetryConfig")
            .field("enabled", &self.enabled)
            .field("otlp_endpoint", &self.otlp_endpoint)
            .field("service_name", &self.service_name)
            .field("sample_ratio", &self.sample_ratio)
            .field("metrics_enabled", &self.metrics_enabled)
            .field("metrics_bind", &self.metrics_bind)
            .field(
                "metrics_token",
                &self.metrics_token.as_ref().map(|_| "<redacted>"),
            )
            .field("metrics_allowlist", &self.metrics_allowlist)
            .finish()
    }
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
            metrics_token: None,
            metrics_allowlist: Vec::new(),
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
        if let Ok(v) = std::env::var("GARRAIA_METRICS_TOKEN") {
            let trimmed = v.trim();
            if !trimmed.is_empty() {
                cfg.metrics_token = Some(trimmed.to_string());
            }
        }
        if let Ok(v) = std::env::var("GARRAIA_METRICS_ALLOW") {
            cfg.metrics_allowlist = v
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
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
            std::env::set_var("GARRAIA_METRICS_TOKEN", "s3cret-metrics-token");
            std::env::set_var("GARRAIA_METRICS_ALLOW", "127.0.0.1/32, 10.0.0.0/8");
        }

        let cfg = TelemetryConfig::from_env().expect("happy path should parse");
        assert!(cfg.enabled);
        assert_eq!(cfg.otlp_endpoint.as_deref(), Some("http://collector:4317"));
        assert_eq!(cfg.service_name, "unit-test-svc");
        assert!((cfg.sample_ratio - 0.5).abs() < f64::EPSILON);
        assert!(cfg.metrics_enabled);
        assert_eq!(cfg.metrics_bind, "127.0.0.1:19464");
        assert_eq!(cfg.metrics_token.as_deref(), Some("s3cret-metrics-token"));
        assert_eq!(
            cfg.metrics_allowlist,
            vec!["127.0.0.1/32".to_string(), "10.0.0.0/8".to_string()]
        );

        // Empty vars ⇒ None / empty Vec.
        unsafe {
            std::env::set_var("GARRAIA_METRICS_TOKEN", "   ");
            std::env::set_var("GARRAIA_METRICS_ALLOW", " , ");
        }
        let cfg = TelemetryConfig::from_env().expect("empty metrics vars parse");
        assert!(cfg.metrics_token.is_none());
        assert!(cfg.metrics_allowlist.is_empty());

        // Debug redacts token but keeps other fields.
        unsafe {
            std::env::set_var("GARRAIA_METRICS_TOKEN", "super-secret");
        }
        let cfg = TelemetryConfig::from_env().expect("debug path should parse");
        let dbg = format!("{cfg:?}");
        assert!(
            !dbg.contains("super-secret"),
            "token must not leak via Debug: {dbg}"
        );
        assert!(
            dbg.contains("<redacted>"),
            "token must be marked redacted: {dbg}"
        );
        assert!(dbg.contains("127.0.0.1:19464"));

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
            std::env::remove_var("GARRAIA_METRICS_TOKEN");
            std::env::remove_var("GARRAIA_METRICS_ALLOW");
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
        assert!(d.metrics_token.is_none());
        assert!(d.metrics_allowlist.is_empty());
    }

    #[test]
    fn debug_redacts_token_when_none() {
        let cfg = TelemetryConfig::default();
        let dbg = format!("{cfg:?}");
        // With None the field shows as `metrics_token: None` (wrapped
        // via Option<&str>), which is fine — no token to leak.
        assert!(dbg.contains("metrics_token"));
        assert!(!dbg.contains("<redacted>"));
    }
}
