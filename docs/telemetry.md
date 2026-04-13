# Telemetry (OpenTelemetry + Prometheus)

> Status: baseline implementation (GAR-384). Instrumentation is additive — the gateway runs fine with telemetry off.

## TL;DR

1. Start the observability stack:
   ```bash
   docker compose -f docker-compose.yml -f ops/compose.otel.yml up -d
   ```
2. Start the gateway with telemetry enabled:
   ```bash
   GARRAIA_OTEL_ENABLED=true \
   GARRAIA_METRICS_ENABLED=true \
   cargo run -p garraia-gateway
   ```
3. Open Jaeger at http://localhost:16686, select service `garraia-gateway`, and click "Find Traces" to see the latest trace.

---

## 1. Configuration

### Env vars

| Variable | Default | Purpose |
|---|---|---|
| `GARRAIA_OTEL_ENABLED` | `false` | Enable OTLP trace export |
| `GARRAIA_OTEL_EXPORTER_OTLP_ENDPOINT` | `http://localhost:4317` | gRPC endpoint for the OTLP collector |
| `GARRAIA_OTEL_SERVICE_NAME` | `garraia-gateway` | Service name shown in Jaeger |
| `GARRAIA_OTEL_SAMPLE_RATIO` | `1.0` | Fraction of traces to sample (0.0–1.0) |
| `GARRAIA_METRICS_ENABLED` | `false` | Enable Prometheus metrics scrape endpoint |
| `GARRAIA_METRICS_BIND` | `127.0.0.1:9464` | Socket address for `/metrics` |

### .env example

```env
# Telemetry — enable for local dev / staging; off by default in prod
GARRAIA_OTEL_ENABLED=true
GARRAIA_OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317
GARRAIA_OTEL_SERVICE_NAME=garraia-gateway
GARRAIA_OTEL_SAMPLE_RATIO=1.0

GARRAIA_METRICS_ENABLED=true
GARRAIA_METRICS_BIND=127.0.0.1:9464
```

### Disabled by default

All variables default to a safe, zero-overhead state. A production binary with no telemetry env vars set emits no spans, makes no network connections to a collector, and exposes no metrics endpoint. Opt in explicitly per environment.

---

## 2. Running locally with Jaeger + Prometheus + Grafana

```bash
# Bring up Jaeger, Prometheus, and Grafana alongside the main compose services
docker compose -f docker-compose.yml -f ops/compose.otel.yml up -d
```

| UI | URL | Credentials |
|---|---|---|
| Jaeger | http://localhost:16686 | none |
| Prometheus | http://localhost:9090 | none |
| Grafana | http://localhost:3000 | admin / garraia |

**Sanity check:**

```bash
# Send a request to the gateway
curl -s http://localhost:3888/health

# Then open Jaeger, select service "garraia-gateway", and find the trace for
# the /health request — it should appear within a few seconds.
```

---

## 3. Reading a trace

**Finding your trace:**
Open Jaeger at http://localhost:16686, select `garraia-gateway` from the Service dropdown, and click "Find Traces". The most recent request appears at the top.

**Span hierarchy:**

```
http.request  [TraceLayer — root span, covers the full HTTP round trip]
  └─ AgentRuntime::process_message   [#[tracing::instrument] in garraia-agents]
       └─ SessionStore::append_message  [#[tracing::instrument] in garraia-db]
       └─ SessionStore::load_recent_messages
```

The root span comes from `garraia_telemetry::http_trace_layer()`. Nested spans are created automatically by `#[tracing::instrument]` on `AgentRuntime::process_message*`, `SessionStore::append_message*`, and `SessionStore::load_recent_messages`.

**Correlating logs with a trace:**

Every response includes an `x-request-id` header (added by `garraia_telemetry::request_id_layer()`). This ID appears in structured logs alongside the `trace_id`. Search Jaeger by trace ID or grep logs by request ID to correlate the two.

---

## 4. Adding a span to your code

### Pattern A: function-scoped span

Use `#[tracing::instrument]` for async functions. The span is correctly attached across `.await` points.

