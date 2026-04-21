//! Integration tests for the dedicated `/metrics` listener spawned by
//! `garraia-gateway::metrics_exporter::spawn_dedicated_metrics_listener`
//! (plan 0024 / GAR-412).
//!
//! Each scenario exercises the end-to-end HTTP round trip — bind a real
//! `TcpListener` on an OS-assigned port (`:0`), then make a `reqwest`
//! call back to `local_addr`. The `test-helpers` feature gate mirrors
//! the other integration binaries (harness_smoke, rest_v1_*) so a plain
//! `cargo test -p garraia-gateway` (without the feature) skips this
//! binary instead of failing at link time.

use std::net::SocketAddr;
use std::time::Duration;

use garraia_gateway::metrics_auth::MetricsAuthConfig;
use garraia_gateway::metrics_exporter::{MetricsExporterError, spawn_dedicated_metrics_listener};
use garraia_telemetry::PrometheusHandle;
use metrics_exporter_prometheus::PrometheusBuilder;

/// Build a fresh `PrometheusHandle` without touching the global
/// recorder. Each test gets its own sandboxed handle so we can run
/// them in parallel without fighting over `INSTALL`.
fn fresh_handle() -> PrometheusHandle {
    PrometheusBuilder::new().build_recorder().handle()
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .expect("reqwest client builds")
}

// ─── Scenario 1 — dedicated, loopback, no auth: 200 ──────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn loopback_no_auth_ok() {
    let cfg = MetricsAuthConfig::default();
    let bind: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let addr = spawn_dedicated_metrics_listener(cfg, bind, fresh_handle())
        .await
        .expect("dev-mode loopback spawn must succeed");

    let resp = client()
        .get(format!("http://{addr}/metrics"))
        .send()
        .await
        .expect("GET /metrics should reach the dedicated listener");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let content_type = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(
        content_type.contains("text/plain") && content_type.contains("version=0.0.4"),
        "expected Prometheus exposition content-type, got {content_type}"
    );
}

// ─── Scenario 2 — dedicated, non-loopback, no auth: startup fail-closed ─────

#[tokio::test(flavor = "multi_thread")]
async fn dedicated_non_loopback_no_auth_startup_err() {
    let cfg = MetricsAuthConfig::default();
    let bind: SocketAddr = "0.0.0.0:0".parse().unwrap();
    let err = spawn_dedicated_metrics_listener(cfg, bind, fresh_handle())
        .await
        .expect_err("non-loopback + no auth must not spawn");
    assert!(
        matches!(err, MetricsExporterError::AuthNotConfigured),
        "expected AuthNotConfigured, got {err:?}"
    );
    // Nothing to tear down — the listener never bound a socket.
}

// ─── Scenario 3 — dedicated, token match: 200 ────────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn token_match_ok() {
    let cfg = MetricsAuthConfig::from_telemetry_raw(Some("dev-token"), &[]);
    let bind: SocketAddr = "0.0.0.0:0".parse().unwrap();
    let addr = spawn_dedicated_metrics_listener(cfg, bind, fresh_handle())
        .await
        .expect("token-auth'd 0.0.0.0 spawn should succeed");

    let loopback_url = format!("http://127.0.0.1:{}/metrics", addr.port());
    let resp = client()
        .get(&loopback_url)
        .bearer_auth("dev-token")
        .send()
        .await
        .expect("authorized GET /metrics should reach the listener");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
}

// ─── Scenario 4 — dedicated, token mismatch: 401 ────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn token_mismatch_401() {
    let cfg = MetricsAuthConfig::from_telemetry_raw(Some("dev-token"), &[]);
    let bind: SocketAddr = "0.0.0.0:0".parse().unwrap();
    let addr = spawn_dedicated_metrics_listener(cfg, bind, fresh_handle())
        .await
        .expect("token-auth'd spawn should succeed");

    let loopback_url = format!("http://127.0.0.1:{}/metrics", addr.port());
    let resp = client()
        .get(&loopback_url)
        .bearer_auth("WRONG-token")
        .send()
        .await
        .expect("GET /metrics should reach the listener");
    assert_eq!(resp.status(), reqwest::StatusCode::UNAUTHORIZED);
    let body = resp.text().await.unwrap_or_default();
    assert!(
        body.contains("invalid token"),
        "expected 'invalid token' in body, got: {body}"
    );
}

// ─── Scenario 5 — dedicated, allowlist match: 200 ───────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn allowlist_match_ok() {
    let cfg = MetricsAuthConfig::from_telemetry_raw(None, &["127.0.0.0/8".to_string()]);
    let bind: SocketAddr = "0.0.0.0:0".parse().unwrap();
    let addr = spawn_dedicated_metrics_listener(cfg, bind, fresh_handle())
        .await
        .expect("allowlist spawn should succeed");

    let loopback_url = format!("http://127.0.0.1:{}/metrics", addr.port());
    let resp = client()
        .get(&loopback_url)
        .send()
        .await
        .expect("GET /metrics from 127.0.0.1 should reach the listener");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
}

// ─── Scenario 6 — dedicated, allowlist miss: 403 ────────────────────────────

#[tokio::test(flavor = "multi_thread")]
async fn allowlist_miss_403() {
    let cfg = MetricsAuthConfig::from_telemetry_raw(None, &["10.0.0.0/8".to_string()]);
    let bind: SocketAddr = "0.0.0.0:0".parse().unwrap();
    let addr = spawn_dedicated_metrics_listener(cfg, bind, fresh_handle())
        .await
        .expect("allowlist spawn should succeed");

    let loopback_url = format!("http://127.0.0.1:{}/metrics", addr.port());
    let resp = client()
        .get(&loopback_url)
        .send()
        .await
        .expect("GET /metrics should reach the listener");
    assert_eq!(resp.status(), reqwest::StatusCode::FORBIDDEN);
    let body = resp.text().await.unwrap_or_default();
    assert!(
        body.contains("not allowed"),
        "expected 'not allowed' in body, got: {body}"
    );
}
