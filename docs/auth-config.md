# Auth configuration — GarraIA Gateway

> Reference for the `[auth]` config block and the environment variables
> consumed by `garraia-config::AuthConfig` + `garraia-gateway::mobile_auth`.
> Delivered as **plan 0046 (GAR-379 slice 3)** on 2026-04-22 (America/New_York).

This page consolidates the precedence rules between environment variables,
the `[auth]` section of the config file, and the built-in defaults. Every
secret is **environment-only**: storing JWT / HMAC / metrics-token material
in the config file is intentionally vedado — commit-safe files cannot carry
signing key material. Only operational, non-secret knobs live in `[auth]`.

## TL;DR for operators

1. **Set `GARRAIA_JWT_SECRET`** (≥32 bytes, hex/base64 both fine). Generate
   with `openssl rand -hex 32`.
2. **Set `GARRAIA_REFRESH_HMAC_SECRET`** (≥32 bytes, distinct from the JWT
   secret). Generate with `openssl rand -hex 32`.
3. **Optional but recommended**: pin JWT access TTL and refresh TTL in the
   `[auth]` section of your config file (see §2).
4. **Never** add `jwt_secret = "..."`, `refresh_hmac_secret = "..."`, or
   `metrics_token = "..."` to `config.yml` / `config.toml`. They are not
   parsed and would be a committable secret leak.

---

## 1. Precedence matrix

| Field | Environment variable | Config file key | Default | Fail mode when missing |
|---|---|---|---|---|
| `jwt_secret` | `GARRAIA_JWT_SECRET` *(preferred)* <br> `GarraIA_VAULT_PASSPHRASE` *(legacy fallback)* | ❌ **not accepted** | ❌ | `503 Service Unavailable` from `/auth/*` and `/v1/auth/*` |
| `refresh_hmac_secret` | `GARRAIA_REFRESH_HMAC_SECRET` | ❌ **not accepted** | ❌ | `503` from `/v1/auth/*` refresh flow |
| `login_database_url` | `GARRAIA_LOGIN_DATABASE_URL` | ❌ **not accepted** | ❌ | `503` (fail-soft) |
| `signup_database_url` | `GARRAIA_SIGNUP_DATABASE_URL` | ❌ **not accepted** | ❌ | `503` (fail-soft) |
| `app_database_url` | `GARRAIA_APP_DATABASE_URL` (optional) | ❌ **not accepted** | — | `/v1/groups-style` → `503`; `/v1/me` still works |
| `metrics_token` | `GARRAIA_METRICS_TOKEN` | ❌ **not accepted** | — | Dedicated `/metrics` listener fails closed; embedded route → `503` for non-loopback |
| `metrics_allow_cidrs` | `GARRAIA_METRICS_ALLOW` (comma-separated CIDRs) | ❌ **not accepted** | — | Allowlist miss → `403` |
| `jwt_algorithm` | *(none)* | `[auth] jwt_algorithm = "HS256"` | `"HS256"` | `config check` Error |
| `access_token_ttl_secs` | *(none)* | `[auth] access_token_ttl_secs = 900` | `900` (15 min) | `config check` Error if outside `[60, 86400]` |
| `refresh_token_ttl_secs` | *(none)* | `[auth] refresh_token_ttl_secs = 604800` | `604800` (7 days) | `config check` Error if outside `[60, 2_592_000]` or `< access_token_ttl_secs` |
| `metrics_token_ttl_hint_secs` | *(none)* | `[auth] metrics_token_ttl_hint_secs = 0` | `0` (indefinite) | Documentation-only |

**Secrets are env-only by design (plan 0046 §5.1).** The loader rejects
attempts to smuggle secrets via the file by simply ignoring any unknown
key inside `[auth]` — a misspelled `jwt_secret = "..."` is parsed as a
no-op, not as an override.

---

## 2. `[auth]` section — YAML / TOML

### YAML (`config.yml`)

```yaml
auth:
  jwt_algorithm: "HS256"          # only HS256 is accepted today
  access_token_ttl_secs: 900      # 15 minutes
  refresh_token_ttl_secs: 604800  # 7 days
  metrics_token_ttl_hint_secs: 0  # 0 = indefinite (rotation hint)
```

### TOML (`config.toml`)

```toml
[auth]
jwt_algorithm = "HS256"
access_token_ttl_secs = 900
refresh_token_ttl_secs = 604800
metrics_token_ttl_hint_secs = 0
```

Both forms are equivalent. The file is never required — when absent the
defaults above apply automatically (all four fields are `#[serde(default)]`).

---

## 3. Environment variables

All secrets are consumed exclusively via `std::env::var` inside
`crates/garraia-config/src/auth.rs::AuthConfig::from_env`. The gateway
never reads these variables elsewhere (plan 0046 §4 grep invariants).

### 3.1 `GARRAIA_JWT_SECRET` *(preferred)*

Signing key for the HS256 JWT access token issued by `/auth/register`,
`/auth/login`, `/v1/auth/login`, `/v1/auth/refresh`, and the OAuth
callback. Must be **≥32 bytes** after UTF-8 decoding.

```bash
# Generate a 64-char hex secret (32 bytes of entropy):
openssl rand -hex 32
```

### 3.2 `GarraIA_VAULT_PASSPHRASE` *(legacy fallback)*

