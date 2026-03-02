# GarraIA Documentation

Welcome to the GarraIA documentation. GarraIA is a secure, lightweight open-source framework for AI agents, written in Rust.

## Quick Links

- [Installation Guide](./installation.md)
- [Architecture Overview](./architecture.md)
- [Configuration Reference](./configuration.md)
- [Channel Integrations](./channels.md)
- [MCP Setup](./mcp.md)
- [Voice Mode](./voice.md)
- [Memory System](./memory.md)
- [Security](./security.md)
- [Continue Integration](./src/continue-modes.md)

## Features

### Core Features

- **Multi-channel support** - Telegram, Discord, Slack, WhatsApp, iMessage
- **15+ LLM providers** - Anthropic, OpenAI, Ollama, and 12+ OpenAI-compatible
- **Voice pipeline** - STT → LLM → TTS with Whisper, Chatterbox, Hibiki
- **Memory system** - SQLite with vector search, facts extraction
- **MCP support** - Model Context Protocol with stdio and HTTP transport
- **Multi-agent** - A2A protocol, agent registry, priority routing
- **Security** - AES-256-GCM encrypted credentials, allowlists

### Advanced Features

- **Runtime state machine** - Turn-based execution with retry
- **Skills system** - Markdown-based agent skills
- **Scheduling** - Cron, interval, one-time tasks
- **Plugins** - WASM sandbox for extensions
- **Media processing** - PDF and image handling
- **Admin console** - Web-based management UI

## Getting Started

1. Install GarraIA:
   ```bash
   curl -fsSL https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh | sh
   ```

2. Initialize:
   ```bash
   garraia init
   ```

3. Start:
   ```bash
   garraia start
   ```

For detailed instructions, see [Installation Guide](./installation.md).

## Why GarraIA?

| Feature | GarraIA | OpenClaw | ZeroClaw |
|---------|---------|----------|----------|
| Binary size | 16 MB | ~1.2 GB | ~25 MB |
| Memory (idle) | 13 MB | ~388 MB | ~20 MB |
| 100% local | ✅ | ❌ | ❌ |
| Memory system | ✅ (facts, vector) | ❌ | ❌ |
| Pre-built binaries | ✅ | N/A | ❌ |
| Hot-reload config | ✅ | ❌ | ❌ |

## Resources

- [GitHub Repository](https://github.com/michelbr84/GarraRUST)
- [Discord Community](https://discord.gg/aEXGq5cS)
- [Linear Roadmap](https://linear.app/chatgpt25/project/garraia-complete-roadmap-2026-ac242025/overview)
- [Report Issues](https://github.com/michelbr84/GarraRUST/issues)
