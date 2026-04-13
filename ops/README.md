# ops/ — Local Observability Stack

This folder contains a Docker Compose overlay that spins up a full telemetry
stack (Jaeger + Prometheus + Grafana) for local development of the GarraIA
gateway. It is intended for dev/debug only — do not ship to production.

## Running

From the repo root:

```bash
docker compose -f docker-compose.yml -f ops/compose.otel.yml up
```

## UIs

| Service    | URL                     | Credentials         |
| ---------- | ----------------------- | ------------------- |
| Jaeger     | http://localhost:16686  | —                   |
| Prometheus | http://localhost:9090   | —                   |
| Grafana    | http://localhost:3000   | `admin` / `garraia` |

Grafana is pre-provisioned with Prometheus (default) and Jaeger datasources.
Drop dashboard JSON files into `ops/grafana/provisioning/dashboards/` to have
them auto-loaded.

## Gateway wiring

The gateway exposes Prometheus metrics at `/metrics` on port `3888`. To enable
OTLP trace export to Jaeger, start the gateway with:

```bash
GARRAIA_OTEL_ENABLED=true \
GARRAIA_OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317 \
cargo run -p garraia-gateway
```

Prometheus scrapes the gateway via `host.docker.internal:3888` by default, so
the gateway can run on the host while the stack runs in Docker. If you
containerize the gateway on the same compose network, uncomment the
alternative target in `ops/prometheus.yml`.
