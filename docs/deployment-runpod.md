# Runpod Load Balancer Serverless deployment

> **Status:** GAR-603 — implementation slice (added 2026-05-13).
> **Audience:** operators deploying GarraIA/GarraRUST to Runpod's
> Load Balancer Serverless runtime.

GarraIA targets Runpod's **Load Balancer Serverless** model — the container
runs a real HTTP server, and Runpod routes external traffic only to workers
whose `GET /ping` on `PORT_HEALTH` returns HTTP 200.

This is **not** the queue-based serverless model. The two are not interchangeable.

| | Load Balancer Serverless (this guide) | Queue-based Serverless |
|---|---|---|
| Container behavior | Long-running HTTP server | Pulls jobs from a managed queue |
| Health probe | `GET /ping` on `PORT_HEALTH` | Job-status polling |
| Public URL | `https://ENDPOINT_ID.api.runpod.ai/<route>` | n/a — request-response via Runpod API |
| Suits GarraIA | ✅ yes — gateway is HTTP-first | ❌ no — chat sessions need persistent connections |

## What the gateway exposes

Both routes are wired into the production router (`crates/garraia-gateway/src/router.rs`):

| Route | Status | Purpose | Cost |
|---|---|---|---|
| `GET /ping` | 200 `pong` | Runpod LB liveness probe | Stateless, no DB, no provider — instant |
| `GET /health` | 200 `ok` | Generic container healthcheck | Stateless, no DB, no provider — instant |
| `GET /api/health` | JSON | Per-provider health aggregation | Heavier — exercises provider clients |

`/ping` and `/health` are intentionally minimal so they survive any transient
backend issue. Use `/api/health` from dashboards or richer orchestrators that
want a structured per-provider view.

## Runpod endpoint settings

Configure the Load Balancer Serverless endpoint with:

| Setting | Value |
|---|---|
| Container image | Built from this repo's `Dockerfile` (`docker build -t garraia .`) |
| Internal HTTP port | `3888` |
| `PORT` env var | `3888` |
| `PORT_HEALTH` env var | `3888` (must equal `PORT` in this implementation) |
| Exposed HTTP port | `3888` |
| Public URL pattern | `https://ENDPOINT_ID.api.runpod.ai/<route>` (no `:3888` suffix) |

> The port `3888` is **internal to the container**. Runpod does not append it
> to the public URL. Hitting `https://ENDPOINT_ID.api.runpod.ai:3888/...`
> from outside the container will not work.

The `garra start` command honors `PORT` and `HOST` env vars (GAR-603); the
shipped `Dockerfile` `CMD` already passes `--host 0.0.0.0` so the default
`docker run` works without any env overrides. If Runpod injects `PORT` or
`HOST`, the binary picks them up automatically — explicit `--port` / `--host`
flags still win if you ever add them to the start command.

## Local Docker smoke test

Build and run the image locally to verify before pointing Runpod at it.

```bash
# Build
docker build -t garraia:local .

# Run, mapping container 3888 to host 3888.
# Pass an empty .env if you have no secrets to inject.
docker run --rm -p 3888:3888 \
    -e RUST_LOG=info \
    garraia:local

# In another shell — both should return HTTP 200.
curl -fsS http://localhost:3888/ping    # → "pong"
curl -fsS http://localhost:3888/health  # → "ok"
```

If both `curl` calls return 200, the container is exposing the same surface
that Runpod's Load Balancer will probe.

## Runpod public smoke test

Once the endpoint is up:

```bash
ENDPOINT_ID=k3d2h9xumk2r4o   # replace with your endpoint id
curl -fsS https://${ENDPOINT_ID}.api.runpod.ai/ping
# Expected: HTTP 200, body: pong
```

If `/ping` returns `400 {"detail":"timed out waiting for worker"}`, the
endpoint is reachable but the worker has not become healthy yet. Common causes:

- The container start command launched something other than `garra start`
  (e.g. an interactive REPL — does not bind a listener).
- `PORT` / `PORT_HEALTH` mismatch between the endpoint settings and what
  the container binds to.
- The image was built from a branch that predates GAR-603 and has no `/ping`
  route — `curl http://localhost:3888/ping` locally returns 404 in that case.

## Secret hygiene

- **Never** commit `.env`, Runpod API tokens, endpoint-specific bearer tokens,
  or LLM provider keys to the repository.
- Inject runtime secrets via Runpod's environment-variable UI or its secrets
  manager; the gateway picks them up via `garraia-config` (see
  [`docs/auth-config.md`](auth-config.md) for the precedence matrix).
- Logs run through `garraia-security::RedactingWriter`; if you add new
  secrets, register them with the redactor so they never reach
  stdout/stderr/logs.

## Future work

`PORT_HEALTH` currently must equal `PORT` because `/ping` is served from
the same listener as the rest of the gateway. If a future Runpod
deployment requires segregating the health port from the application port,
a dedicated lightweight health-only listener can be added behind a
`--health-port` flag. Tracked in the same Linear issue (GAR-603) as a
follow-up.
