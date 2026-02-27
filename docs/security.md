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
