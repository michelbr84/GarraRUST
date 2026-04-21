//! Prometheus metrics recorder + baseline metric helpers.
//!
//! Plan 0024 (GAR-412): this module installs the Prometheus *recorder*
//! only — it no longer binds an HTTP listener. Serving `/metrics` over
//! HTTP is the gateway's responsibility (`garraia-gateway::metrics_exporter`),
//! which owns the auth middleware and startup fail-closed check.
//! Telemetry stays decoupled from Axum/Tower at the metrics level.
//!
//! # Cardinality contract (plan 0025 / GAR-411 M1)
//!
//! The `route` label passed to [`inc_requests`] and [`record_latency`] MUST
//! be a **template**, not a concrete path. Good: `/api/sessions/{id}/messages`.
//! Bad: `/api/sessions/8f2c7e1a-…-abcd/messages`. Concrete paths create one
//! metric series per unique ID — a per-user/per-session/per-request label
//! cardinality explosion that melts the Prometheus recorder's internal
//! hashmaps and the downstream Prometheus server.
//!
//! In debug builds, [`inc_requests`] / [`record_latency`] call
//! [`debug_assert_route_template`] which panics when it detects a UUID-looking
//! segment or a long all-numeric segment. In release builds the check is a
//! no-op (`#[inline(always)] fn _(route: &str) {}`) — zero runtime cost. The
//! guard is a belt-and-suspenders line of defense on top of the Axum router's
//! own `matched_path` extractor, which the gateway's HTTP middleware uses as
//! the canonical source of route templates.

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
    debug_assert_route_template(route);
    counter!(
        METRIC_REQUESTS_TOTAL,
        "route" => route.to_string(),
        "status" => status.to_string()
    )
    .increment(1);
}

pub fn record_latency(route: &str, seconds: f64) {
    debug_assert_route_template(route);
    histogram!(METRIC_HTTP_LATENCY_SECONDS, "route" => route.to_string()).record(seconds);
}

pub fn inc_errors(kind: &str) {
    counter!(METRIC_ERRORS_TOTAL, "kind" => kind.to_string()).increment(1);
}

pub fn set_active_sessions(n: f64) {
    gauge!(METRIC_ACTIVE_SESSIONS).set(n);
}

/// Debug-only guard against cardinality-exploding route labels.
///
/// Plan 0025 (GAR-411 M1). Panics in debug builds when the route argument
/// looks like a concrete path instead of a template. No-op in release.
///
/// Heuristics (all lowercase-normalized):
/// - Any path segment matching `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`
///   (36 chars, 4 dashes at positions 8/13/18/23) ⇒ UUID leak.
/// - Any path segment that is 6+ chars and entirely ASCII-digit ⇒ numeric
///   ID leak (e.g. `/users/1234567/posts`). Legitimate numeric template
///   placeholders use `{id}` instead.
///
/// Templates like `/v1/groups/{group_id}/members/{user_id}` pass because
/// curly braces are never present in concrete paths.
#[cfg(debug_assertions)]
pub fn debug_assert_route_template(route: &str) {
    for seg in route.split('/') {
        if seg.is_empty() {
            continue;
        }
        if is_uuid_like(seg) {
            panic!(
                "cardinality guard: route label contains a UUID-looking segment '{seg}' \
                 in '{route}' — pass a template like '/api/sessions/{{id}}/messages' instead. \
                 See crates/garraia-telemetry/src/metrics.rs docblock."
            );
        }
        if seg.len() >= 6 && seg.chars().all(|c| c.is_ascii_digit()) {
            panic!(
                "cardinality guard: route label contains a long numeric segment '{seg}' \
                 in '{route}' — pass a template like '/users/{{id}}' instead. \
                 See crates/garraia-telemetry/src/metrics.rs docblock."
            );
        }
    }
}

#[cfg(not(debug_assertions))]
#[inline(always)]
pub fn debug_assert_route_template(_route: &str) {}

#[cfg(debug_assertions)]
fn is_uuid_like(s: &str) -> bool {
    // Canonical UUID: 36 chars, 4 dashes at 8/13/18/23, hex elsewhere.
    if s.len() != 36 {
        return false;
    }
    let bytes = s.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        let is_dash_pos = matches!(i, 8 | 13 | 18 | 23);
        if is_dash_pos {
            if b != b'-' {
                return false;
            }
        } else if !b.is_ascii_hexdigit() {
            return false;
        }
    }
    true
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

    // Plan 0025 (GAR-411 M1) cardinality guard tests.
    // These run only in debug builds because the guard is a no-op in release.

    #[cfg(debug_assertions)]
    #[test]
    fn cardinality_guard_accepts_valid_templates() {
        // Literal braces are the templating sigil — never panic.
        debug_assert_route_template("/api/sessions/{id}/messages");
        debug_assert_route_template("/v1/groups/{group_id}/members/{user_id}");
        debug_assert_route_template("/health");
        debug_assert_route_template("/metrics");
        debug_assert_route_template("/");
        debug_assert_route_template("/api/v1/admin/config");
        // Short numeric (< 6 chars) is allowed — version numbers etc.
        debug_assert_route_template("/api/v2/users/{id}");
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "UUID-looking segment")]
    fn cardinality_guard_rejects_uuid_segment() {
        debug_assert_route_template(
            "/api/sessions/8f2c7e1a-1234-4abc-9def-0123456789ab/messages",
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "numeric segment")]
    fn cardinality_guard_rejects_long_numeric_segment() {
        debug_assert_route_template("/users/1234567/posts");
    }

    #[cfg(debug_assertions)]
    #[test]
    fn cardinality_guard_numeric_boundary_5_chars_passes() {
        // Threshold is `len >= 6`: 5-char numeric is allowed (e.g. API
        // version numbers, short internal IDs that are not per-entity).
        debug_assert_route_template("/api/12345/status");
    }

    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "numeric segment")]
    fn cardinality_guard_numeric_boundary_6_chars_rejected() {
        // Exactly 6 chars is the cutoff for rejection.
        debug_assert_route_template("/users/123456");
    }

    #[cfg(debug_assertions)]
    #[test]
    fn is_uuid_like_matches_canonical_form_only() {
        assert!(is_uuid_like("8f2c7e1a-1234-4abc-9def-0123456789ab"));
        assert!(is_uuid_like("00000000-0000-0000-0000-000000000000"));
        // Too short
        assert!(!is_uuid_like("8f2c7e1a"));
        // Missing dashes
        assert!(!is_uuid_like("8f2c7e1a12344abc9def0123456789abcdef"));
        // Non-hex char
        assert!(!is_uuid_like("gggggggg-gggg-gggg-gggg-gggggggggggg"));
        // Wrong dash positions
        assert!(!is_uuid_like("8f2c7e1-a1234-4abc-9def-0123456789abc"));
    }
}
