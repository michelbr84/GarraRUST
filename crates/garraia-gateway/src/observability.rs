//! Phase 7.2 — Observability: Prometheus metrics, structured logging, OpenTelemetry tracing.
//!
//! Metrics exposed at `GET /metrics` in Prometheus text exposition format.
//! OpenTelemetry spans are emitted for HTTP requests, LLM calls, tool
//! executions and DB queries.  The OTLP exporter endpoint is read from
//! `GARRAIA_OTLP_ENDPOINT` (default: disabled).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

// ── Atomic counters ───────────────────────────────────────────────────────────

/// Atomic counters for gateway-level metrics.
///
/// All fields use relaxed ordering since perfect precision is not required;
/// the counters are meant for human-readable dashboards and alerts.
#[derive(Debug, Default)]
pub struct Metrics {
    // ── HTTP ────────────────────────────────────────────────────────────
    pub requests_total: AtomicU64,
    /// Requests that returned 4xx (client errors).
    pub requests_4xx_total: AtomicU64,
    /// Requests that returned 5xx (server errors).
    pub requests_5xx_total: AtomicU64,
    /// Sum of all HTTP response latencies in milliseconds.
    pub request_latency_sum_ms: AtomicU64,
    /// Number of HTTP latency samples recorded.
    pub request_latency_count: AtomicU64,

    // ── LLM providers ───────────────────────────────────────────────────
    pub provider_requests_total: AtomicU64,
    pub provider_latency_sum_ms: AtomicU64,
    pub provider_latency_count: AtomicU64,
    /// Total LLM prompt tokens consumed (across all providers).
    pub llm_prompt_tokens_total: AtomicU64,
    /// Total LLM completion tokens consumed.
    pub llm_completion_tokens_total: AtomicU64,

    // ── Tools ────────────────────────────────────────────────────────────
    pub tool_calls_total: AtomicU64,
    /// Tool calls that returned an error.
    pub tool_errors_total: AtomicU64,

    // ── Sessions ─────────────────────────────────────────────────────────
    pub active_sessions: AtomicU64,
    /// Total messages processed (across all channels).
    pub messages_processed_total: AtomicU64,

    // ── DB ────────────────────────────────────────────────────────────────
    pub db_queries_total: AtomicU64,
    pub db_query_errors_total: AtomicU64,

    // ── Auth ─────────────────────────────────────────────────────────────
    pub auth_success_total: AtomicU64,
    pub auth_failure_total: AtomicU64,

