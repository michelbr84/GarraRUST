# Architecture Overview

GarraIA is built as a Rust workspace with 14 specialized crates, each responsible for a specific domain.

## Workspace Structure

```
crates/
├── garraia-cli/          # Command-line interface
├── garraia-gateway/      # HTTP/WebSocket gateway + admin console
├── garraia-config/       # Configuration management
├── garraia-channels/     # Channel integrations
├── garraia-agents/       # LLM providers + agent runtime
├── garraia-voice/        # Voice pipeline (STT/TTS)
├── garraia-tools/        # Tool trait + registry
├── garraia-runtime/      # State machine executor
├── garraia-db/           # SQLite memory + vector store
├── garraia-plugins/     # WASM plugin sandbox
├── garraia-media/        # PDF/image processing
├── garraia-security/     # Credential vault + auth
├── garraia-skills/       # Skill parser + installer
└── garraia-common/       # Shared types + errors
```

## Runtime Flow

The agent execution follows a state machine pattern:

```
┌─────────────────────────────────────────────────────────────┐
│                    RUNTIME STATE MACHINE                     │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│   ┌──────────┐      ┌──────────┐      ┌──────────┐      │
│   │   IDLE   │ ───▶ │ RUNNING  │ ───▶ │   DONE   │      │
│   └──────────┘      └──────────┘      └──────────┘      │
│        ▲                  │                   │              │
│        │                  ▼                   │              │
│        │            ┌──────────┐            │              │
│        └────────────│ ERROR    │────────────┘              │
│                     └──────────┘                           │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

### Turn Execution

Each turn follows:

1. **Receive** - Message input from channel
2. **Execute** - Run tools, call LLM
3. **Respond** - Stream response back to channel

## Voice Pipeline

```
┌─────────────────────────────────────────────────────────────┐
│                    VOICE PIPELINE E2E                        │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌────────┐    ┌────────┐    ┌────────┐    ┌────────┐   │
│  │ AUDIO  │───▶│  STT   │───▶│  LLM   │───▶│  TTS   │   │
│  │ INPUT  │    │Whisper │    │Provider│    │Chatterb│   │
│  └────────┘    └────────┘    └────────┘    │ Hibiki │   │
│                                               └────────┘   │
│                                                              │
│  STT: Whisper (local), OpenAI Whisper API                  │
│  TTS: Chatterbox (GPU), Hibiki (GPU), OpenAI TTS            │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

## Multi-Agent Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    MULTI-AGENT SYSTEM                        │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌─────────────────────────────────────────────────────┐   │
│  │              Agent Registry                          │   │
│  │  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐ │   │
│  │  │  Agent-A     │ │  Agent-B    │ │  Agent-C    │ │   │
│  │  │  (priority:1)│ │ (priority:2)│ │ (priority:3)│ │   │
│  │  └─────────────┘ └─────────────┘ └─────────────┘ │   │
│  └─────────────────────────────────────────────────────┘   │
│                         │                                    │
│                         ▼                                    │
│  ┌─────────────────────────────────────────────────────┐   │
│  │              Priority Router                         │   │
│  │  - Match user message to best agent                 │   │
│  │  - Maintain session continuity                       │   │
│  └─────────────────────────────────────────────────────┘   │
│                         │                                    │
│                         ▼                                    │
│  ┌─────────────────────────────────────────────────────┐   │
│  │              A2A Protocol                           │   │
│  │  - JSON-RPC 2.0 agent-to-agent communication       │   │
│  │  - Task submission and status updates              │   │
│  │  - Agent cards at /.well-known/agent.json          │   │
│  └─────────────────────────────────────────────────────┘   │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

## Memory System

```
┌─────────────────────────────────────────────────────────────┐
│                    MEMORY SYSTEM                             │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────────────────────────────────────────────────┐  │
│  │                  memory.db (SQLite + sqlite-vec)      │  │
│  │  ┌─────────────┐  ┌─────────────┐  ┌────────────┐ │  │
│  │  │ Conversations│  │ Vector Store│  │ Sessions   │ │  │
│  │  └─────────────┘  └─────────────┘  └────────────┘ │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                              │
│  ┌──────────────────────────────────────────────────────┐  │
│  │                  facts.json                           │  │
│  │  - LLM-extracted facts from conversations           │  │
│  │  - With context, timestamp, and source              │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                              │
│  ┌──────────────────────────────────────────────────────┐  │
│  │                  Embeddings (Ollama/local)           │  │
│  │  - Semantic search over facts and history           │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

## Security Architecture

```
┌─────────────────────────────────────────────────────────────┐
│                    SECURITY LAYERS                            │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Credential Vault (AES-256-GCM + PBKDF2-SHA256)     │  │
│  │  - API keys encrypted at rest                         │  │
│  │  - Keys: vault > config > environment variable      │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                              │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Authentication                                       │  │
│  │  - Pairing codes for WebSocket access               │  │
│  │  - Per-channel user allowlists                       │  │
│  │  - WebSocket API key authentication                  │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                              │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Input Validation                                     │  │
│  │  - Prompt injection detection (14 patterns)          │  │
│  │  - Path traversal prevention                         │  │
│  │  - SSRF blocking                                      │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                              │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  Rate Limiting                                        │  │
│  │  - HTTP rate limits (configurable)                   │  │
│  │  - WebSocket sliding window (30 msg/min)            │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                              │
│  ┌──────────────────────────────────────────────────────┐  │
│  │  WASM Sandbox                                         │  │
│  │  - Epoch deadlines                                    │  │
│  │  - Resource limits (memory, CPU)                     │  │
│  │  - Path traversal prevention after canonicalization   │  │
│  └──────────────────────────────────────────────────────┘  │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

## Data Flow

```
User Message → Channel Handler → Gateway → Agent Runtime → LLM
                    ↑                                    ↓
              Response ← WebSocket/Channel ← Stream Response ←┘
```

## Configuration Hot-Reload

1. User edits `~/.garraia/config.yml`
2. File watcher detects changes
3. Configuration is re-parsed
4. New settings applied without restart
