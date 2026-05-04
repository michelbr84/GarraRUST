# Configuration Reference

Complete reference for GarraIA configuration options.

## Configuration File Location

GarraIA looks for configuration in:
1. `./config.yml` (current directory)
2. `~/.garraia/config.yml` (home directory)

## Full Configuration Example

```yaml
# Gateway settings
gateway:
  host: "127.0.0.1"  # Bind address
  port: 3888         # HTTP/WebSocket port

# LLM Providers
llm:
  # Anthropic Claude
  claude:
    provider: anthropic
    model: claude-sonnet-4-5-20250929
    api_key: "sk-ant-..."

  # OpenAI
  openai:
    provider: openai
    model: gpt-4o
    api_key: "sk-..."
    base_url: "https://api.openai.com/v1"  # Optional custom endpoint

  # Azure OpenAI
  azure:
    provider: openai
    model: gpt-4o
    api_key: "your-azure-key"
    base_url: "https://your-resource.openai.azure.com/"
    
  # Ollama (local)
  ollama:
    provider: ollama
    model: llama3.1
    base_url: "http://localhost:11434"

  # OpenRouter
  openrouter:
    provider: openrouter
    model: openai/gpt-4o
    api_key: "sk-or-..."

  # Other OpenAI-compatible providers
  deepseek:
    provider: openai
    model: deepseek-chat
    base_url: "https://api.deepseek.com/v1"
    api_key: "your-key"

  mistral:
    provider: openai
    model: mistral-large-latest
    base_url: "https://api.mistral.ai/v1"
    api_key: "your-key"

# Channel Configuration
# Tokens should be provided via environment variables (TELEGRAM_BOT_TOKEN,
# DISCORD_BOT_TOKEN, etc.) rather than stored in this file.
# See: docs/src/guides/connect-telegram.md for the full precedence chain.
channels:
  telegram:
    type: telegram
    enabled: true
    # bot_token resolved from: vault → config → TELEGRAM_BOT_TOKEN env var
    
  discord:
    type: discord
    enabled: false
    # bot_token resolved from: vault → config → DISCORD_BOT_TOKEN env var
    # application_id: "123456789"
    
  slack:
    enabled: false
    bot_token: "xoxb-..."
    app_token: "xapp-..."
    
  whatsapp:
    enabled: false
    phone_number_id: "123456789"
    access_token: "YOUR_ACCESS_TOKEN"
    verify_token: "YOUR_VERIFY_TOKEN"
    webhook_verify: true

# Agent Configuration
agent:
  system_prompt: |
    You are GarraIA, a helpful AI assistant.
    You are running locally and respect user privacy.
  max_tokens: 4096
  max_context_tokens: 100000
  temperature: 0.7
  tools:
    - bash
    - file_read
    - file_write
    - web_fetch
    - web_search

# Multi-agent Configuration
agents:
  assistant:
    name: "General Assistant"
    priority: 1
    model: openai
    system_prompt: "You are a helpful general assistant."
    
  coder:
    name: "Code Expert"
    priority: 2
    model: openai
    system_prompt: "You are an expert programmer."

# Memory Configuration
memory:
  enabled: true
  auto_extract: true      # Extract facts automatically
  extraction_interval: 5   # Minutes between extractions
  max_facts: 100

# Embeddings Configuration
embeddings:
  provider: ollama
  model: nomic-embed-text
  base_url: "http://localhost:11434"
  dimension: 768

# Voice Configuration
voice:
  enabled: false
  tts_endpoint: "http://127.0.0.1:7860"
  stt_provider: whisper
  language: "pt"
  
# MCP Servers
mcp:
  filesystem:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    
  github:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-github"]
    env:
      GITHUB_TOKEN: "your-token"

# Timeouts (in seconds)
timeouts:
  llm:
    default_secs: 30
  tts:
    default_secs: 120
  mcp:
    default_secs: 60
  health:
    default_secs: 5

# Security
security:
  vault_password: "your-vault-password"  # Or set GARRAIA_VAULT_PASSWORD env
  pairing_code_length: 8
  rate_limit:
    enabled: true
    requests_per_minute: 60

# Logging
logging:
  level: "info"  # debug, info, warn, error
  format: "json"  # text, json
```

## Environment Variables

You can use environment variables for sensitive values:

```yaml
llm:
  openai:
    provider: openai
    model: gpt-4o
    # Resolved in order: config > env var
    api_key: "${OPENAI_API_KEY}"
```

Supported env var resolution:
- `${VAR_NAME}` - Uses environment variable
- Leave empty - Uses `GARRAIA_{PROVIDER}_API_KEY`

## Hot Reload

Configuration changes in `config.yml` are applied automatically:
1. Edit `~/.garraia/config.yml`
2. Changes detected within seconds
3. No restart required

## CLI Configuration

### Validate config

```bash
garraia config check
```

Options:
- `--json` — machine-readable JSON output
- `--strict` — treat warnings as errors (useful for CI)

## Advanced Options

### Custom Channels

```yaml
channels:
  custom:
    type: http
    endpoint: "http://localhost:8080/webhook"
    auth_header: "X-Custom-Auth"
```

### Scheduling

```yaml
scheduler:
  enabled: true
  timezone: "America/Sao_Paulo"
```

### Observability

```yaml
observability:
  tracing:
    enabled: true
    endpoint: "http://localhost:4318/v1/traces"
  metrics:
    enabled: true
    port: 9090
```
