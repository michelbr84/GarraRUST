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
| `jwt_secret` | `GARRAIA_JWT_SECRET` *(preferred)* <br> `GarraIA_VAULT_PASSPHRASE` *(legacy fallback, deprecated)* <br> `GARRAIA_VAULT_PASSPHRASE` *(last fallback — issue #824)* | ❌ **not accepted** | ❌ | `503 Service Unavailable` from `/auth/*` and `/v1/auth/*` |
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

### 3.2 `GarraIA_VAULT_PASSPHRASE` *(legacy fallback, deprecated spelling)*

Accepted when `GARRAIA_JWT_SECRET` is unset. Preserved for zero-breaking
change in dev workflows that predate GAR-379. New deployments should
prefer `GARRAIA_JWT_SECRET`. `garraia config check` emits a deprecation
warning whenever this mixed-case spelling is present.

### 3.2.1 `GARRAIA_VAULT_PASSPHRASE` *(last fallback — issue #824)*

The canonical credential-vault passphrase (all-caps) doubles as the
**last** JWT-secret fallback. Before #824, exporting only this spelling
left `/auth/*` answering 503 while the near-identical mixed-case alias
above would have worked — a silent casing trap. The reverse also holds
now: the credential vault (`garraia-security`) accepts the mixed-case
alias as a fallback, with a deprecation warning at boot.

**Precedence:** `GARRAIA_JWT_SECRET` > `GarraIA_VAULT_PASSPHRASE` >
`GARRAIA_VAULT_PASSPHRASE`. The mixed-case alias stays ahead of the
all-caps spelling so deploys that set both with different values keep
signing JWTs with the same secret as before #824. Covered by unit tests
`from_env_prefers_jwt_secret_over_vault_passphrase`,
`from_env_accepts_canonical_vault_passphrase_as_last_fallback` and
`from_env_prefers_legacy_alias_over_canonical_vault_passphrase`.

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

**The legacy `/auth/*` surface is gated by the same wiring — there is no
SQLite-only auth mode.** `mobile_auth` reaches the signing secret
exclusively through `AppState::jwt_signing_secret()`, and that field is
populated only after **both** Postgres pools (`garraia_login` +
`garraia_signup`) connect and pass their role guard (`server.rs`,
all-`Ok` arm). Consequences:

- Setting only `GARRAIA_JWT_SECRET` unlocks **nothing** — `/auth/*` and
  `/v1/auth/*` keep answering 503.
- A reachable Postgres with the wrong role in the DSN (e.g. a superuser
  URL) fails the `SELECT current_user` guard, the gateway logs
  `garraia-auth wiring partially failed; /v1/auth/* will return 503`,
  and the legacy `/auth/*` stays down with it.
- Dev deployments that don't want Postgres must simply accept the 503s
  as the (correct, fail-closed) steady state.

---

## 5. `config check` integration

`garraia config check` (plan 0035 / GAR-379 slice 1) validates the
`[auth]` block and cross-checks it against the process environment:

- **Error** when `jwt_algorithm` is not in the accepted set (`HS256`).
- **Error** when `access_token_ttl_secs` is outside `[60, 86400]`.
- **Error** when `refresh_token_ttl_secs` is outside `[60, 2_592_000]`
  or smaller than `access_token_ttl_secs`.
- **Warning** when none of `GARRAIA_JWT_SECRET`,
  `GarraIA_VAULT_PASSPHRASE` or `GARRAIA_VAULT_PASSPHRASE` is set
  (auth flow will 503).
- **Warning** when a JWT secret env var **is** set but any of
  `GARRAIA_REFRESH_HMAC_SECRET`, `GARRAIA_LOGIN_DATABASE_URL`,
  `GARRAIA_SIGNUP_DATABASE_URL` is missing — `AuthConfig::from_env` is
  all-or-nothing, so `/auth/*` and `/v1/auth/*` still answer 503
  (previously this partial state passed `config check` clean).
- **Warning** (network section, field `gateway.host`) when the config
  file binds `0.0.0.0`/`::` with no `gateway.api_key` or TLS disabled.
  Note `garra start --host` / the `HOST` env var can override the file
  value at runtime — the finding reflects the file, not the live process.
- **Warning** when the env secret is set **and** `[auth]` overrides are
  present — non-secret overrides apply but secrets remain env-only.
- **Warning** (deprecation) whenever the mixed-case
  `GarraIA_VAULT_PASSPHRASE` spelling is present (issue #824).
- **Warning** when both passphrase spellings are set with **different**
  values — the vault prefers the all-caps one while the JWT fallback
  prefers the mixed-case one (presence/equality only; values are never
  emitted).

The JSON output of `config check --json` never contains secret values —
only presence flags (plan 0035 SEC-M-02).

---

## 6. Troubleshooting

### `/auth/login` returns 503 "auth not configured"

Cause: the auth wiring never came up. That takes **all four** required
env vars (`GARRAIA_JWT_SECRET` or a vault-passphrase fallback,
`GARRAIA_REFRESH_HMAC_SECRET`, `GARRAIA_LOGIN_DATABASE_URL`,
`GARRAIA_SIGNUP_DATABASE_URL`) *and* both Postgres pools connecting as
their exact roles (§4, §7). Start with the secrets:

```bash
export GARRAIA_JWT_SECRET=$(openssl rand -hex 32)
export GARRAIA_REFRESH_HMAC_SECRET=$(openssl rand -hex 32)
```

then provision the database per §7. Run `garraia config check` to
confirm which variables the gateway sees, and look for
`garraia-auth wired (login + signup pools + jwt)` in the boot log.

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

## 7. Provisioning the Postgres roles

End-to-end runbook to take a gateway from "all auth vars absent" to a
working `/v1/auth/*` + `/auth/*`. Until now this sequence only existed in
the test harness (`crates/garraia-auth/tests/common/harness.rs`) and as
prose in ADR 0005 §Production.

### 7.1 Generate the secrets

```bash
export GARRAIA_JWT_SECRET=$(openssl rand -hex 32)            # ≥32 bytes
export GARRAIA_REFRESH_HMAC_SECRET=$(openssl rand -hex 32)   # distinct from the JWT secret
export GARRAIA_UPLOAD_HMAC_SECRET=$(openssl rand -hex 32)    # tus commit integrity (optional in dev)
export GARRAIA_VAULT_PASSPHRASE=$(openssl rand -hex 32)      # credential vault
LOGIN_PW=$(openssl rand -hex 24); SIGNUP_PW=$(openssl rand -hex 24); APP_PW=$(openssl rand -hex 24)
```

Hex output is deliberate: it is ≥32 ASCII bytes and safe to embed in a
DSN without URL-encoding. `scripts/gen-auth-secrets.sh` prints this whole
block ready to paste. See also the top-level [`.env.example`](../.env.example).

### 7.2 Postgres with pgvector

The migrations require the `pgcrypto`, `citext` and `vector` extensions
(superuser to create). Plain `postgres:16-alpine` does **not** ship
pgvector and fails migration 005 — use the pgvector image:

```bash
docker run -d --name garraia-pg -p 127.0.0.1:5432:5432 \
  -e POSTGRES_PASSWORD=<superuser-pw> -e POSTGRES_DB=garraia_workspace \
  -v garraia_pg:/var/lib/postgresql/data pgvector/pgvector:pg16
```

### 7.3 Run the workspace migrations

There is no `garra db migrate`; run them out-of-band as a privileged
role (sqlx-cli, or `psql -f` on each file in lexicographic order):

```bash
cargo install sqlx-cli --no-default-features --features postgres,rustls
DATABASE_URL=postgres://postgres:<superuser-pw>@127.0.0.1:5432/garraia_workspace \
  sqlx migrate run --source crates/garraia-workspace/migrations
```

### 7.4 Promote the NOLOGIN roles

The migrations create `garraia_login` (BYPASSRLS, migration 008),
`garraia_signup` (BYPASSRLS, migration 010) and `garraia_app`
(RLS-enforced, migration 007) as NOLOGIN. Promote each with its own
password from your secret store (ADR 0005 §Production; `pg_hba.conf`
must use `scram-sha-256`, never `trust`):

```sql
ALTER ROLE garraia_login  WITH LOGIN PASSWORD '<LOGIN_PW>';
ALTER ROLE garraia_signup WITH LOGIN PASSWORD '<SIGNUP_PW>';
ALTER ROLE garraia_app    WITH LOGIN PASSWORD '<APP_PW>';
```

### 7.5 Build the DSNs

Each pool runs `SELECT current_user` at connect time and refuses any
other role — the DSN must authenticate **as** the role itself (a
superuser URL fails with `WrongRole`; `SET ROLE` is not supported):

```bash
export GARRAIA_LOGIN_DATABASE_URL="postgres://garraia_login:${LOGIN_PW}@127.0.0.1:5432/garraia_workspace"
export GARRAIA_SIGNUP_DATABASE_URL="postgres://garraia_signup:${SIGNUP_PW}@127.0.0.1:5432/garraia_workspace"
export GARRAIA_APP_DATABASE_URL="postgres://garraia_app:${APP_PW}@127.0.0.1:5432/garraia_workspace"
```

### 7.6 Persist and verify

For the systemd unit, write the variables to `/etc/garraia/env`
(`chown root:garraia` + `chmod 640` — the unit already loads it via
`EnvironmentFile`). Never commit a `.env`. Then restart and verify — a green `/health` proves
nothing about auth:

```bash
garraia config check --strict                      # expect exit 0
# journal/log: "garraia-auth wired (login + signup pools + jwt)"
curl -si -X POST http://127.0.0.1:3888/v1/auth/signup \
  -H 'Content-Type: application/json' \
  -d '{"email":"op@example.com","password":"<strong password>"}'   # 201/409, NOT 503
curl -si -X POST http://127.0.0.1:3888/v1/auth/login \
  -H 'Content-Type: application/json' \
  -d '{"email":"op@example.com","password":"<strong password>"}'   # 200/401, NOT 503
```

Keep the gateway bound to `127.0.0.1` and terminate TLS at a reverse
proxy for any external exposure (`docs/src/production-runbook.md` §2 —
release binaries do not carry the `tls` cargo feature).

---

## 8. Cross-references

- ADR 0005 — identity provider architecture (BYPASSRLS roles).
- Plan 0010 — v1/auth/* endpoints.
- Plan 0011 — `AuthConfig` introduction.
- Plan 0024 — `/metrics` auth.
- Plan 0035 — `config check`.
- Plan 0036 — Argon2id lazy upgrade (removes PBKDF2 writes).
- Plan 0046 — **this slice**: JWT secret centralization + fail-closed.
