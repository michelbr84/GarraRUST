# Continue Configuration Templates

This guide provides configuration templates for integrating [Continue](https://continue.dev/) (VS Code extension) with GarraIA Gateway using agent modes.

## Overview

GarraIA provides an OpenAI-compatible API at `/v1/chat/completions` that works seamlessly with Continue. The gateway supports mode-based routing through the `X-Agent-Mode` header, allowing you to configure different behavioral profiles for different use cases.

## Supported HTTP Headers (GAR-234)

GarraIA supports the following custom headers for Continue integration:

| Header | Description | Example |
|--------|-------------|---------|
| `X-Agent-Mode` | Override agent mode for the session (auto, code, debug, ask, review, search, architect, orchestrator, edit) | `X-Agent-Mode: debug` |
| `X-Session-Id` | Provide a specific session ID for conversation continuity | `X-Session-Id: abc-123-def` |
| `X-Request-Id` | Request ID for tracing and debugging (generated if not provided) | `X-Request-Id: req-789` |
| `X-User-Id` | Explicit user identification | `X-User-Id: user-456` |
| `Authorization` | Bearer token for authentication | `Authorization: Bearer your-token` |

### Header Processing Priority

1. **X-Agent-Mode** - Sets the agent mode for the session (highest priority for mode override)
2. **X-Session-Id** - Uses provided session ID or creates new one
3. **X-Request-Id** - Used for request tracing; generated if not provided
4. **X-User-Id** - User identification (fallback if not in Authorization)
5. **Authorization** - Primary authentication method

### Example with All Headers

```bash
curl -X POST http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer your-auth-token" \
  -H "X-Agent-Mode: debug" \
  -H "X-Session-Id: my-session-123" \
  -H "X-Request-Id: req-abc-456" \
  -H "X-User-Id: developer-1" \
  -d '{
    "model": "gpt-4",
    "messages": [{"role": "user", "content": "Debug this error"}]
  }'
```

## Available Modes

| Mode | Description | Best For |
|------|-------------|----------|
| `auto` | Automatic routing based on message content | General purpose |
| `code` | Full implementation capabilities | Writing, refactoring, creating files |
| `debug` | Read-only analysis + bash for troubleshooting | Error investigation |
| `ask` | Question-answering only | Understanding code, explaining concepts |
| `review` | Code review mode (read-only) | PR reviews, code analysis |
| `search` | Search and inspection only | Finding patterns, exploring codebase |
| `architect` | Design and planning | Architecture decisions, roadmaps |
| `orchestrator` | Multi-step execution | Complex workflows |
| `edit` | Quick edits | Small modifications |

---

## Basic Configuration

### Minimum Required Setup

Add this to your Continue `config.json`:

```json
{
  "models": [
    {
      "name": "garraia",
      "provider": "openai",
      "apiBase": "http://localhost:3000/v1",
      "apiKey": "your-auth-token-here",
      "model": "gpt-4"
    }
  ]
}
```

### With Authentication

```json
{
  "models": [
    {
      "name": "garraia",
      "provider": "openai",
      "apiBase": "http://localhost:3000/v1",
      "headers": {
        "Authorization": "Bearer your-auth-token-here"
      },
      "model": "gpt-4"
    }
  ]
}
```

> **Note:** Replace `http://localhost:3000` with your gateway URL if running remotely.

---

## Mode-Specific Templates

### Auto Mode (Default)

Automatic routing based on message content. The system decides the best mode using heuristics.

```json
{
  "models": [
    {
      "name": "garraia-auto",
      "provider": "openai",
      "apiBase": "http://localhost:3000/v1",
      "apiKey": "your-auth-token",
      "model": "gpt-4",
      "headers": {
        "X-Agent-Mode": "auto"
      }
    }
  ]
}
```

**When to use:** General purpose development, when you want the system to handle routing automatically.

---

### Code Mode

Focused on implementation with full tool access (file read/write, bash, etc.).

```json
{
  "models": [
    {
      "name": "garraia-code",
      "provider": "openai",
      "apiBase": "http://localhost:3000/v1",
      "apiKey": "your-auth-token",
      "model": "gpt-4",
      "headers": {
        "X-Agent-Mode": "code"
      }
    }
  ]
}
```

**When to use:** Writing new code, refactoring, creating files, running commands.

---

### Debug Mode

Focused on troubleshooting with read-only access plus bash for running tests/commands.

```json
{
  "models": [
    {
      "name": "garraia-debug",
      "provider": "openai",
      "apiBase": "http://localhost:3000/v1",
      "apiKey": "your-auth-token",
      "model": "gpt-4",
      "headers": {
        "X-Agent-Mode": "debug"
      }
    }
  ]
}
```

**When to use:** Investigating errors, analyzing stack traces, running tests to reproduce issues.

---

### Ask Mode

Question-answering mode with limited or no tool access.

```json
{
  "models": [
    {
      "name": "garraia-ask",
      "provider": "openai",
      "apiBase": "http://localhost:3000/v1",
      "apiKey": "your-auth-token",
      "model": "gpt-4",
      "headers": {
        "X-Agent-Mode": "ask"
      }
    }
  ]
}
```

**When to use:** Understanding code, explaining concepts, answering questions about the codebase.

---

### Review Mode

Read-only code review mode.

```json
{
  "models": [
    {
      "name": "garraia-review",
      "provider": "openai",
      "apiBase": "http://localhost:3000/v1",
      "apiKey": "your-auth-token",
      "model": "gpt-4",
      "headers": {
        "X-Agent-Mode": "review"
      }
    }
  ]
}
```

**When to use:** PR reviews, analyzing code changes, security audits.

---

## Mode Override via Header

You can override the mode dynamically using the `X-Agent-Mode` header in individual requests:

```bash
# Force debug mode for a specific request
curl -X POST http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "X-Agent-Mode: debug" \
  -H "Authorization: Bearer your-auth-token" \
  -d '{
    "model": "gpt-4",
    "messages": [{"role": "user", "content": "Why is this function failing?"}]
  }'
```

### Header Precedence

Mode is resolved in this priority order:

1. **X-Agent-Mode header** (highest priority) - Override per-request
2. **Message prefix** - `mode: <mode>` or `/mode <mode>` in user message
3. **Session mode** - Set via `/api/mode/select`
4. **Channel default** - Per-channel preference
5. **Default mode** - `auto` for most channels, `ask` for Telegram

### Mode Prefix Fallback

You can also override the mode by including it in your message using the `mode:` prefix:

```bash
# Using mode prefix in message
curl -X POST http://localhost:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer your-auth-token" \
  -d '{
    "model": "gpt-4",
    "messages": [{"role": "user", "content": "mode: debug\\nWhy is this function failing?"}]
  }'
```

Supported prefix formats:
- `mode: debug` - Set mode inline with message
- `/mode ask` - Alternative slash syntax

---

## Complete Multi-Profile Configuration

Here's a comprehensive example with multiple profiles for different use cases:

```json
{
  "models": [
    {
      "name": "garraia-auto",
      "provider": "openai",
      "apiBase": "http://localhost:3000/v1",
      "apiKey": "your-auth-token",
      "model": "gpt-4",
      "headers": {
        "X-Agent-Mode": "auto"
      }
    },
    {
      "name": "garraia-code",
      "provider": "openai",
      "apiBase": "http://localhost:3000/v1",
      "apiKey": "your-auth-token",
      "model": "gpt-4",
      "headers": {
        "X-Agent-Mode": "code"
      }
    },
    {
      "name": "garraia-debug",
      "provider": "openai",
      "apiBase": "http://localhost:3000/v1",
      "apiKey": "your-auth-token",
      "model": "gpt-4",
      "headers": {
        "X-Agent-Mode": "debug"
      }
    },
    {
      "name": "garraia-review",
      "provider": "openai",
      "apiBase": "http://localhost:3000/v1",
      "apiKey": "your-auth-token",
      "model": "gpt-4",
      "headers": {
        "X-Agent-Mode": "review"
      }
    },
    {
      "name": "garraia-ask",
      "provider": "openai",
      "apiBase": "http://localhost:3000/v1",
      "apiKey": "your-auth-token",
      "model": "gpt-4",
      "headers": {
        "X-Agent-Mode": "ask"
      }
    }
  ],
  "modelMeta": [
    {
      "title": "Default",
      "model": "garraia-auto"
    }
  ]
}
```

---

## API Endpoints Reference

### Chat Completions

```
POST /v1/chat/completions
```

OpenAI-compatible endpoint with streaming support.

### Mode Management

```
GET  /api/modes              # List available modes
POST /api/mode/select        # Select mode for session
GET  /api/mode/current       # Get current mode
GET  /api/models             # List available models
```

---

## Compatible Models

GarraIA supports any OpenAI-compatible model. Common choices:

| Provider | Model | Notes |
|----------|-------|-------|
| OpenAI | `gpt-4`, `gpt-4o`, `gpt-3.5-turbo` | Requires API key |
| Anthropic | `claude-3-opus`, `claude-3-sonnet` | Via OpenRouter |
| Ollama | `llama2`, `codellama`, `mistral` | Local |
| DeepSeek | `deepseek-chat` | Cost-effective |
| Azure OpenAI | `gpt-4` | Enterprise |

> **Note:** Configure the model name in the `model` field of your Continue config. The actual model used depends on your gateway provider configuration.

---

## Environment Variables

If using environment variables in your configuration:

```json
{
  "models": [
    {
      "name": "garraia",
      "provider": "openai",
      "apiBase": "${GARRAIA_API_BASE}",
      "apiKey": "${GARRAIA_API_KEY}",
      "model": "${GARRAIA_MODEL:-gpt-4}",
      "headers": {
        "X-Agent-Mode": "auto"
      }
    }
  ]
}
```

---

## Troubleshooting

### Connection Refused

Ensure GarraIA gateway is running:

```bash
garraia start
# or
cargo run --release -p garraia-gateway
```

### Authentication Errors

Verify your API key matches the one configured in GarraIA:

```bash
# Check your config
cat config.yaml | grep -A5 api_key
```

### Mode Not Working

Test the mode header directly:

```bash
curl -v -H "X-Agent-Mode: debug" \
  http://localhost:3000/api/modes
```

### Model Not Found

List available models:

```bash
curl http://localhost:3000/v1/models
```

---

## See Also

- [Mode System Documentation](./modes.md) - Detailed mode configuration
- [Gateway Configuration](./configuration.md) - Gateway setup
- [API Reference](./architecture.md) - Full API documentation
