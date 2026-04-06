# Memory System

GarraIA has a comprehensive memory system that allows the agent to learn and remember information.

## Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    MEMORY SYSTEM                               │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌─────────────────┐  ┌─────────────────┐                  │
│  │   facts.json    │  │   memory.db     │                  │
│  │  (extracted)    │  │   (SQLite)      │                  │
│  └─────────────────┘  └─────────────────┘                  │
│          │                    │                             │
│          ▼                    ▼                             │
│  ┌─────────────────────────────────────────┐               │
│  │         Embeddings (Ollama/local)       │               │
│  └─────────────────────────────────────────┘               │
│                        │                                    │
│                        ▼                                    │
│         ┌─────────────────────────────┐                    │
│         │      Agent Context          │                    │
│         └─────────────────────────────┘                    │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

## Components

### facts.json

Automatically extracted facts from conversations:

```json
[
  {
    "fact": "User prefers responses in Portuguese",
    "context": "When asked about language preference",
    "source": "telegram:123456",
    "timestamp": "2026-02-27T10:00:00Z"
  }
]
```

### memory.db

SQLite database with:
- Conversation history
- Session data
- Vector search (sqlite-vec)

### Embeddings

Semantic search using:
- Ollama (local)
- OpenAI
- Cohere

## Configuration

```yaml
memory:
  enabled: true
  auto_extract: true        # Extract facts automatically
  extraction_interval: 5     # Minutes between extractions
  max_facts: 100           # Maximum facts to store

embeddings:
  provider: ollama
  model: nomic-embed-text
  base_url: "http://localhost:11434"
  dimension: 768
```

## CLI Commands

### List Facts

```bash
garraia memory list
```

### Search Facts

```bash
garraia memory search <query>
```

### Add Fact Manually

```bash
garraia memory add "User prefers short responses"
```

### Clear Memory

```bash
garraia memory clear
```

### Export Memory

```bash
garraia memory export
```

## How It Works

### Fact Extraction

1. After each conversation, LLM analyzes messages
2. Important facts are identified
3. Facts stored with context and source
4. Facts included in future prompts

### Semantic Search

1. User query converted to embedding
2. Similar facts found via vector search
3. Relevant facts added to context
4. Agent responds with context awareness

## Data Location

```
~/.garraia/
├── memoria/
│   ├── fatos.json          # Extracted facts
│   └── embeddings/         # Embedding cache
├── data/
│   ├── memory.db           # SQLite memory
│   └── sessions.db         # Session data
```

## Memory API

### Search

```bash
curl -X POST http://127.0.0.1:3888/api/memory/search \
  -H "Content-Type: application/json" \
  -d '{"query": "user preferences", "limit": 5}'
```

### Add

```bash
curl -X POST http://127.0.0.1:3888/api/memory \
  -H "Content-Type: application/json" \
  -d '{"fact": "User likes coffee", "context": "mentioned in chat"}'
```

## Disable Memory

To disable memory:

```yaml
memory:
  enabled: false
```

Or use CLI:

```bash
garraia memory disable
```
