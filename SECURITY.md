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
* Authentication required by default on the WebSocket gateway
* Per-channel user allowlists with pairing codes
* Prompt injection detection and input sanitization
* WASM sandboxing for optional plugins
* Localhost-only binding by default (127.0.0.1, not 0.0.0.0)
* SHA-256 verified self-updates

## Scope

**In scope:**

* garraia-security crate (credential vault, allowlists, pairing)
* garraia-gateway crate (WebSocket, HTTP API, auth)
* install.sh and the self-update mechanism
* Prompt injection or sandbox escape in the agent runtime
* Credential leakage in any channel (Telegram, Discord, Slack, WhatsApp, iMessage)
* Plugin/WASM sandbox escapes

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

