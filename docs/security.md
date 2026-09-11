# Security

GarraIA is designed with security as a core principle.

## Security Features

### Credential Vault

API keys and tokens are encrypted at rest:

- **Encryption**: AES-256-GCM
- **Key derivation**: PBKDF2-SHA256
- **Location**: `~/.garraia/credentials/vault.json`

```yaml
security:
  vault_password: "your-password"
```

Or use environment variable:
```bash
export GARRAIA_VAULT_PASSWORD="your-password"
```

### Authentication

#### Pairing Codes

WebSocket connections require pairing:

```bash
# Generate pairing code
garraia pair

# Or use /pair command in Telegram
/pair
```

#### Per-Channel Allowlists

Restrict access by user ID:

```yaml
channels:
  telegram:
    allowed_users:
      - 123456789
      - 987654321
```

#### Two-factor authentication (admin console)

The admin console supports TOTP as a second factor at login. Enrollment and
removal live under **Security** in the console; the second factor is only
required for accounts that turned it on.

There are no recovery codes yet. **If you lose the authenticator device and you
are the only administrator, the documented way back in is to clear the second
factor directly in the database** — it is the only step that does not need a
valid code:

```bash
# Stop the gateway first: the store holds the session map in memory.
# Default path below; if `data_dir` is set in the config, use that instead.
sqlite3 "$HOME/.garraia/data/admin.db" \
  "UPDATE admin_users SET totp_enabled = 0, totp_secret = NULL,
     totp_secret_enc = NULL, totp_secret_nonce = NULL
   WHERE username = 'YOUR_USERNAME';"
```

Then start the gateway again and sign in with your password.

Two things worth knowing before you turn it on:

- Turning 2FA **off** from inside the console requires a valid code. That is
  deliberate: it is exactly what someone holding only your password would try.
- Rotating the secret while 2FA is on is refused (`409`). Disable it with a
  valid code first — replacing the secret underneath you would leave your
  authenticator app pointing at the old one.

Both limits of the first version are now closed:

- The wrong-code lockout (5 codes in 15 minutes) is **durable**: it lives in
  the `totp_attempts` table of `admin.db`, not in process memory, so a
  gateway restart no longer clears it and two instances over the same
  database share one count (#1140). The trade is deliberate and it is the
  cheaper half: after five wrong codes you wait out the 15-minute window —
  restarting the gateway is no longer a shortcut, for you or for someone who
  has your password.
- The TOTP secret is stored **encrypted** (AES-256-GCM under the admin master
  key, the same key `admin/secrets.rs` uses for provider keys in the same
  file) — #1141. A database written before this change keeps the cleartext
  value until the next read, which encrypts it and clears the old column;
  the upgrade is forward-only and needs no operator step.

Encrypting the secret also creates a dependency that did not exist before, so
two operational notes come with it:

- **The secret now lives and dies with the admin master key.** A master-key
  rotation carries it along automatically — the re-key re-encrypts
  `admin_users.totp_secret_enc` in the same transaction as everything else — but
  *losing* the key is different. If `<config_dir>/admin/master.key` is deleted,
  truncated, or the vault passphrase changes, the secret stops decrypting and
  the console answers **HTTP 500 on login**, not "2FA disabled". That is
  deliberate (an unreadable second factor must never degrade into no second
  factor), and the way out is the same break-glass `UPDATE` documented above:
  clear the second factor in the database and enroll again. A `master.key` that
  exists but cannot be used is preserved as `master.key.unreadable` and logged
  loudly rather than silently replaced.
- **Locked out by the 15-minute window and do not want to disable 2FA?** Clear
  just the counter, with the gateway stopped:
  `sqlite3 "$HOME/.garraia/data/admin.db" "DELETE FROM totp_attempts;"`

Two things this does **not** change, worth being explicit about:

- `admin_sessions.token` is still stored in cleartext, so whoever reads
  `admin.db` still walks away with a live admin session without passing the
  second factor. What encryption removes is the *durable* compromise: the
  cleartext secret used to outlive session expiry and a password change.
  Protecting the database file is still the mitigation that matters — and
  the master key must not sit next to it.
- The **mobile** flow still stores its secret in cleartext
  (`mobile_users.totp_secret`, base32, unencrypted). Only the console side
  is encrypted today.
- There is still **no global rate limit on `POST /admin/api/login`**. The
  durable counter above bounds guesses at the *second* factor, per user, after
  a valid password; it does nothing about guessing the password itself, which
  remains unbounded (each attempt costs 600k PBKDF2 iterations, so it is also a
  cheap way to burn CPU). `/admin/api/recovery/*` already sits behind a rate
  limiter; the login route does not. This is the one piece of #1140 that the
  durable counter did **not** deliver.

### Input Validation

#### Prompt Injection Detection

14 pattern categories detected:

- System prompt extraction attempts
- Role manipulation
- Context injection
- And more...

Configuration:
```yaml
security:
  prompt_injection_detection:
    enabled: true
    block_threshold: 0.8
```

#### Path Traversal Prevention

File operations are sandboxed:
- Path canonicalization before access
- Directory traversal blocked

### Network Security

#### Localhost Binding

Gateway binds to `127.0.0.1` by default:

```yaml
gateway:
  host: "127.0.0.1"  # Not 0.0.0.0
```

#### Rate Limiting

HTTP and WebSocket rate limits:

```yaml
security:
  rate_limit:
    enabled: true
    http:
      requests_per_minute: 60
    websocket:
      messages_per_minute: 30
```

### WASM Sandbox

Plugins run in isolated sandbox:

```yaml
plugins:
  enabled: true
  sandbox:
    memory_limit_mb: 128
    cpu_time_limit_ms: 1000
```

Features:
- Epoch-based execution limits
- Resource constraints
- Filesystem access control

## Best Practices

### 1. Use Environment Variables

Don't store API keys in config files:

```yaml
llm:
  openai:
    # Use env var instead of hardcoding
    api_key: ""  # Resolved from OPENAI_API_KEY
```

### 2. Regular Updates

Keep GarraIA updated:

```bash
garraia update
```

### 3. Secure the Vault

- Use strong vault password
- Don't share password
- Rotate periodically

### 4. Limit Channels

Enable only needed channels:

```yaml
channels:
  telegram:
    enabled: true  # Only enable what you need
  # discord:
  #   enabled: false
```

### 5. Use Allowlists

Restrict access:

```yaml
channels:
  telegram:
    allowed_users:
      - your_user_id
```

## Audit

### Log Redaction

API keys are automatically redacted in logs:

```
# Before:
2026-02-27 10:00:00 [INFO] API call with key: sk-1234567890abcdef

# After:
2026-02-27 10:00:00 [INFO] API call with key: [REDACTED]
```

### Audit Logs

Admin console provides audit trail:

```bash
# Via API
curl http://127.0.0.1:3888/api/admin/audit
```

## Compliance

GarraIA helps with:

- **Data residency**: All data local
- **No telemetry**: No external data collection
- **Encryption**: At rest and in transit
- **Access control**: Per-channel allowlists

## Reporting Security Issues

See [SECURITY.md](../SECURITY.md) for vulnerability reporting.
