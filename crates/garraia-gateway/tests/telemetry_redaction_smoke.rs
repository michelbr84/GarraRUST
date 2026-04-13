//! Smoke test proving that sensitive request headers (e.g. `Authorization`)
//! are NOT leaked into tracing output by the telemetry layers the gateway
//! installs on its routers.
//!
//! Our `garraia_telemetry::http_trace_layer()` configures the underlying
//! `TraceLayer` with `DefaultMakeSpan::new().include_headers(false)` — this
//! test is the regression guard for that configuration. If it ever flips to
//! `include_headers(true)` (or headers leak some other way), the bearer token
//! below will show up in captured tracing output and this test will fail.

use axum::{body::Body, http::Request, routing::get, Router};
use tower::ServiceExt;
use tracing_test::traced_test;

const SECRET_TOKEN: &str = "sekret-value-xyz";

#[traced_test]
#[tokio::test]
async fn authorization_header_is_not_logged() {
    // Layer order mirrors `garraia_gateway::router::apply_telemetry_layers`
    // so this test faithfully reproduces production middleware ordering.
    // tower reverses declaration on request ingress: request_id_layer runs
    // first (stamps x-request-id), then http_trace_layer (creates span),
    // then propagate_request_id_layer (surfaces id on response).
    let app: Router = Router::new()
        .route("/health", get(|| async { "ok" }))
        .layer(garraia_telemetry::propagate_request_id_layer())
        .layer(garraia_telemetry::http_trace_layer())
        .layer(garraia_telemetry::request_id_layer());

    let req = Request::builder()
        .method("GET")
        .uri("/health")
        .header("authorization", format!("Bearer {SECRET_TOKEN}"))
        .body(Body::empty())
        .expect("request builder");

    let response = app.oneshot(req).await.expect("oneshot");
    assert_eq!(response.status(), 200);

    // `logs_contain` is injected into the test scope by `#[traced_test]`.
    assert!(
        !logs_contain(SECRET_TOKEN),
        "authorization token leaked into tracing output — http_trace_layer \
         must not include request headers in spans"
    );
}
