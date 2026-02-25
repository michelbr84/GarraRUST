use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// Atomic counters for gateway-level metrics.
///
/// All fields use relaxed ordering since perfect precision is not required;
/// the counters are meant for human-readable dashboards and alerts.
#[derive(Debug, Default)]
pub struct Metrics {
    pub requests_total: AtomicU64,
    pub provider_requests_total: AtomicU64,
    pub provider_latency_sum_ms: AtomicU64,
    pub provider_latency_count: AtomicU64,
    pub tool_calls_total: AtomicU64,
    pub memory_operations_total: AtomicU64,
    pub active_sessions: AtomicU64,
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a completed LLM provider call with its latency in milliseconds.
    pub fn record_provider_call(&self, latency_ms: u64) {
        self.provider_requests_total.fetch_add(1, Ordering::Relaxed);
        self.provider_latency_sum_ms
            .fetch_add(latency_ms, Ordering::Relaxed);
        self.provider_latency_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a single tool invocation.
    pub fn record_tool_call(&self) {
        self.tool_calls_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a memory store operation (read or write).
    pub fn record_memory_op(&self) {
        self.memory_operations_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment the active-session gauge.
    pub fn inc_sessions(&self) {
        self.active_sessions.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement the active-session gauge.
    pub fn dec_sessions(&self) {
        self.active_sessions.fetch_sub(1, Ordering::Relaxed);
    }

    /// Render all metrics in Prometheus text exposition format.
    pub fn render_prometheus(&self) -> String {
        let requests = self.requests_total.load(Ordering::Relaxed);
        let provider_requests = self.provider_requests_total.load(Ordering::Relaxed);
        let latency_sum = self.provider_latency_sum_ms.load(Ordering::Relaxed);
        let latency_count = self.provider_latency_count.load(Ordering::Relaxed);
        let tool_calls = self.tool_calls_total.load(Ordering::Relaxed);
        let memory_ops = self.memory_operations_total.load(Ordering::Relaxed);
        let sessions = self.active_sessions.load(Ordering::Relaxed);

        format!(
            "\
# HELP garraia_requests_total Total HTTP requests
# TYPE garraia_requests_total counter
garraia_requests_total {requests}
# HELP garraia_provider_requests_total Total LLM provider calls
# TYPE garraia_provider_requests_total counter
garraia_provider_requests_total {provider_requests}
# HELP garraia_provider_latency_sum_ms Sum of provider latencies in ms
# TYPE garraia_provider_latency_sum_ms counter
garraia_provider_latency_sum_ms {latency_sum}
# HELP garraia_provider_latency_count Count of provider latency observations
# TYPE garraia_provider_latency_count counter
garraia_provider_latency_count {latency_count}
# HELP garraia_tool_calls_total Total tool invocations
# TYPE garraia_tool_calls_total counter
garraia_tool_calls_total {tool_calls}
# HELP garraia_memory_operations_total Total memory store operations
# TYPE garraia_memory_operations_total counter
garraia_memory_operations_total {memory_ops}
# HELP garraia_active_sessions Current active sessions
# TYPE garraia_active_sessions gauge
garraia_active_sessions {sessions}
"
        )
    }
}

/// Global metrics instance shared across the gateway.
static GLOBAL_METRICS: std::sync::OnceLock<Arc<Metrics>> = std::sync::OnceLock::new();

/// Get (or lazily initialise) the global metrics singleton.
pub fn global_metrics() -> Arc<Metrics> {
    GLOBAL_METRICS
        .get_or_init(|| Arc::new(Metrics::new()))
        .clone()
}

// ---------------------------------------------------------------------------
// Prometheus endpoint
// ---------------------------------------------------------------------------

/// `GET /metrics` — returns metrics in Prometheus text exposition format.
pub async fn prometheus_metrics_handler() -> impl IntoResponse {
    let body = global_metrics().render_prometheus();
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/plain; version=0.0.4; charset=utf-8")
        .body(Body::from(body))
        .unwrap()
}

// ---------------------------------------------------------------------------
// Request-ID / Tenant-ID middleware
// ---------------------------------------------------------------------------

/// Axum middleware that:
/// 1. Generates a UUID `request_id` for each HTTP request.
/// 2. Extracts `tenant_id` from the `X-Tenant-Id` header (defaults to `"default"`).
/// 3. Creates a tracing span carrying both values so downstream handlers
///    automatically include them in log output.
pub async fn request_id_middleware(req: Request, next: Next) -> Response {
    let request_id = uuid::Uuid::new_v4().to_string();

    let tenant_id = req
        .headers()
        .get("X-Tenant-Id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("default")
        .to_string();

    let span = tracing::info_span!(
        "request",
        request_id = %request_id,
        tenant_id = %tenant_id,
    );

    let _guard = span.enter();

    next.run(req).await
}

// ---------------------------------------------------------------------------
// Structured logging middleware
// ---------------------------------------------------------------------------

/// Axum middleware that logs each request with structured fields:
/// method, path, status, latency_ms, request_id, tenant_id.
///
/// Also increments `requests_total` in the global metrics.
pub async fn structured_logging_middleware(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();

    let request_id = uuid::Uuid::new_v4().to_string();

    let tenant_id = req
        .headers()
        .get("X-Tenant-Id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("default")
        .to_string();

    global_metrics()
        .requests_total
        .fetch_add(1, Ordering::Relaxed);

    let start = Instant::now();
    let response = next.run(req).await;
    let latency_ms = start.elapsed().as_millis() as u64;

    let status = response.status().as_u16();

    tracing::info!(
        %method,
        %path,
        status,
        latency_ms,
        %request_id,
        %tenant_id,
        "request completed"
    );

    response
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_new_starts_at_zero() {
        let m = Metrics::new();
        assert_eq!(m.requests_total.load(Ordering::Relaxed), 0);
        assert_eq!(m.provider_requests_total.load(Ordering::Relaxed), 0);
        assert_eq!(m.provider_latency_sum_ms.load(Ordering::Relaxed), 0);
        assert_eq!(m.provider_latency_count.load(Ordering::Relaxed), 0);
        assert_eq!(m.tool_calls_total.load(Ordering::Relaxed), 0);
        assert_eq!(m.memory_operations_total.load(Ordering::Relaxed), 0);
        assert_eq!(m.active_sessions.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn record_provider_call_increments_counters() {
        let m = Metrics::new();
        m.record_provider_call(150);
        m.record_provider_call(250);

        assert_eq!(m.provider_requests_total.load(Ordering::Relaxed), 2);
        assert_eq!(m.provider_latency_sum_ms.load(Ordering::Relaxed), 400);
        assert_eq!(m.provider_latency_count.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn record_tool_call_increments() {
        let m = Metrics::new();
        m.record_tool_call();
        m.record_tool_call();
        m.record_tool_call();
        assert_eq!(m.tool_calls_total.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn record_memory_op_increments() {
        let m = Metrics::new();
        for _ in 0..5 {
            m.record_memory_op();
        }
        assert_eq!(m.memory_operations_total.load(Ordering::Relaxed), 5);
    }

    #[test]
    fn sessions_inc_dec() {
        let m = Metrics::new();
        m.inc_sessions();
        m.inc_sessions();
        assert_eq!(m.active_sessions.load(Ordering::Relaxed), 2);
        m.dec_sessions();
        assert_eq!(m.active_sessions.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn render_prometheus_format() {
        let m = Metrics::new();
        m.requests_total.store(42, Ordering::Relaxed);
        m.record_provider_call(100);
        m.record_tool_call();
        m.record_memory_op();
        m.inc_sessions();

        let output = m.render_prometheus();

        assert!(output.contains("garraia_requests_total 42"));
        assert!(output.contains("garraia_provider_requests_total 1"));
        assert!(output.contains("garraia_provider_latency_sum_ms 100"));
        assert!(output.contains("garraia_provider_latency_count 1"));
        assert!(output.contains("garraia_tool_calls_total 1"));
        assert!(output.contains("garraia_memory_operations_total 1"));
        assert!(output.contains("garraia_active_sessions 1"));
        assert!(output.contains("# HELP"));
        assert!(output.contains("# TYPE"));
    }

    #[test]
    fn render_prometheus_contains_all_metric_types() {
        let m = Metrics::new();
        let output = m.render_prometheus();

        let counters = [
            "garraia_requests_total",
            "garraia_provider_requests_total",
            "garraia_provider_latency_sum_ms",
            "garraia_provider_latency_count",
            "garraia_tool_calls_total",
            "garraia_memory_operations_total",
        ];
        for name in counters {
            assert!(
                output.contains(&format!("# TYPE {name} counter")),
                "missing counter type for {name}"
            );
        }
        assert!(output.contains("# TYPE garraia_active_sessions gauge"));
    }
}