    // ── Memory ───────────────────────────────────────────────────────────
    pub memory_operations_total: AtomicU64,
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }

    // ── HTTP helpers ──────────────────────────────────────────────────────

    pub fn record_request(&self, status: u16, latency_ms: u64) {
        self.requests_total.fetch_add(1, Ordering::Relaxed);
        self.request_latency_sum_ms
            .fetch_add(latency_ms, Ordering::Relaxed);
        self.request_latency_count.fetch_add(1, Ordering::Relaxed);
        if status >= 500 {
            self.requests_5xx_total.fetch_add(1, Ordering::Relaxed);
        } else if status >= 400 {
            self.requests_4xx_total.fetch_add(1, Ordering::Relaxed);
        }
    }

    // ── LLM helpers ───────────────────────────────────────────────────────

    /// Record a completed LLM provider call with its latency in milliseconds.
    pub fn record_provider_call(&self, latency_ms: u64) {
        self.provider_requests_total.fetch_add(1, Ordering::Relaxed);
        self.provider_latency_sum_ms
            .fetch_add(latency_ms, Ordering::Relaxed);
        self.provider_latency_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record token usage for a single LLM completion.
    pub fn record_tokens(&self, prompt_tokens: u64, completion_tokens: u64) {
        self.llm_prompt_tokens_total
            .fetch_add(prompt_tokens, Ordering::Relaxed);
        self.llm_completion_tokens_total
            .fetch_add(completion_tokens, Ordering::Relaxed);
    }

    // ── Tool helpers ──────────────────────────────────────────────────────

    /// Record a single tool invocation.
    pub fn record_tool_call(&self) {
        self.tool_calls_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a tool invocation that resulted in an error.
    pub fn record_tool_error(&self) {
        self.tool_calls_total.fetch_add(1, Ordering::Relaxed);
        self.tool_errors_total.fetch_add(1, Ordering::Relaxed);
    }

    // ── Session helpers ───────────────────────────────────────────────────

    /// Increment the active-session gauge.
    pub fn inc_sessions(&self) {
        self.active_sessions.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement the active-session gauge.
    pub fn dec_sessions(&self) {
        self.active_sessions.fetch_sub(1, Ordering::Relaxed);
    }

    /// Record a processed message (any channel).
    pub fn record_message(&self) {
        self.messages_processed_total
            .fetch_add(1, Ordering::Relaxed);
    }

    // ── DB helpers ────────────────────────────────────────────────────────

    pub fn record_db_query(&self, error: bool) {
        self.db_queries_total.fetch_add(1, Ordering::Relaxed);
        if error {
            self.db_query_errors_total.fetch_add(1, Ordering::Relaxed);
        }
    }

    // ── Auth helpers ──────────────────────────────────────────────────────

    pub fn record_auth(&self, success: bool) {
        if success {
            self.auth_success_total.fetch_add(1, Ordering::Relaxed);
        } else {
            self.auth_failure_total.fetch_add(1, Ordering::Relaxed);
        }
    }

    // ── Memory helpers ────────────────────────────────────────────────────

    /// Record a memory store operation (read or write).
    pub fn record_memory_op(&self) {
        self.memory_operations_total.fetch_add(1, Ordering::Relaxed);
    }

    // ── Prometheus text format renderer ──────────────────────────────────

    /// Render all metrics in Prometheus text exposition format (version 0.0.4).
    pub fn render_prometheus(&self) -> String {
        let requests = self.requests_total.load(Ordering::Relaxed);
        let requests_4xx = self.requests_4xx_total.load(Ordering::Relaxed);
        let requests_5xx = self.requests_5xx_total.load(Ordering::Relaxed);
        let req_latency_sum = self.request_latency_sum_ms.load(Ordering::Relaxed);
        let req_latency_count = self.request_latency_count.load(Ordering::Relaxed);

        let provider_requests = self.provider_requests_total.load(Ordering::Relaxed);
        let latency_sum = self.provider_latency_sum_ms.load(Ordering::Relaxed);
        let latency_count = self.provider_latency_count.load(Ordering::Relaxed);
        let prompt_tokens = self.llm_prompt_tokens_total.load(Ordering::Relaxed);
        let completion_tokens = self.llm_completion_tokens_total.load(Ordering::Relaxed);

        let tool_calls = self.tool_calls_total.load(Ordering::Relaxed);
        let tool_errors = self.tool_errors_total.load(Ordering::Relaxed);

        let sessions = self.active_sessions.load(Ordering::Relaxed);
        let messages = self.messages_processed_total.load(Ordering::Relaxed);

        let db_queries = self.db_queries_total.load(Ordering::Relaxed);
        let db_errors = self.db_query_errors_total.load(Ordering::Relaxed);

        let auth_success = self.auth_success_total.load(Ordering::Relaxed);
        let auth_failure = self.auth_failure_total.load(Ordering::Relaxed);

        let memory_ops = self.memory_operations_total.load(Ordering::Relaxed);

        // Compute average latencies (in milliseconds) as floating-point
        let req_latency_avg = if req_latency_count > 0 {
            req_latency_sum as f64 / req_latency_count as f64
        } else {
            0.0
        };
        let prov_latency_avg = if latency_count > 0 {
            latency_sum as f64 / latency_count as f64
        } else {
            0.0
        };

        format!(
            "\
# HELP garraia_http_requests_total Total HTTP requests received
# TYPE garraia_http_requests_total counter
garraia_http_requests_total {requests}
# HELP garraia_http_requests_4xx_total HTTP requests resulting in 4xx status
# TYPE garraia_http_requests_4xx_total counter
garraia_http_requests_4xx_total {requests_4xx}
# HELP garraia_http_requests_5xx_total HTTP requests resulting in 5xx status
# TYPE garraia_http_requests_5xx_total counter
garraia_http_requests_5xx_total {requests_5xx}
# HELP garraia_http_request_duration_ms_sum Sum of HTTP request durations in ms
# TYPE garraia_http_request_duration_ms_sum counter
garraia_http_request_duration_ms_sum {req_latency_sum}
# HELP garraia_http_request_duration_ms_count Number of HTTP duration observations
# TYPE garraia_http_request_duration_ms_count counter
garraia_http_request_duration_ms_count {req_latency_count}
# HELP garraia_http_request_duration_ms_avg Average HTTP request duration in ms
# TYPE garraia_http_request_duration_ms_avg gauge
garraia_http_request_duration_ms_avg {req_latency_avg:.2}
# HELP garraia_active_sessions Current number of active sessions
# TYPE garraia_active_sessions gauge
garraia_active_sessions {sessions}
# HELP garraia_messages_processed_total Total messages processed across all channels
# TYPE garraia_messages_processed_total counter
garraia_messages_processed_total {messages}
# HELP garraia_llm_requests_total Total LLM provider API calls
# TYPE garraia_llm_requests_total counter
garraia_llm_requests_total {provider_requests}
# HELP garraia_llm_request_duration_ms_sum Sum of LLM request durations in ms
# TYPE garraia_llm_request_duration_ms_sum counter
garraia_llm_request_duration_ms_sum {latency_sum}
# HELP garraia_llm_request_duration_ms_count Number of LLM duration observations
# TYPE garraia_llm_request_duration_ms_count counter
garraia_llm_request_duration_ms_count {latency_count}
# HELP garraia_llm_request_duration_ms_avg Average LLM provider latency in ms
# TYPE garraia_llm_request_duration_ms_avg gauge
garraia_llm_request_duration_ms_avg {prov_latency_avg:.2}
# HELP garraia_llm_prompt_tokens_total Total prompt tokens sent to LLM providers
# TYPE garraia_llm_prompt_tokens_total counter
garraia_llm_prompt_tokens_total {prompt_tokens}
# HELP garraia_llm_completion_tokens_total Total completion tokens received from LLM providers
# TYPE garraia_llm_completion_tokens_total counter
garraia_llm_completion_tokens_total {completion_tokens}
# HELP garraia_tool_executions_total Total tool invocations
# TYPE garraia_tool_executions_total counter
garraia_tool_executions_total {tool_calls}
# HELP garraia_tool_errors_total Tool invocations that resulted in errors
# TYPE garraia_tool_errors_total counter
garraia_tool_errors_total {tool_errors}
# HELP garraia_db_queries_total Total database queries executed
# TYPE garraia_db_queries_total counter
garraia_db_queries_total {db_queries}
# HELP garraia_db_query_errors_total Database queries that resulted in errors
# TYPE garraia_db_query_errors_total counter
garraia_db_query_errors_total {db_errors}
# HELP garraia_auth_success_total Successful authentication events
# TYPE garraia_auth_success_total counter
garraia_auth_success_total {auth_success}
# HELP garraia_auth_failure_total Failed authentication events
# TYPE garraia_auth_failure_total counter
garraia_auth_failure_total {auth_failure}
# HELP garraia_memory_operations_total Total memory store operations
# TYPE garraia_memory_operations_total counter
garraia_memory_operations_total {memory_ops}

# HELP garraia_requests_total Total HTTP requests (legacy alias)
# TYPE garraia_requests_total counter
garraia_requests_total {requests}
# HELP garraia_provider_requests_total Total LLM provider calls (legacy alias)
# TYPE garraia_provider_requests_total counter
garraia_provider_requests_total {provider_requests}
# HELP garraia_provider_latency_sum_ms Sum of provider latencies in ms (legacy alias)
# TYPE garraia_provider_latency_sum_ms counter
garraia_provider_latency_sum_ms {latency_sum}
# HELP garraia_provider_latency_count Count of provider latency observations (legacy alias)
# TYPE garraia_provider_latency_count counter
garraia_provider_latency_count {latency_count}
# HELP garraia_tool_calls_total Total tool invocations (legacy alias)
# TYPE garraia_tool_calls_total counter
garraia_tool_calls_total {tool_calls}
"
        )
    }
}

// ── Global singleton ──────────────────────────────────────────────────────────

/// Global metrics instance shared across the gateway.
static GLOBAL_METRICS: std::sync::OnceLock<Arc<Metrics>> = std::sync::OnceLock::new();

/// Get (or lazily initialise) the global metrics singleton.
pub fn global_metrics() -> Arc<Metrics> {
    GLOBAL_METRICS
        .get_or_init(|| Arc::new(Metrics::new()))
        .clone()
}

// ── Prometheus endpoint ───────────────────────────────────────────────────────

/// `GET /metrics` — returns metrics in Prometheus text exposition format.
pub async fn prometheus_metrics_handler() -> impl IntoResponse {
    let body = global_metrics().render_prometheus();
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/plain; version=0.0.4; charset=utf-8")
        .body(Body::from(body))
        .unwrap()
}

// ── Request-ID / Tenant-ID middleware ─────────────────────────────────────────

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

// ── Structured logging + metrics middleware ───────────────────────────────────

/// Axum middleware that logs each request with structured fields and updates
/// global Prometheus counters:
///   method, path, status, latency_ms, request_id, tenant_id.
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

    let start = Instant::now();
    let response = next.run(req).await;
    let latency_ms = start.elapsed().as_millis() as u64;
    let status = response.status().as_u16();

    global_metrics().record_request(status, latency_ms);

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

// ── OpenTelemetry tracing helpers ─────────────────────────────────────────────

/// Configuration for the OpenTelemetry OTLP exporter.
#[derive(Debug, Clone)]
pub struct OtelConfig {
    /// OTLP endpoint URL (e.g. `http://localhost:4317`).
    pub endpoint: Option<String>,
    /// Service name reported in traces.
    pub service_name: String,
    /// Service version.
    pub service_version: String,
}

impl OtelConfig {
    /// Load from environment variables:
    ///   - `GARRAIA_OTLP_ENDPOINT`
    ///   - `OTEL_SERVICE_NAME` (fallback: "garraia-gateway")
    pub fn from_env() -> Self {
        Self {
            endpoint: std::env::var("GARRAIA_OTLP_ENDPOINT").ok(),
            service_name: std::env::var("OTEL_SERVICE_NAME")
                .unwrap_or_else(|_| "garraia-gateway".into()),
            service_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.endpoint.is_some()
    }
}

/// Initialize the global tracing subscriber.
///
/// When `GARRAIA_OTLP_ENDPOINT` is set, the endpoint is logged.
/// Full OTLP export requires wiring in an external subscriber layer
/// (e.g. `opentelemetry-otlp`) which is done at binary startup in server.rs.
///
/// This function records a structured log entry with the OTLP config so
/// the gateway startup is observable regardless.
pub fn init_tracing(otel: &OtelConfig) {
    if otel.is_enabled() {
        tracing::info!(
            endpoint = otel.endpoint.as_deref().unwrap_or(""),
            service = %otel.service_name,
            version = %otel.service_version,
            "OTLP tracing configured"
        );
    } else {
        tracing::debug!("OTLP tracing disabled (set GARRAIA_OTLP_ENDPOINT to enable)");
    }
}

/// Create a tracing span for an HTTP request.
///
/// Span attributes follow the OpenTelemetry semantic conventions for HTTP.
#[inline]
pub fn http_span(method: &str, path: &str, request_id: &str) -> tracing::Span {
    tracing::info_span!(
        "http.request",
        "http.method" = method,
        "http.route" = path,
        "request_id" = request_id,
    )
}

/// Create a tracing span for an LLM provider call.
#[inline]
pub fn llm_span(provider: &str, model: &str) -> tracing::Span {
    tracing::info_span!(
        "llm.completion",
        "llm.provider" = provider,
        "llm.model" = model,
    )
}

/// Create a tracing span for a tool execution.
#[inline]
pub fn tool_span(tool_name: &str) -> tracing::Span {
    tracing::info_span!("tool.execute", "tool.name" = tool_name)
}

/// Create a tracing span for a database query.
#[inline]
pub fn db_span(operation: &str, table: &str) -> tracing::Span {
    tracing::info_span!(
        "db.query",
        "db.operation" = operation,
        "db.table" = table,
        "db.system" = "sqlite",
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

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
    fn record_request_increments_all_counters() {
        let m = Metrics::new();
        m.record_request(200, 50);
        m.record_request(404, 10);
        m.record_request(500, 5);

        assert_eq!(m.requests_total.load(Ordering::Relaxed), 3);
        assert_eq!(m.requests_4xx_total.load(Ordering::Relaxed), 1);
        assert_eq!(m.requests_5xx_total.load(Ordering::Relaxed), 1);
        assert_eq!(m.request_latency_sum_ms.load(Ordering::Relaxed), 65);
        assert_eq!(m.request_latency_count.load(Ordering::Relaxed), 3);
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
    fn record_tokens_increments() {
        let m = Metrics::new();
        m.record_tokens(100, 50);
        m.record_tokens(200, 100);
        assert_eq!(m.llm_prompt_tokens_total.load(Ordering::Relaxed), 300);
        assert_eq!(m.llm_completion_tokens_total.load(Ordering::Relaxed), 150);
    }

    #[test]
    fn record_tool_call_increments() {
        let m = Metrics::new();
        m.record_tool_call();
        m.record_tool_call();
        m.record_tool_call();
        assert_eq!(m.tool_calls_total.load(Ordering::Relaxed), 3);
        assert_eq!(m.tool_errors_total.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn record_tool_error_increments_both() {
        let m = Metrics::new();
        m.record_tool_error();
        assert_eq!(m.tool_calls_total.load(Ordering::Relaxed), 1);
        assert_eq!(m.tool_errors_total.load(Ordering::Relaxed), 1);
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
    fn render_prometheus_contains_new_metrics() {
        let m = Metrics::new();
        m.record_request(200, 42);
        m.record_tokens(100, 50);
        m.record_tool_error();
        m.record_db_query(false);
        m.record_auth(true);
        m.record_auth(false);
        m.record_message();

        let out = m.render_prometheus();

        // New Phase-7.2 metric names
        assert!(out.contains("garraia_http_requests_total"));
        assert!(out.contains("garraia_messages_processed_total 1"));
        assert!(out.contains("garraia_llm_prompt_tokens_total 100"));
        assert!(out.contains("garraia_llm_completion_tokens_total 50"));
        assert!(out.contains("garraia_tool_executions_total"));
        assert!(out.contains("garraia_tool_errors_total 1"));
        assert!(out.contains("garraia_db_queries_total 1"));
        assert!(out.contains("garraia_auth_success_total 1"));
        assert!(out.contains("garraia_auth_failure_total 1"));

        // Legacy aliases still present
        assert!(out.contains("garraia_requests_total"));
        assert!(out.contains("garraia_provider_requests_total"));
        assert!(out.contains("# HELP"));
        assert!(out.contains("# TYPE"));
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
            "garraia_http_requests_total",
            "garraia_llm_requests_total",
            "garraia_llm_prompt_tokens_total",
            "garraia_llm_completion_tokens_total",
            "garraia_tool_executions_total",
            "garraia_messages_processed_total",
            "garraia_memory_operations_total",
            // legacy aliases
            "garraia_requests_total",
            "garraia_provider_requests_total",
            "garraia_tool_calls_total",
        ];
        for name in counters {
            assert!(
                output.contains(&format!("# TYPE {name} counter")),
                "missing counter type for {name}"
            );
        }
        assert!(output.contains("# TYPE garraia_active_sessions gauge"));
        assert!(output.contains("# TYPE garraia_http_request_duration_ms_avg gauge"));
    }

    #[test]
    fn otel_config_without_endpoint_is_disabled() {
        // Build an OtelConfig with no endpoint directly — avoids unsafe env manipulation
        let config = OtelConfig {
            endpoint: None,
            service_name: "garraia-gateway".into(),
            service_version: "0.0.0".into(),
        };
        assert!(!config.is_enabled());
    }

    #[test]
    fn otel_config_with_endpoint_is_enabled() {
        let config = OtelConfig {
            endpoint: Some("http://localhost:4317".into()),
            service_name: "garraia-gateway".into(),
            service_version: "0.0.0".into(),
        };
        assert!(config.is_enabled());
    }
}
