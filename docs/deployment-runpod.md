# Runpod Load Balancer Serverless deployment

> **Status:** GAR-603 — implementation slice added 2026-05-13; checklist
> reconciled 2026-05-24 from static code/docs evidence. Public Runpod smoke
> remains a follow-up.
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

## Fresh RunPod GPU pod — one-shot bootstrap

On a brand-new RunPod GPU pod (Ubuntu base image, no Garra installed), the
fastest path is:

```bash
# Install the binary (once)
curl -fsSL https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh | sh

# Configure interactively
garraia init

# Run in the foreground (logs visible)
garraia start
```

`garraia init` (plan 0126, PR-A) will:

1. Detect the GPU via `nvidia-smi`. **It does not install NVIDIA drivers
   or CUDA** — if `nvidia-smi` already works, the runtime is assumed
   usable. Pods spun up with one of RunPod's official PyTorch / CUDA
   images already satisfy this.
2. Offer to install Ollama and pull
   `hf.co/MaziyarPanahi/Qwen3-14B-GGUF:Q4_K_M`. Both prompts default to
   yes but require an explicit confirmation keystroke.
3. Detect the lack of systemd inside RunPod containers and fall back to
   a `nohup ollama serve >> ~/.garraia/ollama.log 2>&1 &` start, with
   the PID stamped at `~/.garraia/ollama.pid`.
4. Write `gateway.host: 0.0.0.0` and `port: 3888` (or the value of
   `PORT` when set) into `~/.config/garraia/config.yml` so the gateway
   binds to the pod's public interface from the first run.
5. Skip TTS/STT auto-install but write the endpoint defaults
   (`http://127.0.0.1:7860` for Chatterbox, `http://127.0.0.1:9090` for
   faster-whisper) and print the matching `pip install` commands.

PR-B (plan 0127, **planned, not yet shipped**) chains those two commands
behind a single `curl … | sh` so the whole flow becomes one line. Until
that lands, run them in sequence as above.

Skip toggles (relevant once PR-B lands but also honored by `garraia
init` today):

- `GARRAIA_BOOTSTRAP_LOCAL=0` — skip GPU/local-stack prompts.
- `GARRAIA_SKIP_INIT=1` / `GARRAIA_SKIP_START=1` — installer-only
  (PR-B).

## Future work

`PORT_HEALTH` currently must equal `PORT` because `/ping` is served from
the same listener as the rest of the gateway. If a future Runpod
deployment requires segregating the health port from the application port,
a dedicated lightweight health-only listener can be added behind a
`--health-port` flag. Tracked in the same Linear issue (GAR-603) as a
follow-up.
