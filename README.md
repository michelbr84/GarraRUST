<p align="center">
  <img src="assets/logo.png" alt="GarraIA" width="280" />
</p>

<h1 align="center">GarraIA</h1>

<p align="center">
  <strong>O framework seguro e leve de código aberto para agentes de IA.</strong>
</p>

<p align="center">
  <a href="https://github.com/michelbr84/GarraRUST/actions"><img src="https://github.com/michelbr84/GarraRUST/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI"></a>
  <a href="https://github.com/michelbr84/GarraRUST/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="Licença: MIT"></a>
  <a href="https://github.com/michelbr84/GarraRUST/stargazers"><img src="https://img.shields.io/github/stars/michelbr84/GarraRUST" alt="Estrelas"></a>
  <a href="https://github.com/michelbr84/GarraRUST/issues"><img src="https://img.shields.io/github/issues/michelbr84/GarraRUST" alt="Issues"></a>
  <a href="https://github.com/michelbr84/GarraRUST/issues?q=label%3Agood-first-issue+is%3Aopen"><img src="https://img.shields.io/github/issues/michelbr84/GarraRUST/good-first-issue?color=7057ff&label=good%20first%20issues" alt="Boas Primeiras Issues"></a>
  <a href="https://discord.gg/aEXGq5cS"><img src="https://img.shields.io/badge/discord-join-5865F2?logo=discord&logoColor=white" alt="Discord"></a>
</p>

<p align="center">
  <a href="#início-rápido">Início Rápido</a> &middot;
  <a href="#por-que-garraia">Por que GarraIA?</a> &middot;
  <a href="#recursos">Recursos</a> &middot;
  <a href="#segurança">Segurança</a> &middot;
  <a href="#arquitetura">Arquitetura</a> &middot;
  <a href="#migrando-do-openclaw">Migrar do OpenClaw</a> &middot;
  <a href="#contribuindo">Contribuindo</a>
</p>

---

Um único binário de 16 MB que executa seus agentes de IA no Telegram, Discord, Slack, WhatsApp e iMessage - com armazenamento de credenciais criptografadas, recarregamento de configuração a quente e 13 MB de RAM em idle. Desenvolvido em Rust para a segurança e confiabilidade que os agentes de IA exigem.

