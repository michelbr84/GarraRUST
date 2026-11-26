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
  truncated, or the vault passphrase changes, the boot finds that the key it
  resolved opens none of the stored secrets and the whole console answers
  **HTTP 503** (#2013; see
  [Admin console answering 503](#admin-console-answering-503-master-key-unavailable)),
  never "2FA disabled". If only the 2FA seed is unreadable while other secrets
  still open, that login answers **HTTP 500**. Both are deliberate (an
  unreadable second factor must never degrade into no second factor). Going
  back to the original key is the fix; without it, the way out is the
  break-glass `UPDATE` documented above: clear the second factor in the
  database and enroll again.
- **A damaged or removed `kdf_params` row disables the console, not the
  gateway (#1992, #1999).** The console answers HTTP 503 and the stored
  secrets are left untouched; see
  [Admin console answering 503](#admin-console-answering-503-master-key-unavailable)
  below for the causes and the way back.
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

#### Admin console answering 503 (master key unavailable)

With `GARRAIA_ADMIN_KEY` or `GARRAIA_VAULT_PASSPHRASE` set, the admin master key
is derived from that passphrase and from the per-installation salt recorded in
the single row of the `kdf_params` table in `admin.db`; without either, it is
the random `<config_dir>/admin/master.key`. When the boot cannot
produce a key that is safe to use, the gateway still starts — chat, channels
and `/api/*` keep working — but `/admin` and everything under it answer
**HTTP 503** with `{"error": "admin_unavailable"}`. The reason is in the
gateway log, on an `ERROR` line that starts with `admin panel disabled`; the
HTTP response does not carry it.

These cases used to fall back to the pre-2026-08-29 shared salt and keep the
console running. That opened none of the existing secrets, and every secret the
console wrote from then on (provider keys, 2FA seeds) was encrypted under the
weaker key and became unreadable once the real parameters were read again. The
503 replaces that silent loss (#1992, #1999). The log line names one of these
causes:

- **The `kdf_params` row cannot be used**: unknown version, a salt that is not
  32 bytes of base64, an iteration count outside the range this code writes
  (600 000 to 10 000 000), or a column that cannot be read. The row is damaged
  or was edited. Restore `admin.db` from a backup taken while the console
  worked.
- **No KDF parameters are recorded, and the stored secrets could not be shown
  to be under the legacy key**: usually a secret does not decrypt with it,
  because the row was removed after the re-key or the passphrase changed
  (including a passphrase turned on over secrets that were encrypted under
  `<config_dir>/admin/master.key`). Restore the backup, or go back to the
  passphrase (or to `master.key`) the secrets were encrypted with. If the log
  shows a read error instead (database locked, I/O), restart once `admin.db`
  is readable; nothing was written.
- **Entropy failure**: the system random number generator failed while the
  salt or `master.key` was being generated. Fix the host and restart; no secret
  could be encrypted safely without it anyway.
- **The resolved key opens none of the stored secrets** (#2013): the
  `kdf_params` row is fine, but none of the stored admin secrets (provider
  keys, archived versions, 2FA seeds) decrypts with the key derived from it,
  or with a well-formed `master.key`. The usual cause is a passphrase changed
  after the secrets were encrypted, or a `master.key` replaced by another one.
  Set back the passphrase (or `master.key`) the secrets were encrypted with.
  The rule is "none opens", not "one does not": a single damaged row only
  fails its own read and does not take the console down. The flip side: in a
  store that already mixes two keys, a key that opens only the minority still
  passes, so when recovering, go back to the key that opens the provider keys,
  not just an old 2FA seed.
- **`master.key` is missing or the wrong length while secrets are stored**
  (#2013): with no passphrase set, the console used to mint a new random
  `master.key` here, which opened none of the stored secrets. It no longer
  does, and it leaves the file untouched. Restore `master.key` from a backup,
  or set again the passphrase the secrets were encrypted with. With no admin
  secret stored, a lost `master.key` is still replaced by a new one, since
  there is nothing under it to lose; a wrong-length file is kept as
  `master.key.unreadable` (or `.unreadable.N`, never overwriting an earlier
  copy).
- **`master.key` exists but cannot be read** (permissions, I/O): never
  replaced, stored secrets or not, since that is no proof it is damaged. Make
  it readable by the gateway's user (mode 0600) and restart.
- **A new `master.key` could not be written**: the console does not run on a
  key the next boot would not have. Make `<config_dir>/admin` writable and
  restart. The new file is created owner-only (0600) from the first byte and
  never replaces a file that appeared in the meantime.
- **The stored secrets could not be read** to check them against the key
  (database locked, I/O error): restart once `admin.db` is readable; nothing
  was written.

An installation with no admin secret stored yet boots as before, whatever the
passphrase: there is nothing to check the key against.

Do not "repair" the row by hand with made-up values: a key derived from
anything other than the recorded parameters opens none of the stored secrets.
Without a backup the stored admin secrets cannot be recovered, and the way back
is to start over. Either move `admin.db` aside (the next boot creates an empty
store, and the console is set up again from scratch), or keep the admin users
and clear only what was encrypted, with the gateway stopped, as in the 2FA
break-glass above:

```bash
sqlite3 "$HOME/.garraia/data/admin.db" \
  "DELETE FROM secret_versions; DELETE FROM secrets; DELETE FROM kdf_params;
   UPDATE admin_users SET totp_enabled = 0, totp_secret = NULL,
     totp_secret_enc = NULL, totp_secret_nonce = NULL;"
```

The next boot then re-keys an empty store and the console comes back. Each
admin signs in with their password, enrolls 2FA again, and re-enters the
provider keys.

One case stays fail-soft on purpose. If the migration away from the legacy salt
opens every stored secret but fails to write (disk full, database locked), the
console keeps working on the legacy key and the next boot retries. The data is
provably under that key, so nothing is at risk.

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

### Execution Profiles

Everything above describes the `standard` execution profile — the posture
for a shared machine. A second profile, `isolated-pod` (`execution.profile`
or `GARRAIA_EXECUTION_PROFILE`), declares that the process runs in a
disposable pod: the WhatsApp owner in a 1:1 chat gets the `code` floor
(`bash`, `file_write`, MCP tools, subagents) and the auto-provisioned
`filesystem` MCP server is rooted at `execution.pod_root`. The rule is
**full power inside the isolated pod; no implicit access outside the pod**:
the profile is never inferred from container markers, an invalid value
refuses to boot, groups and paired-only contacts never inherit it, and the
bash risky-command gate and file jail stay on. It does not isolate a mounted
host filesystem, the Docker socket, `--privileged` or host secrets — see
[`execution-profiles.md`](execution-profiles.md) and
[ADR 0024](adr/0024-perfis-de-execucao-isolated-pod.md).

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