Accepted when `GARRAIA_JWT_SECRET` is unset. Preserved for zero-breaking
change in dev workflows that predate GAR-379. New deployments should
prefer `GARRAIA_JWT_SECRET`.

**Precedence:** if both are set, `GARRAIA_JWT_SECRET` wins (covered by
`AuthConfig::from_env` + unit test `from_env_prefers_jwt_secret_over_vault_passphrase`).

### 3.3 `GARRAIA_REFRESH_HMAC_SECRET`

HMAC-SHA256 key used by `garraia-auth::SessionStore` to hash opaque
refresh tokens. **Must be distinct** from `GARRAIA_JWT_SECRET` (the
reuse would turn a leaked JWT into a refresh forgery). ≥32 bytes.

### 3.4 `GARRAIA_LOGIN_DATABASE_URL` / `GARRAIA_SIGNUP_DATABASE_URL`

Postgres connection URLs for the `garraia_login` and `garraia_signup`
BYPASSRLS roles. Both required for the `/v1/auth/*` flow. See ADR 0005
for the rationale.

### 3.5 `GARRAIA_APP_DATABASE_URL` *(optional)*

Postgres URL for the `garraia_app` RLS-enforced role. When absent, only
`/v1/groups`-style write endpoints are disabled; the rest of `/v1/auth/*`
and `/v1/me` continue to work.

### 3.6 `GARRAIA_METRICS_TOKEN` / `GARRAIA_METRICS_ALLOW`

Bearer token and CIDR allowlist for the `/metrics` endpoint. Loaded
from `garraia-telemetry::TelemetryConfig::from_env` → wired through
`MetricsAuthConfig::from_telemetry_raw`. See `docs/telemetry.md` for
the full plan 0024 behavior.

---

## 4. Fail modes (plan 0046 §5.2)

When `AuthConfig::from_env` returns `Ok(None)` (any required env var
missing), the gateway boots in **fail-soft mode**: the main listener
comes up, non-auth routes serve normally, and auth routes respond
**`503 Service Unavailable`** with a stable JSON body:

```json
{"error": "auth not configured"}
```

This behavior applies to:

- `POST /auth/register` *(mobile legacy)*
- `POST /auth/login` *(mobile legacy)*
- `GET /me` *(mobile legacy — extractor fails closed)*
- `POST /v1/auth/login`
- `POST /v1/auth/signup`
- `POST /v1/auth/refresh`
- `POST /v1/auth/logout`
- OAuth callback (`/oauth/{provider}/callback`)

**Zero hardcoded fallback.** Prior to plan 0046 the legacy mobile flow
used `"garraia-insecure-default-jwt-secret-change-me"` as a dev fallback —
that string is gone from the codebase. Tokens signed with it will fail
verification immediately on upgrade.

---

## 5. `config check` integration

`garraia config check` (plan 0035 / GAR-379 slice 1) validates the
`[auth]` block and cross-checks it against the process environment:

- **Error** when `jwt_algorithm` is not in the accepted set (`HS256`).
- **Error** when `access_token_ttl_secs` is outside `[60, 86400]`.
- **Error** when `refresh_token_ttl_secs` is outside `[60, 2_592_000]`
  or smaller than `access_token_ttl_secs`.
- **Warning** when neither `GARRAIA_JWT_SECRET` nor
  `GarraIA_VAULT_PASSPHRASE` is set (auth flow will 503).
- **Warning** when the env secret is set **and** `[auth]` overrides are
  present — non-secret overrides apply but secrets remain env-only.

The JSON output of `config check --json` never contains secret values —
only presence flags (plan 0035 SEC-M-02).

---

## 6. Troubleshooting

### `/auth/login` returns 503 "auth not configured"

Cause: `GARRAIA_JWT_SECRET` and `GarraIA_VAULT_PASSPHRASE` are both
unset. Either one will unblock the endpoint:

```bash
export GARRAIA_JWT_SECRET=$(openssl rand -hex 32)
```

Run `garraia config check` to confirm the gateway now sees the variable.

### `/v1/auth/login` still returns 503 after setting `GARRAIA_JWT_SECRET`

Cause: one of the other required env vars is missing
(`GARRAIA_REFRESH_HMAC_SECRET`, `GARRAIA_LOGIN_DATABASE_URL`,
`GARRAIA_SIGNUP_DATABASE_URL`). `AuthConfig::from_env` is all-or-nothing
for these four. `config check` lists which ones are detected.

### Tokens issued before the upgrade suddenly fail with "invalid or expired token"

Cause: those tokens were signed with the old
`garraia-insecure-default-jwt-secret-change-me` fallback. They are no
longer verifiable. Users must re-authenticate.

### `config check` reports a warning about `[auth]` overrides

Cause: operator set both env secrets and custom non-secret fields in
`[auth]`. This is not an error — it's a reminder that the secrets
always come from env regardless of the file.

---

## 7. Cross-references

- ADR 0005 — identity provider architecture (BYPASSRLS roles).
- Plan 0010 — v1/auth/* endpoints.
- Plan 0011 — `AuthConfig` introduction.
- Plan 0024 — `/metrics` auth.
- Plan 0035 — `config check`.
- Plan 0036 — Argon2id lazy upgrade (removes PBKDF2 writes).
- Plan 0046 — **this slice**: JWT secret centralization + fail-closed.