<!-- TODO: Adicionar GIF de demonstração do terminal VHS aqui (#103) -->

## Início Rápido

```bash
# Instalar (Linux, macOS)
curl -fsSL https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh | sh

# Configuração interativa - escolha seu provedor de LLM, armazene chaves de API em cofre criptografado
garraia init

# Iniciar
garraia start
```

<details>
<summary>Compilar a partir do código-fonte</summary>

```bash
# Requer Rust 1.85+
cargo build --release
./target/release/garraia init
./target/release/garraia start

# Opcional: incluir suporte a plugins WASM
cargo build --release --features plugins
```
</details>

Binários pré-compilados para Linux (x86_64, aarch64), macOS (Intel, Apple Silicon) e Windows (x86_64) estão disponíveis nas [Versões do GitHub](https://github.com/michelbr84/GarraRUST/releases).

## Por que GarraIA?

### vs OpenClaw, ZeroClaw e outros frameworks de agentes de IA

| | **GarraIA** | **OpenClaw** (Node.js) | **ZeroClaw** (Rust) |
|---|---|---|---|
| **Tamanho do binário** | 16 MB | ~1.2 GB (com node_modules) | ~25 MB |
| **Memória em idle** | 13 MB | ~388 MB | ~20 MB |
| **Início a frio** | 3 ms | 13.9 s | ~50 ms |
| **Armazenamento de credenciais** | Cofre criptografado AES-256-GCM | Arquivo de configuração em texto puro | Arquivo de configuração em texto puro |
| **Autenticação padrão** | Habilitada (pareamento WebSocket) | Desabilitada por padrão | Desabilitada por padrão |
| **Agendamento** | Cron, intervalo, único | Sim | Não |
| **Roteamento multi-agente** | Planejado (#108) | Sim (agentId) | Não |
| **Orquestração de sessões** | Planejado (#108) | Sim | Não |
| **Suporte MCP** | Stdio | Stdio + HTTP | Stdio |
| **Canais** | 5 | 6+ | 4 |
| **Provedores de LLM** | 14 | 10+ | 22+ |
| **Binários pré-compilados** | Sim | N/A (Node.js) | Compilar a partir do código-fonte |
| **Recarregamento de config a quente** | Sim | Não | Não |
| **Sistema de plugins WASM** | Opcional (sandbox) | Não | Não |
| **Auto-atualização** | Sim (`garraia update`) | npm | Compilar a partir do código-fonte |

*Benchmarks medidos em um droplet DigitalOcean com 1 vCPU, 1 GB RAM. [Reproduza você mesmo](bench/).*

## Segurança

O GarraIA foi desenvolvido para os requisitos de segurança de agentes de IA que ficam sempre ativos, acessam dados privados e se comunicam externamente.

- **Cofre de credenciais criptografadas** - Chaves de API e tokens armazenados com criptografia AES-256-GCM em `~/.garraia/credentials/vault.json`. Nunca em texto puro no disco.
- **Autenticação por padrão** - Gateway WebSocket requer códigos de pareamento. Sem acesso não autenticado fora da caixa.
- **Listas de permissões por usuário** - Listas de permissões por canal controlam quem pode interagir com o agente. Mensagens não autorizadas são descartadas silenciosamente.
- **Detecção de injeção de prompt** - Validação e saneamento de entrada antes do conteúdo chegar ao LLM.
- **Sandbox WASM** - Plugin opcional em sandbox via runtime WebAssembly com acesso controlado ao host (compile com `--features plugins`).
- **Binding apenas em localhost** - Gateway faz bind em `127.0.0.1` por padrão, não em `0.0.0.0`.

## Recursos

### Provedores de LLM

**Provedores nativos:**

- **Anthropic Claude** - streaming (SSE), uso de ferramentas
- **OpenAI** - GPT-4o, Azure, qualquer endpoint compatível com OpenAI via `base_url`
- **Ollama** - modelos locais com streaming

**Provedores compatíveis com OpenAI:**

- **Sansa** - LLM regional via [sansaml.com](https://sansaml.com)
- **DeepSeek** - DeepSeek Chat
- **Mistral** - Mistral Large
- **Gemini** - Google Gemini via API compatível com OpenAI
- **Falcon** - TII Falcon 180B (AI71)
- **Jais** - Core42 Jais 70B
- **Qwen** - Alibaba Qwen Plus
- **Yi** - 01.AI Yi Large
- **Cohere** - Command R Plus
- **MiniMax** - MiniMax Text 01
- **Moonshot** - Kimi K2

### Canais
- **Telegram** - respostas streaming, MarkdownV2, comandos do bot, indicadores de digitação, lista de permissões de usuários com códigos de pareamento
- **Discord** - comandos slash, tratamento de mensagens orientado a eventos, gerenciamento de sessões
- **Slack** - Socket Mode, respostas streaming, lista de permissões/pareamento
- **WhatsApp** - webhooks da Meta Cloud API, lista de permissões/pareamento
- **iMessage** - nativo macOS via polling de chat.db, grupos de chat, envio via AppleScript ([guia de configuração](docs/imessage-setup.md))

### MCP (Protocolo de Contexto de Modelo)
- Conecte qualquer servidor compatível com MCP (sistema de arquivos, GitHub, bancos de dados, busca na web)
- Ferramentas aparecem como ferramentas nativas do agente com nomes namespaced (`server.tool`)
- Configure em `config.yml` ou `~/.garraia/mcp.json` (compatível com Claude Desktop)
- CLI: `garraia mcp list`, `garraia mcp inspect <name>`

### Runtime do Agente
- Loop de execução de ferramentas - bash, file_read, file_write, web_fetch, web_search, schedule_heartbeat (até 10 iterações)
- Memória de conversa com suporte a SQLite com busca vetorial (sqlite-vec + embeddings Cohere)
- Gerenciamento de janela de contexto - aparamento automático de histórico
- Tarefas agendadas - agendamento cron, intervalo e único

### Skills
- Defina skills de agente como arquivos Markdown (SKILL.md) com frontmatter YAML
- Auto-descoberta de `~/.garraia/skills/` - injetado no prompt do sistema
- CLI: `garraia skill list`, `garraia skill install <url>`, `garraia skill remove <name>`

### Infraestrutura
- **Recarregamento de config a quente** - edite `config.yml`, as alterações são aplicadas sem reiniciar
- **Daemonização** - `garraia start --daemon` com gerenciamento de PID
- **Auto-atualização** - `garraia update` baixa a versão mais recente com verificação SHA-256, `garraia rollback` para reverter
- **Reinicialização** - `garraia restart` para graciosamente parar e iniciar o daemon
- **Troca de provedor em runtime** - adicione ou troque provedores de LLM via interface webchat ou API REST sem reiniciar
- **Ferramenta de migração** - `garraia migrate openclaw` importa skills, canais e credenciais
- **Configuração interativa** - `garraia init` wizard para configuração de provedor e chave de API

## Migrando do OpenClaw?

Um comando importa suas skills, configurações de canais e credenciais (criptografadas no cofre):

```bash
garraia migrate openclaw
```

Use `--dry-run` para visualizar as alterações antes de confirmar. Use `--source /caminho/para/openclaw` para especificar um diretório de configuração personalizado do OpenClaw.

## Configuração

O GarraIA procura configuração em `~/.garraia/config.yml`:

```yaml
gateway:
  host: "127.0.0.1"
  port: 3888

llm:
  claude:
    provider: anthropic
    model: claude-sonnet-4-5-20250929
    # api_key resolvido de: vault > config > variável de ambiente ANTHROPIC_API_KEY

  ollama-local:
    provider: ollama
    model: llama3.1
    base_url: "http://localhost:11434"

channels:
  telegram:
    type: telegram
    enabled: true
    bot_token: "seu-bot-token"  # ou variável de ambiente TELEGRAM_BOT_TOKEN

agent:
  system_prompt: "Você é um assistente útil."
  max_tokens: 4096
  max_context_tokens: 100000

memory:
  enabled: true

# Servidores MCP para ferramentas externas
mcp:
  filesystem:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
```

Consulte a [referência completa de configuração](docs/) para todas as opções, incluindo Discord, Slack, WhatsApp, iMessage, embeddings e configuração de servidor MCP.

## Arquitetura

```
crates/
  garraia-cli/        # CLI, assistente de init, gerenciamento de daemon
  garraia-gateway/    # Gateway WebSocket, API HTTP, sessões
  garraia-config/     # Carregamento YAML/TOML, hot-reload, config MCP
  garraia-channels/   # Discord, Telegram, Slack, WhatsApp, iMessage
  garraia-agents/     # Provedores de LLM, ferramentas, cliente MCP, runtime do agente
  garraia-db/         # Memória SQLite, busca vetorial (sqlite-vec)
  garraia-plugins/    # Sandbox de plugins WASM (wasmtime)
  garraia-media/     # Processamento de mídia (esquelético)
  garraia-security/  # Cofre de credenciais, listas de permissões, pareamento, validação
  garraia-skills/    # Parser de SKILL.md, scanner, instalador
  garraia-common/    # Tipos compartilhados, erros, utilitários
```

| Componente | Status |
|-----------|--------|
| Gateway (WebSocket, HTTP, sessões) | Funcionando |
| Telegram (streaming, comandos, pareamento) | Funcionando |
| Discord (comandos slash, sessões) | Funcionando |
| Slack (Socket Mode, streaming) | Funcionando |
| WhatsApp (webhooks) | Funcionando |
| iMessage (macOS, grupos) | Funcionando |
| Provedores de LLM (14: Anthropic, OpenAI, Ollama + 11 compatíveis com OpenAI) | Funcionando |
| Ferramentas do agente (bash, file_read, file_write, web_fetch, web_search, schedule_heartbeat) | Funcionando |
| Cliente MCP (stdio, bridge de ferramentas) | Funcionando |
| Skills (SKILL.md, auto-descoberta) | Funcionando |
| Configuração (YAML/TOML, hot-reload) | Funcionando |
| Memória (SQLite, busca vetorial) | Funcionando |
| Segurança (cofre, lista de permissões, pareamento) | Funcionando |
| Agendamento (cron, intervalo, único) | Funcionando |
| CLI (init, start/stop/restart, update, migrate, mcp, skills) | Funcionando |
| Sistema de plugins (Sandbox WASM) | Esquelético |
| Processamento de mídia | Esquelético |

## Contribuindo

O GarraIA é código aberto sob licença MIT. Junte-se ao [Discord](https://discord.gg/aEXGq5cS) para conversar com contribuidores, fazer perguntas ou compartilhar o que você está construindo. Consulte [CONTRIBUTING.md](CONTRIBUTING.md) para instruções de configuração, diretrizes de código e visão geral dos crates.

### Prioridades atuais

| Prioridade | Issue | Descrição |
|----------|-------|-------------|
| **P0** | [#103](https://github.com/michelbr84/GarraRUST/issues/103) | README e posicionamento |
| **P0** | [#104](https://github.com/michelbr84/GarraRUST/issues/104) | Website: garraia.org |
| **P0** | [#105](https://github.com/michelbr84/GarraRUST/issues/105) | Comunidade Discord |
| **P1** | [#106](https://github.com/michelbr84/GarraRUST/issues/106) | Skills iniciais incluídas |
| **P1** | [#107](https://github.com/michelbr84/GarraRUST/issues/107) | Reforço de agendamento |
| **P1** | [#108](https://github.com/michelbr84/GarraRUST/issues/108) | Roteamento multi-agente |
| **P1** | [#109](https://github.com/michelbr84/GarraRUST/issues/109) | Script de instalação |
| **P1** | [#110](https://github.com/michelbr84/GarraRUST/issues/110) | Releases Linux aarch64 + Windows |
| **P1** | [#80](https://github.com/michelbr84/GarraRUST/issues/80) | MCP: transporte HTTP, recursos, prompts |

Navegue por todas as [issues abertas](https://github.com/michelbr84/GarraRUST/issues) ou filtre por [`good-first-issue`](https://github.com/michelbr84/GarraRUST/issues?q=label%3Agood-first-issue+is%3Aopen) para encontrar um lugar para começar.

## Licença

MIT
