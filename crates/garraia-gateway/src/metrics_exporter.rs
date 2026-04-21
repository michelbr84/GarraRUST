//! Dedicated Prometheus `/metrics` listener (plan 0024 / GAR-412).
//!
//! Spawned at gateway boot when `GARRAIA_METRICS_ENABLED=true`. Owns the
//! HTTP surface that `garraia-telemetry::PrometheusHandle` renders,
//! keeping the telemetry crate free of Axum/Tower at the metrics level.
//!
//! Fail-closed at **startup**: when `bind` is non-loopback and neither a
//! Bearer token nor an allowlist is configured, we refuse to bind the
//! socket and return `Err(MetricsExporterError::AuthNotConfigured)`.
//! The gateway main listener remains fail-soft — `spawn` errors are
//! logged and the main listener continues serving.

use std::net::SocketAddr;

use axum::Router;
use axum::body::Body;
use axum::http::{StatusCode, header};
use axum::middleware::from_fn_with_state;
use axum::response::Response;
use axum::routing::get;
use garraia_telemetry::PrometheusHandle;
use thiserror::Error;
use tokio::net::TcpListener;
use tracing::{error, info};

use crate::metrics_auth::{MetricsAuthConfig, metrics_auth_layer};

/// Failure modes of [`spawn_dedicated_metrics_listener`].
///
/// The `Err(AuthNotConfigured)` variant is the deliberate fail-closed
/// signal — caller logs and continues booting the gateway. Other
/// variants (`Bind`, `Io`) wrap operational failures.
#[derive(Debug, Error)]
pub enum MetricsExporterError {
    /// Non-loopback bind refused because neither Bearer token nor
    /// allowlist was configured. The listener was **not** spawned.
    #[error(
        "metrics auth not configured for non-loopback bind; listener disabled \
         (set GARRAIA_METRICS_TOKEN or GARRAIA_METRICS_ALLOW)"
    )]
    AuthNotConfigured,

    /// TCP bind failed (port in use, permission denied, invalid addr).
    #[error("metrics listener bind failed: {0}")]
    Bind(#[from] std::io::Error),
}

/// Wire the dedicated `/metrics` listener.
///
/// Order of operations:
///
/// 1. **Startup fail-closed check** — non-loopback bind without auth
///    returns `Err(AuthNotConfigured)` before touching the socket.
/// 2. Build an Axum `Router` with `GET /metrics` that renders
///    `handle.render()` as Prometheus exposition text.
/// 3. Apply the [`metrics_auth_layer`] middleware (runtime 401/403/503
///    for the same route — belt-and-braces with the startup check).
/// 4. `TcpListener::bind(bind).await?`.
/// 5. Log the bound address + auth mode (never the token value).
/// 6. `tokio::spawn` the server. Errors from `axum::serve` are swallowed
///    so the task does not poison the runtime on shutdown.
///
/// Returns the listener's resolved [`SocketAddr`] on success. This is
/// useful for integration tests that bind to port `0` and need to know
/// the OS-assigned port to connect back.
pub async fn spawn_dedicated_metrics_listener(
    cfg: MetricsAuthConfig,
    bind: SocketAddr,
    handle: PrometheusHandle,
) -> Result<SocketAddr, MetricsExporterError> {
    if !bind.ip().is_loopback() && cfg.is_unauthenticated() {
        error!(
            addr = %bind,
            "metrics auth not configured for non-loopback bind; listener disabled \
             (set GARRAIA_METRICS_TOKEN or GARRAIA_METRICS_ALLOW)"
        );
        return Err(MetricsExporterError::AuthNotConfigured);
    }

    let render_handle = handle.clone();
    let app = Router::new()
        .route(
            "/metrics",
            get(move || {
                let handle = render_handle.clone();
                async move {
                    Response::builder()
                        .status(StatusCode::OK)
                        .header(
                            header::CONTENT_TYPE,
                            "text/plain; version=0.0.4; charset=utf-8",
                        )
                        .body(Body::from(handle.render()))
                        .expect("building a static Prometheus response cannot fail")
                }
            }),
        )
        .layer(from_fn_with_state(cfg.clone(), metrics_auth_layer));

    let listener = TcpListener::bind(bind).await?;
    let local_addr = listener.local_addr()?;
    info!(
        addr = %local_addr,
        mode = cfg.describe_mode(),
        "metrics dedicated listener up"
    );

    tokio::spawn(async move {
        let service = app.into_make_service_with_connect_info::<std::net::SocketAddr>();
        if let Err(e) = axum::serve(listener, service).await {
            error!(error = %e, "metrics dedicated listener exited with error");
        }
    });

    Ok(local_addr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use metrics_exporter_prometheus::PrometheusBuilder;

    fn fresh_handle() -> PrometheusHandle {
        // `build_recorder()` builds a recorder without installing it
        // globally — safe for unit tests to call multiple times.
        let recorder = PrometheusBuilder::new().build_recorder();
        recorder.handle()
    }

    #[tokio::test]
    async fn non_loopback_without_auth_fails_closed() {
        let cfg = MetricsAuthConfig::default();
        let bind: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let err = spawn_dedicated_metrics_listener(cfg, bind, fresh_handle())
            .await
            .expect_err("non-loopback + no auth must fail closed");
        assert!(matches!(err, MetricsExporterError::AuthNotConfigured));
    }

    #[tokio::test]
    async fn loopback_without_auth_spawns_successfully() {
        let cfg = MetricsAuthConfig::default();
        let bind: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let addr = spawn_dedicated_metrics_listener(cfg, bind, fresh_handle())
            .await
            .expect("loopback dev mode should spawn");
        assert!(addr.port() != 0, "OS should have assigned a real port");
    }
}
