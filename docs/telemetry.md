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
| `GARRAIA_METRICS_BIND` | `127.0.0.1:9464` | Socket address for the dedicated `/metrics` listener |
| `GARRAIA_METRICS_TOKEN` | *(unset)* | Bearer token required on `/metrics` (plan 0024 / GAR-412) |
| `GARRAIA_METRICS_ALLOW` | *(unset)* | Comma-separated CIDR allowlist for `/metrics` peers (plan 0024 / GAR-412) |

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

### 1.1 TLS for OTLP exports (production)

> ⚠️ **Plaintext by default.** `opentelemetry-otlp` uses `tonic` as the gRPC
> transport, which defaults to HTTP/2 **cleartext** (`http://`). Any span
> attribute — including `http.target`, `http.url`, request IDs, or custom
> fields you attach — traverses the wire unencrypted unless you configure
> TLS explicitly.

**Production recommendation:**

```env
# Force TLS by using the https:// scheme. tonic picks up the scheme
# automatically and negotiates TLS against the collector's cert.
GARRAIA_OTEL_EXPORTER_OTLP_ENDPOINT=https://otel.example.com:4317
```

Your collector (Jaeger-as-OTLP, OpenTelemetry Collector, Grafana Tempo, etc.)
must be configured to terminate TLS on that port. See the collector's docs
for serving TLS — typically a `tls:` block under the OTLP receiver with
`cert_file` / `key_file` paths or an ACME integration.

**Development / loopback:** `http://localhost:4317` is fine when the
collector runs on the same host — the traffic never leaves the local
network interface.

**Why no auto-TLS?** `opentelemetry-otlp` defers to the scheme declared in
the endpoint URL — `http://` negotiates cleartext, `https://` negotiates TLS
against the collector's cert. A "TLS by default unless explicitly disabled"
knob does not exist in the current `opentelemetry-otlp` API (0.14.x series,
built on `tonic 0.12`) and would conflict with service-mesh sidecar deploys
(where the sidecar terminates TLS and forwards cleartext on loopback). We
rely on this docs warning to keep the matrix simple. Revisit when
`opentelemetry-otlp` exposes a first-class TLS opt-in independent of the
endpoint scheme (tracked in GAR-411 M2 follow-up notes).

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

The `/metrics` endpoint binds to `127.0.0.1` only by default. To expose it to a remote Prometheus scraper, set `GARRAIA_METRICS_BIND` explicitly (e.g., `0.0.0.0:9464`) **and** configure either `GARRAIA_METRICS_TOKEN` or `GARRAIA_METRICS_ALLOW` per §6.1 below. Without one of those the dedicated listener refuses to spawn (fail-closed startup) and the embedded route returns `503` for every non-loopback caller.

## 6.1 Security (plan 0024 / GAR-412)

The gateway exposes `/metrics` on two surfaces:

- **Dedicated listener** — spawned when `GARRAIA_METRICS_ENABLED=true`. Serves `handle.render()` from the globally-installed `metrics-exporter-prometheus` recorder. Bound to `GARRAIA_METRICS_BIND` (default `127.0.0.1:9464`).
- **Embedded route** — `GET /metrics` on the main gateway listener (port `3888` by default). Serves the legacy `observability::Metrics` snapshot — always on, always behind the same auth middleware.

Both surfaces share one `metrics_auth_layer` (crate `garraia-gateway::metrics_auth`). The middleware enforces the following matrix:

| Peer | Token configured | Allowlist configured | Outcome |
|---|---|---|---|
| loopback (`127.0.0.1`, `::1`) | no | empty | **200** — dev ergonomics |
| loopback | yes | — | **200** only with `Authorization: Bearer <token>`; otherwise **401** |
| loopback | no | configured | **200** when loopback is in the allowlist; otherwise **403** |
| non-loopback | no | empty | **503** (`metrics: auth not configured`) — safety net |
| non-loopback | yes | — | **200** only with correct token; otherwise **401** |
| non-loopback | — | configured | **200** only when peer IP ∈ allowlist; otherwise **403** |

**Fail-closed semantics — 2 listeners, 2 layers (intentional):**

- **Dedicated listener** fails closed at **startup**: if `GARRAIA_METRICS_ENABLED=true` **and** the bind is non-loopback **and** neither token nor allowlist is configured, `spawn_dedicated_metrics_listener` returns `Err(MetricsExporterError::AuthNotConfigured)` and no socket is opened. The gateway main listener stays healthy — telemetry is fail-soft by GAR-384 contract.
- **Embedded route** fails closed at **runtime**: the main listener always binds (it serves `/v1/*`, `/admin/*`, `/auth/*`, `/ws`) but the middleware returns `503`/`401`/`403` for any caller that doesn't satisfy the matrix above.

**Deploy recommendations:**

| Scenario | Recommended config |
|---|---|
| Local dev | *(leave unset)* — loopback-only defaults |
| Prometheus scraper on same host | `GARRAIA_METRICS_BIND=127.0.0.1:9464` (no auth needed) |
| Scraper behind VPN / private LAN | `GARRAIA_METRICS_BIND=0.0.0.0:9464` + `GARRAIA_METRICS_ALLOW=10.0.0.0/8,192.168.0.0/16` |
| Scraper with internet path | `GARRAIA_METRICS_BIND=0.0.0.0:9464` + `GARRAIA_METRICS_TOKEN=<random hex>` (and TLS terminator in front) |
| Escape hatch for a legacy scraper | `GARRAIA_METRICS_ALLOW=0.0.0.0/0` — explicit opt-in to the pre-0024 behavior |

**Secret hygiene:** `GARRAIA_METRICS_TOKEN` is redacted in `TelemetryConfig`'s `Debug` impl. It is listed alongside `GARRAIA_JWT_SECRET` in `CLAUDE.md` regra #6 — never log its value, never commit it to configs.

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
`GARRAIA_METRICS_ENABLED=true` is required for the **dedicated** listener on `GARRAIA_METRICS_BIND`. The **embedded** `/metrics` route on the main listener (port `3888` by default) is always registered. Also confirm you are hitting `127.0.0.1:9464` and not `0.0.0.0:9464` — the dedicated listener binds to loopback by default.

**/metrics returns 401 (`metrics: invalid token`):**
`GARRAIA_METRICS_TOKEN` is configured on the gateway side and the request is missing `Authorization: Bearer <token>` or the value does not match. Fix the scraper config.

**/metrics returns 403 (`metrics: peer not allowed`):**
`GARRAIA_METRICS_ALLOW` is configured and the peer IP (the immediate TCP peer — not `X-Forwarded-For`) is not inside any CIDR on the allowlist. Add the scraper's IP or broaden the CIDR.

**/metrics returns 503 (`metrics: auth not configured`):**
The main listener is serving the embedded route but the peer is non-loopback and neither token nor allowlist is configured. Set one of `GARRAIA_METRICS_TOKEN` or `GARRAIA_METRICS_ALLOW` and restart, or proxy the scrape through loopback.

**Dedicated metrics subsystem refused to start (`metrics auth not configured for non-loopback bind; listener disabled`):**
`GARRAIA_METRICS_BIND` is non-loopback and neither `GARRAIA_METRICS_TOKEN` nor `GARRAIA_METRICS_ALLOW` is set — plan 0024's startup fail-closed check refuses to expose unauth'd metrics. Configure one and restart. The gateway main listener is unaffected.

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