```rust
#[tracing::instrument(skip(self), fields(provider = %provider_name), err)]
async fn do_thing(&self, provider_name: &str) -> Result<()> {
    // Every await point inside here is covered by this span.
    Ok(())
}
```

### Pattern B: block-scoped span

Use a manual span when you need to instrument a specific block rather than a whole function.

```rust
let span = tracing::info_span!("my.operation", key = %value);
let _enter = span.enter();
// Work happens here, inside the span.
// Span closes when _enter is dropped.
```

**Guidance:**

- Use `skip(self)` or `skip_all` to prevent large or non-`Debug` arguments from being emitted as span fields.
- Never put secrets, API keys, or PII in `fields(...)`. See §5.
- For async code, always prefer Pattern A — `span.enter()` in Pattern B does not survive `.await`.

---

## 5. PII safety

- `garraia_telemetry::http_trace_layer()` excludes request and response headers from spans by default. Authorization headers, cookies, and other sensitive headers are never recorded.
- Do not add span fields with names such as: `password`, `secret`, `token`, `api_key`, `jwt`, `passphrase`, `authorization`, or `cookie`.
- To correlate a request without leaking credentials, use the `x-request-id` header value — it is safe to log and trace.

---

## 6. Metrics

When `GARRAIA_METRICS_ENABLED=true`, the gateway exposes four baseline metrics at `127.0.0.1:9464/metrics`:

| Metric | Type | Description |
|---|---|---|
| `garraia_requests_total{route, status}` | counter | Total HTTP requests by route and status code |
| `garraia_http_latency_seconds{route}` | histogram | Request duration in seconds, by route |
| `garraia_errors_total{kind}` | counter | Error count by error kind |
| `garraia_active_sessions` | gauge | Current number of active sessions |

Call sites use the helpers from `garraia_telemetry`:

```rust
garraia_telemetry::inc_requests(route, status);
garraia_telemetry::record_latency(route, elapsed_seconds);
garraia_telemetry::inc_errors(kind);
garraia_telemetry::set_active_sessions(n);
```

The `/metrics` endpoint binds to `127.0.0.1` only. To expose it to a remote Prometheus scraper, set `GARRAIA_METRICS_BIND` explicitly (e.g., `0.0.0.0:9464`) and ensure the network is appropriately restricted.

---

## 7. Sampling

- Default `GARRAIA_OTEL_SAMPLE_RATIO=1.0` records every trace — suitable for local development.
- Recommended for production: `0.1` (10%) or lower depending on request volume.
- The sampler is `TraceIdRatioBased`, which makes a consistent per-trace decision. All spans belonging to a sampled trace are recorded; no orphan spans are produced.

---

## 8. Troubleshooting

**No traces appear in Jaeger:**
Check that `GARRAIA_OTEL_ENABLED=true` is set. Verify the OTLP endpoint is reachable — `curl -v http://localhost:4317` will not return an HTTP response (gRPC), but the TCP connection should succeed. If it is refused, the collector is not running.

**"Telemetry init failed" on startup:**
The gateway continues without telemetry and logs the underlying error to stderr. Fix the configuration error and restart. The process does not abort.

**/metrics returns 404:**
`GARRAIA_METRICS_ENABLED=true` is required. Also confirm you are hitting `127.0.0.1:9464` and not `0.0.0.0:9464` — the listener binds to loopback by default.

---

## 9. Next steps (out of scope for GAR-384)

- Structured per-subsystem dashboards in Grafana (future issue).
- Alerting rules in Prometheus / Grafana.
- Cross-service trace propagation when a gateway-to-workspace-service boundary exists (Fase 3).
- Instrumentation of `garraia-channels` adapters (not yet covered).

---

## References

- `plan.md` (GAR-384) — specification for this implementation
- `ROADMAP.md` §2.3 — telemetry position in Fase 2
- [OpenTelemetry Rust docs](https://docs.rs/opentelemetry/0.26/)
- [tracing crate docs](https://docs.rs/tracing/)
- Linear: [GAR-384](https://linear.app/chatgpt25/issue/GAR-384)
