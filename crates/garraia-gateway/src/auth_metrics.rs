//! Auth flow metrics — Prometheus baseline (GAR-391c).
//!
//! Counters + histograms for the `/v1/auth/*` endpoints:
//!
//! | Metric | Type | Labels |
//! |---|---|---|
//! | `garraia_auth_login_total` | counter | `outcome` (bounded enum) |
//! | `garraia_auth_login_latency_seconds` | histogram | `outcome` |
//! | `garraia_auth_refresh_total` | counter | `outcome` |
//! | `garraia_auth_signup_total` | counter | `outcome` |
//!
//! ## Cardinality discipline
//!
//! `outcome` values are restricted to a fixed enum:
//! `success | failure_invalid_credentials | failure_account_inactive |
//! failure_internal | failure_unknown_hash | failure_duplicate_email`.
//!
//! No per-user / per-group / per-IP labels — those would create unbounded
//! cardinality. Per-user breakdowns belong in the audit_events trail.
//!
//! ## Telemetry feature gate
//!
//! When the `telemetry` feature is OFF (gateway built without OTel +
//! Prometheus), these functions become no-ops via `cfg(feature = "telemetry")`.
//! That keeps the call sites in `auth_routes.rs` unconditional.

#[cfg(feature = "telemetry")]
mod imp {
    use metrics::{counter, histogram};

    pub fn record_login(outcome: &'static str, latency_seconds: f64) {
        counter!("garraia_auth_login_total", "outcome" => outcome).increment(1);
        histogram!("garraia_auth_login_latency_seconds", "outcome" => outcome)
            .record(latency_seconds);
    }

    pub fn record_refresh(outcome: &'static str) {
        counter!("garraia_auth_refresh_total", "outcome" => outcome).increment(1);
    }

    pub fn record_signup(outcome: &'static str) {
        counter!("garraia_auth_signup_total", "outcome" => outcome).increment(1);
    }
}

#[cfg(not(feature = "telemetry"))]
mod imp {
    pub fn record_login(_outcome: &'static str, _latency_seconds: f64) {}
    pub fn record_refresh(_outcome: &'static str) {}
    pub fn record_signup(_outcome: &'static str) {}
}

pub use imp::{record_login, record_refresh, record_signup};
