# Security Policy

## Supported Versions

|Version|Supported|
|-|-|
|latest|Yes|

## Reporting a Vulnerability

**Please do NOT report security vulnerabilities via public GitHub issues.**

Instead, use one of the following:

* **GitHub Private Vulnerability Reporting (preferred):**
  https://github.com/michelbr84/GarraRUST/security/advisories/new
* **Email:** security@garraia.org

### What to include

* A description of the vulnerability and its potential impact
* Steps to reproduce the issue
* Any relevant logs, config, or code snippets (redact any real credentials)

### What to expect

* **Acknowledgement:** within 48 hours
* **Initial assessment:** within 5 business days
* **Patch timeline:** within 14 days for critical, 30 days for others
* **Credit:** reporters credited in release notes unless they prefer anonymity

## Security Features

GarraIA is built with security as a core requirement:

* AES-256-GCM encrypted credential vault (never plaintext on disk)
* Argon2id (RFC 9106, m=64 MiB, t=3, p=4) for new credentials; PBKDF2-HMAC-SHA256 lazy
  upgrade path for legacy hashes (transactional under `FOR NO KEY UPDATE`)
* JWT HS256 access tokens with explicit algorithm-confusion guards; opaque refresh
  tokens fingerprinted with a separate HMAC-SHA256 secret
* Multi-tenant isolation in `garraia-workspace`: 22 tables under FORCE RLS, plus
  dedicated `garraia_login` / `garraia_signup` Postgres roles fronted by typed
  `LoginPool` / `SignupPool` newtypes (raw `PgPool` access forbidden in auth paths)
* Authentication required by default on the WebSocket gateway and the REST `/v1/*`
  surface
* Per-channel user allowlists with pairing codes
* Prompt injection detection and input sanitization
* WASM sandboxing for optional plugins
* Localhost-only binding by default (127.0.0.1, not 0.0.0.0)
* SHA-256 verified self-updates
* GitHub secret-scanning push protection enabled on this repository

## Scope

**In scope:**

* garraia-security crate (credential vault, allowlists, pairing)
* garraia-auth crate (RBAC role/permission table, JWT, password hashing, refresh tokens)
* garraia-gateway crate (WebSocket, HTTP API, REST `/v1/*`, auth, tus upload endpoints)
* garraia-workspace crate (multi-tenant Postgres + pgvector schema, FORCE RLS, BYPASSRLS roles)
* garraia-storage crate (object storage trait, LocalFs / S3-compatible backends, tus 1.0)
* garraia-cli crate `migrate workspace` flow (SQLite → Postgres user/identity hash reassembly)
* install.sh and the self-update mechanism
* Prompt injection or sandbox escape in the agent runtime
* Credential leakage in any channel (Telegram, Discord, Slack, WhatsApp, iMessage)
* Plugin/WASM sandbox escapes
* Cross-tenant data exposure (RLS bypass, broken `Principal` extractor, missing
  `WITH CHECK` enforcement on writes)

**Out of scope:**

* Vulnerabilities in third-party dependencies (report upstream, then notify us)
* Issues requiring physical access to the host machine
* Denial of service requiring high traffic volume

## Disclosure Policy

We follow responsible disclosure. Please:

1. Give us reasonable time to patch before public disclosure
2. Do not access or modify other users data
3. Do not degrade service availability during testing

We will not pursue legal action against researchers acting in good faith.

