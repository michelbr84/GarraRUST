# Referência da CLI

O binário `garraia` (alias `garra`) concentra toda a operação. Fonte: [`crates/garraia-cli/src/main.rs`](https://github.com/michelbr84/GarraRUST/blob/main/crates/garraia-cli/src/main.rs).

## Comandos

| Comando | O que faz |
|---|---|
| `garra init` | Wizard interativo: provedor LLM, chave no cofre criptografado |
| `garra start` | Inicia o agente (`--daemon`, `--with-voice`, `--host`, `--port`) |
| `garra stop` / `restart` / `status` | Controle do processo |
| `garra chat` | Chat interativo no terminal |
| `garra ask "<pergunta>"` | Pergunta única, com `--json` e exit codes — [docs/cli-ask.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/cli-ask.md) |
| `garra mcp-server` | Expõe `garra_ask` como servidor MCP stdio (Claude Desktop/Code) — [docs/cli-mcp-server.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/cli-mcp-server.md) |
| `garra channel list` · `channel status <nome>` | Canais de chat configurados |
| `garra plugin list/install/remove/watch` | Plugins WASM (feature `plugins`) |
| `garra skill list/install/remove` | Skills do agente |
| `garra mcp list/inspect/resources/prompts` | Servidores MCP conectados |
| `garra migrate openclaw` | Importa dados do OpenClaw |
| `garra migrate workspace` | SQLite → Postgres multi-tenant (Group Workspace) |
| `garra config check [--json] [--strict]` | Valida a configuração; exit codes 0 / 2 / 65 |
| `garra glob test` | Testa padrões glob/ignore (`--mode bash`, `--debug-regex`, `--json`) — [semântica](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/glob-semantics.md) |
| `garra update` / `rollback` | Auto-atualização com SHA-256 / volta versão |
| `garra verify` | Pipeline local: fmt, clippy, test, flutter analyze, gitleaks (exit 0/2) |
| `garra max-power` | Modo agent-advanced nativo (ADR 0011) |

Para flags completas de qualquer comando: `garra <comando> --help`.
