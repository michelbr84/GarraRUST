<!-- markdownlint-disable MD033 MD041 MD060 -->

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
</p>

<p align="center">
  <img src="https://img.shields.io/badge/rust-1.92%2B-orange?logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/license-MIT-blue" alt="License">
  <img src="https://img.shields.io/badge/crates-16-green" alt="Crates">
  <img src="https://img.shields.io/badge/channels-11-purple" alt="Channels">
  <img src="https://img.shields.io/badge/LLM%20providers-15-red" alt="Providers">
</p>

<p align="center">
  <a href="#início-rápido">Início Rápido</a> &middot;
  <a href="#por-que-garraia">Por que GarraIA?</a> &middot;
  <a href="#recursos">Recursos</a> &middot;
  <a href="#memória-e-auto-aprendizado">Memória</a> &middot;
  <a href="#segurança">Segurança</a> &middot;
  <a href="#arquitetura">Arquitetura</a> &middot;
  <a href="#migrando-do-openclaw">Migrar do OpenClaw</a> &middot;
  <a href="#contribuindo">Contribuindo</a>
</p>

---

**O assistente de IA brasileiro que funciona 100% no seu computador.** Um único binário de 16 MB que executa seus agentes de IA no Telegram, Discord, Slack, WhatsApp e iMessage — com armazenamento de credenciais criptografadas, recarregamento de configuração a quente, sistema completo de memória e apenas 13 MB de RAM em modo de espera. Desenvolvido em Rust para a segurança e confiabilidade que os agentes de IA exigem.

**100% Local** — Todos os seus dados, conversas e configurações ficam exclusivamente no seu computador. Nenhum dado é enviado para servidores externos.

<!-- TODO: Adicionar GIF de demonstração do terminal VHS aqui (#103) -->

## 🗺️ Roadmap AAA

O desenvolvimento do GarraRUST segue um plano ambicioso de evolução para o tier AAA em 7 fases, consolidado no [ROADMAP.md](ROADMAP.md). Inclui Superpowers, TurboQuant+ (KV cache), RAG local (lancedb), MCP + plugins WASM, zero-latency streaming (OpenTelemetry), e a nova direção **Group Workspace** — espaço compartilhado família/equipe multi-tenant com arquivos, chats, memória IA e módulo tipo-Notion (tasks + docs + databases), desenhado em [`deep-research-report.md`](deep-research-report.md). Execução semana a semana acompanhada nos [projects Linear do time GarraIA-RUST](https://linear.app/chatgpt25/team/GAR/projects).

## Início Rápido

```bash
# Requer Rust 1.92+ (alinhado com MSRV declarado em Cargo.toml — GAR-441)
cargo build --release -p garraia

# Configuração interativa - escolha seu provedor de LLM, armazene chaves de API em cofre criptografado
./target/release/garraia init

# Iniciar
./target/release/garraia start

# Opcional: incluir suporte a plugins WASM
cargo build --release -p garraia --features plugins
```

<details>
<summary>Compilar o app desktop (Tauri)</summary>

O app desktop requer que o binário CLI já esteja compilado como sidecar:

```bash
# 1. Compilar o CLI primeiro
cargo build --release -p garraia

# 2. Copiar para o diretório de sidecar esperado pelo Tauri
cp target/release/garraia crates/garraia-desktop/src-tauri/binaries/garraia-$(rustc -vV | grep host | cut -d' ' -f2)

# 3. Compilar o desktop
cargo build --release -p garraia-desktop
```

</details>

<details>
<summary>Instalar via script (Linux, macOS) — requer binários publicados no release</summary>

```bash
curl -fsSL https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh | sh
garraia init
garraia start
```

> **Nota:** o script de instalação requer que binários CLI pré-compilados estejam publicados nas [Versões do GitHub](https://github.com/michelbr84/GarraRUST/releases). Enquanto isso, compile a partir do código-fonte conforme acima.

</details>

Instaladores para desktop (Windows `.msi`) e mobile (Android `.apk`) estão disponíveis nas [Versões do GitHub](https://github.com/michelbr84/GarraRUST/releases).

## Por que GarraIA?

### vs OpenClaw, ZeroClaw e outros frameworks de agentes de IA

| | | **GarraIA** | **OpenClaw** (Node.js) | **ZeroClaw** (Rust) |
|---|---|---|---|---|
| | **Tamanho do binário** | 16 MB | ~1.2 GB (com node_modules) | ~25 MB |
| | **Memória em idle** | 13 MB | ~388 MB | ~20 MB |
| | **Início a frio** | 3 ms | 13.9 s | ~50 ms |
| | **Armazenamento de credenciais** | Cofre criptografado AES-256-GCM | Arquivo de configuração em texto puro | Arquivo de configuração em texto puro |
| | **Autenticação padrão** | Habilitada (pareamento WebSocket) | Desabilitada por padrão | Desabilitada por padrão |
| | **Agendamento** | Cron, intervalo, único | Sim | Não |
| | **Roteamento multi-agente** | Sim (Priority Router) | Sim (agentId) | Não |
| | **Orquestração de sessões** | Sim (Session Continuity) | Sim | Não |
| | **Suporte MCP** | Stdio, HTTP, SSE, StreamableHttp | Stdio + HTTP | Stdio |
| | **Canais** | 11 | 6+ | 4 |
| | **Provedores de LLM** | 100+ | 10+ | 22+ |
| | **Binários pré-compilados** | Sim | N/A (Node.js) | Compilar a partir do código-fonte |
| | **Recarregamento de config a quente** | Sim | Não | Não |
| | **Sistema de plugins WASM** | Opcional (sandbox) | Não | Não |
| | **Auto-atualização** | Sim (`garraia update`) | npm | Compilar a partir do código-fonte |
| | **Arquitetura 100% local** | ✅ Sim | Não | Não |
| | **Sistema de memória completo** | ✅ Sim (facts, sessions, vetorial) | Não | Não |
| | **Auto-learning (extrator LLM)** | ✅ Sim | Não | Não |

*Benchmarks medidos em um droplet DigitalOcean com 1 vCPU, 1 GB RAM. [Reproduza você mesmo](bench/).*

## Recursos

### Provedores de LLM

**Provedores nativos:**

- **Anthropic Claude** - streaming (SSE), uso de ferramentas
- **OpenAI** - GPT-4o, Azure, qualquer endpoint compatível com OpenAI via `base_url`
- **Ollama** - modelos locais com streaming, embeddings locais

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
- **OpenRouter** - Acesso a +100 LLMs (Anthropic, OpenAI, Meta, etc.) via [openrouter.ai](https://openrouter.ai)

### Canais

- **Telegram** - respostas streaming, MarkdownV2, comandos do bot, indicadores de digitação, lista de permissões de usuários com códigos de pareamento
- **Discord** - comandos slash, tratamento de mensagens orientado a eventos, gerenciamento de sessões
- **Slack** - Socket Mode, respostas streaming, lista de permissões/pareamento
- **WhatsApp** - webhooks da Meta Cloud API, lista de permissões/pareamento
- **iMessage** - nativo macOS via polling de chat.db, grupos de chat, envio via AppleScript ([guia de configuração](docs/imessage-setup.md))
- **Google Chat** - integração via API do Google Workspace
- **Microsoft Teams** - bot via Bot Framework / Graph API
- **Matrix** - protocolo federado, suporte a rooms e E2EE
- **LINE** - Messaging API com webhooks
- **IRC** - cliente IRC com suporte a múltiplos canais e redes
- **Signal** - mensagens seguras via signal-cli
- **VS Code** - via API OpenAI-compatible, integrado ao mesmo histórico de conversas

### Comandos e Aliases (Slash Commands)

O GarraIA possui um sistema unificado de comandos interativos disponíveis no chat (integrado nativamente ao menu do Telegram):

- `/help` - Exibe os comandos disponíveis dinamicamente
- `/clear` - Limpa o histórico da conversa atual
- `/model [nome]` - Visualiza ou altera o modelo LLM em uso
- `/pair` - Gera um código de convite para pareamento
- `/users` - Lista os usuários permitidos no sistema
- `/voz` (ou `/voice`) - Alterna o envio de respostas em áudio na sessão
- `/health` - Exibe o status de saúde dos serviços (LLMs, TTS, BD, MCP)
- `/providers` - Lista os provedores LLM configurados
- `/stats` - Exibe métricas de uso e uptime do servidor
- `/config` - Gerencia definições em runtime (apenas administradores)
- `/mcp` - Gerencia servidores e recursos MCP acoplados

Além dos comandos embutidos, qualquer servidor MCP que exponha **prompts** via `prompts/list` aparece automaticamente como slash command. Por exemplo, um servidor de automação com prompt `n8n-deploy` fica disponível como `/n8n-deploy [args]`. O endpoint `GET /api/slash-commands` retorna a lista completa (built-ins + MCP dinâmicos).

### Voice Mode (STT/TTS) com Múltiplos Providers

- **STT Providers** - Whisper local (whisper.cpp) e OpenAI Whisper API com dual-endpoint
- **TTS Providers** - Chatterbox (GPU, multilíngue), Hibiki, ElevenLabs, Kokoro, OpenAI TTS API
- **Síntese multilíngue** - pt, en, es, fr, de, it, hi via GPU local
- **Endpoint REST** - `POST /api/tts` para síntese sob demanda
- **Ativação** - `garraia start --with-voice` habilita o modo de voz
- **Health check automático** - verificação HTTP do Chatterbox no boot
- **Integração Telegram** - resposta por áudio automática no pipeline voice
- **Conversão de formato** - via ffmpeg, streaming de áudio em tempo real

### VS Code Integration (API OpenAI-Compatible)

O GarraIA agora oferece uma **API OpenAI-compatible** que permite integração com o VS Code e outras ferramentas que suportam endpoints estilo OpenAI.

#### Endpoints Disponíveis

| Endpoint | Método | Descrição |
|----------|--------|----------|
| `/v1/chat/completions` | POST | Enviar mensagens e receber respostas do agente |
| `/v1/models` | GET | Listar modelos disponíveis |

#### Cabeçalhos Personalizados

| Cabeçalho | Descrição |
|-----------|-----------|
| `X-Session-Id` | ID de sessão para continuidade de conversa |
| `Authorization` | Chave de API (Bearer token) |
| `X-Source` | Fonte da requisição (ex: "vscode", "telegram") |

#### Exemplo de Uso

```bash
# Listar modelos disponíveis
curl -X GET http://127.0.0.1:3888/v1/models \
  -H "Authorization: Bearer sua-api-key"

# Enviar mensagem (sem sessão - cria nova)
curl -X POST http://127.0.0.1:3888/v1/chat/completions \
  -H "Authorization: Bearer sua-api-key" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o",
    "messages": [
      {"role": "user", "content": "Olá, como você está?"}
    ]
  }'

# Enviar mensagem (com sessão existente)
curl -X POST http://127.0.0.1:3888/v1/chat/completions \
  -H "Authorization: Bearer sua-api-key" \
  -H "X-Session-Id: sessao-123-abc" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o",
    "messages": [
      {"role": "user", "content": "Continue a conversa anterior"}
    ]
  }'
```

#### Configuração no VS Code

Use extensões como **Continue** ou **Watt** que suportam endpoints OpenAI customizados:

```json
// settings.json do VS Code (exemplo para Continue)
{
  "continue.serverEndpoint": "http://127.0.0.1:3888/v1",
  "continue.apiKey": "sua-api-key",
  "continue.selectedModel": "gpt-4o"
}
```

Consulte o [guia completo de configuração para VS Code](docs/vscode/setup.md) para instruções passo a passo com a extensão Continue.

#### Continuidade de Conversa

O GarraIA mantém **histórico unificado** entre todos os canais:

- **Mesma sessão** = mesmo histórico, mesma memória
- Telegram ↔ VS Code ↔ Web Chat compartilham o contexto
- Sessões são persistidas em SQLite automaticamente

#### Session ID Strategy

| Método | Descrição |
|--------|-----------|
| `X-Session-Id` header | Recomendado: passe o ID de sessão explicitamente |
| Gerar novo | Se nenhum ID for fornecido, uma nova sessão é criada |
| Recuperação | Use `/v1/models` para verificar a conexão, depois inicie com `X-Session-Id` vazio para nova sessão |

#### Segurança Api

- Requer autenticação via `Authorization: Bearer <api_key>`
- O endpoint é binding em `127.0.0.1` por padrão (local only)
- Para produção, configure TLS/reverse proxy
- Use o sistema de whitelist do GarraIA para controlar acesso

### MCP (Protocolo de Contexto de Modelo)

- Conecte qualquer servidor compatível com MCP (sistema de arquivos, GitHub, bancos de dados, busca na web)
- Ferramentas aparecem como ferramentas nativas do agente com nomes namespaced (`server.tool`)
- Configure em `config.yml` ou `~/.garraia/mcp.json` (compatível com Claude Desktop)
- CLI: `garraia mcp list`, `garraia mcp inspect <name>`

### Modos de Execução (Agent Modes)

O GarraIA possui um sistema avançado de **Modos de Execução** que permite selecionar diferentes estratégias de comportamento do agente:

| Modo | Descrição | Políticas de Ferramentas |
|------|-----------|--------------------------|
| **Auto** | Roteamento inteligente automático baseado no conteúdo da mensagem | Herda do modo resolvido |
| **Ask** | Modo de pergunta/resposta, foco em explicações | Leitura apenas |
| **Search** | Busca e inspeção de código sem modificar arquivos | `repo_search`, `list_dir`, `file_read` |
| **Architect** | Design e planejamento de arquitetura | Ferramentas de leitura |
| **Code** | Implementação e refatoração de código | `file_read`, `file_write`, `bash` |
| **Debug** | Análise de erros e troubleshooting | `repo_search`, `file_read`, `bash` (read-only) |
| **Orchestrator** | Execução multi-etapas com validação | Todas com guardrails |
| **Review** | Revisão de código e análise de diffs | `git_diff`, `file_read` |
| **Edit** | Edição direcionada de arquivos | `file_read`, `file_write` |
| **Custom** | Modos criados pelo usuário | Herda do base_mode com overrides |

#### Precedência de Modo

O modo é resolvido nesta ordem:

1. **Header** `X-Agent-Mode` (maior prioridade)
2. **Comando** `/mode <nome>` no chat
3. **Preferência por canal** (Telegram = `ask`, Web/API = `auto`)
4. **Preferência por usuário**
5. **Default** do sistema

#### Comandos de Modo

- `/mode` - Mostra o modo atual
- `/mode <nome>` - Altera o modo (ex: `/mode code`)
- `/modes` - Lista todos os modos disponíveis

#### Modos Customizados

Crie seus próprios modos baseados em um modo existente:

```bash
# Via API
curl -X POST http://127.0.0.1:3888/api/modes/custom \
  -H "Authorization: Bearer sua-api-key" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "Rust Strict",
    "description": "Modo rigoroso para Rust",
    "base_mode": "code",
    "prompt_override": "Você é um especialista em Rust...",
    "tool_policy_overrides": {
      "allow": ["file_read", "file_write", "bash"],
      "deny": ["web_fetch"]
    },
    "defaults": {
      "temperature": 0.3,
      "max_tokens": 8192
    }
  }'
```

Ou use a UI WebChat para criar/editar modos via interface visual.

#### Ferramentas do Modo Orchestrator

O modo Orchestrator executa tarefas multi-etapas com:

- **Planejamento** - Gera lista de steps automaticamente
- **Validação** - Verifica resultado de cada step
- **Retry** - Tenta novamente em caso de falha (máx 2x)
- **Segurança** - Checklist de comandos bash perigosos bloqueados
- **Limites** - max_loops: 10, timeout: 30s por step

#### Auto Mode Router

O modo `auto` usa heurísticas determinísticas para selecionar o modo correto:

- Contém caminho de arquivo (`C:\`, `G:\`, `/home/`) → `search` ou `debug`
- "refatorar", "implementar", "criar arquivo" → `code`
- "explique", "o que é", "conceito" → `ask`
- "erro", "stacktrace", "panic", "log" → `debug`
- "roadmap", "design", "arquitetura" → `architect`
- "faça review", "analise diff" → `review`

#### Integração com Continue/VS Code

Configure o Continue para usar o GarraIA com o modo desejado:

```json
// settings.json do VS Code
{
  "continue.serverEndpoint": "http://127.0.0.1:3888/v1",
  "continue.apiKey": "sua-api-key",
  "continue.selectedModel": "gpt-4o"
}
```

Para usar modo específico, adicione o header `X-Agent-Mode` na requisição ou use o comando `/mode` no chat.

##### Headers Suportados

| Header | Descrição |
|--------|-----------|
| `X-Agent-Mode` | Override de modo (auto, code, debug, ask, etc.) |
| `X-Request-Id` | ID de request para tracing |
| `X-Session-Id` | ID de sessão para continuidade |
| `X-User-Id` | ID do usuário |

##### Modo Prefix (Fallback)

Se o header não for suportado, use prefix no início da mensagem:

- `mode: debug` → muda para modo debug
- `/mode ask` → muda para modo ask

Consulte a [documentação completa de integração com Continue](docs/continue-modes.md).

#### API de Modos

| Endpoint | Método | Descrição |
|----------|--------|----------|
| `/api/modes` | GET | Lista todos os modos disponíveis |
| `/api/mode/select` | POST | Seleciona modo para sessão |
| `/api/mode/current` | GET | Retorna modo atual da sessão |
| `/api/modes/custom` | GET/POST | Lista/cria modos customizados |
| `/api/modes/custom/:id` | PATCH/DELETE | Edita/remove modo custom |

### Runtime do Agente

- Loop de execução de ferramentas - bash, file_read, file_write, web_fetch, web_search, repo_search, list_dir, git_diff, schedule_heartbeat (até 10 iterações)
- Memória de conversa com suporte a SQLite com busca vetorial (sqlite-vec + embeddings Cohere)
- **Janela de contexto deslizante** - `max_history_messages` limita quantos turnos são enviados ao LLM sem afetar o armazenamento; `trim_messages_to_budget` apara pelo orçamento de tokens
- **Sumarização automática de contexto** - quando o número de turnos desde o último resumo atinge `summarize_threshold`, um job background chama um modelo barato para gerar um resumo. O resumo é injetado como mensagem System no início do histórico hidratado — o LLM sempre tem contexto de sessões longas sem estourar a janela
- Tarefas agendadas - agendamento cron, intervalo e único

### Skills

- Defina skills de agente como arquivos Markdown (SKILL.md) com frontmatter YAML
- Auto-descoberta de `~/.garraia/skills/` - injetado no prompt do sistema
- CLI: `garraia skill list`, `garraia skill install <url>`, `garraia skill remove <name>`

### MCP Tool Integration com Marketplace

- Conecte qualquer servidor compatível com MCP (filesystem, GitHub, bancos de dados, busca na web)
- **Marketplace de ferramentas** - descubra e instale servidores MCP via `garraia mcp install`
- Ferramentas aparecem como ferramentas nativas com nomes namespaced (`server.tool`)
- Prompts MCP viram slash commands automaticamente
- Admin API para adicionar/remover servidores sem reiniciar

### Sistema de Plugins WASM

- Sandbox WebAssembly via wasmtime com acesso controlado ao host
- Compile com `--features plugins` para habilitar
- Isolamento de memória e CPU por plugin
- API host para acesso a ferramentas e estado do agente

### Skills Editor com CRUD

- Defina skills de agente como arquivos Markdown (SKILL.md) com frontmatter YAML
- Auto-descoberta de `~/.garraia/skills/`
- **Editor visual** na WebChat UI para criar/editar skills
- CLI: `garraia skill list`, `garraia skill install <url>`, `garraia skill remove <name>`
- CRUD completo via API REST (`GET/POST/PATCH/DELETE /api/skills`)

### Autenticacao OAuth2/OIDC + TOTP 2FA

- **OAuth2/OIDC** - suporte a provedores externos de identidade
- **TOTP 2FA** - autenticacao de dois fatores via aplicativo (Google Authenticator, Authy)
- **JWT** - tokens de sessao com 30 dias de validade, refresh automatico
- **PBKDF2-HMAC-SHA256** - 600k iteracoes para hash de senhas
- **Pareamento por codigo** - whitelist de usuarios por canal

### EU AI Act Compliance

- **Headers X-AI-Model** - todas as respostas incluem o modelo usado (`X-AI-Model`, `X-AI-Provider`)
- **Transparencia** - identificacao clara de conteudo gerado por IA
- **Logging auditavel** - registros estruturados de todas as interacoes com LLMs

### TLS/HTTPS Nativo

- **Suporte TLS nativo** - configure certificados SSL diretamente no GarraIA
- **Let's Encrypt** - renovacao automatica de certificados
- **Binding seguro** - `127.0.0.1` por padrao, `0.0.0.0` com TLS para producao

### Health Checks Centralizados

- **Boot** - tabela visual no terminal com ✅/❌ e latência por provider
- **Endpoint** - `GET /api/health` retorna JSON com status de todos os providers
- **Background** - verificação periódica (60s) com detecção de mudança de status
- **Providers** - Ollama, OpenRouter, OpenAI, Anthropic, Chatterbox TTS
- **Cache** - resultados cacheados para respostas instantâneas no endpoint

### Infraestrutura

- **Recarregamento de config a quente** - edite `config.yml`, as alterações são aplicadas sem reiniciar
- **Daemonização** - `garraia start --daemon` com gerenciamento de PID
- **Auto-atualização** - `garraia update` baixa a versão mais recente com verificação SHA-256, `garraia rollback` para reverter
- **Reinicialização** - `garraia restart` para graciosamente parar e iniciar o daemon
- **Troca de provedor em runtime** - adicione ou troque provedores de LLM via interface webchat ou API REST sem reiniciar
- **Fallback automático de providers** - em caso de erro 429/5xx, tenta automaticamente o próximo provider configurado em `fallback_providers` com backoff exponencial e circuit breaker
- **Timeouts configuráveis** - timeouts por tipo (LLM: 30s, TTS: 120s, MCP: 60s, Health: 5s) via `config.yml`
- **Rate limiting por IP** - proteção automática configurável (`per_second`, `burst_size`) via `config.yml`
- **Logs estruturados** - campos rastreáveis (`request_id`, `session_id`, `source`, `model`, `latency_ms`); JSON format via `GARRAIA_LOG_FORMAT=json`
- **Ferramenta de migração** - `garraia migrate openclaw` importa skills, canais e credenciais
- **Configuração interativa** - `garraia init` wizard para configuração de provedor e chave de API

## Memória e Auto-Aprendizado

O GarraIA possui um sistema completo de memória que permite ao agente aprender e lembrar informações entre conversas.

### Sistema de Memória Completo

```text
~/.garraia/
├── memoria/
│   ├── fatos.json          # Facts extraídos pelo LLM
│   └── embeddings/         # Embeddings vetoriais locais
├── data/
│   ├── memory.db           # Memória SQLite com vetores
│   └── sessions.db         # Sessões de conversa
└── credentials/
    └── vault.json          # Credenciais criptografadas
```

### Componentes da Memória

| Componente | Descrição |
|------------|-----------|
| **facts.json** | Fatos importantes extraídos automaticamente das conversas pelo extrator LLM |
| **memory.db** | Banco SQLite com histórico de conversas e busca vetorial (sqlite-vec) |
| **sessions.db** | Gerenciamento de sessões de conversa persistentes |
| **embeddings/** | Vetores de embedding armazenados localmente para busca semântica |

### Auto-Learning com Extrator LLM

O GarraIA aprende automaticamente das conversas usando um extrator LLM dedicado:

- **Extração automática** - Após cada conversa, o extrator analisa as mensagens e identifica fatos importantes
- **Fatos estruturados** - Informações são salvas em `fatos.json` com contexto e data
- **Busca semântica** - Use embeddings locais (Ollama) para buscar fatos relevantes
- **Integração com o prompt** - Facts são automaticamente incluídos no contexto do agente

```yaml
memory:
  enabled: true
  auto_extract: true        # Extrai fatos automaticamente
  extraction_interval: 5    # Intervalo em minutos
  max_facts: 100           # Máximo de fatos armazenados
  
embeddings:
  provider: ollama          # ou "openai", "cohere"
  model: nomic-embed-text  # Modelo de embedding local
  base_url: "http://localhost:11434"
```

### Embeddings Locais com Ollama

Execute embeddings 100% no seu computador usando Ollama:

- **Modelos suportados**: nomic-embed-text, mxbai-embed-large, all-minilm, etc.
- **Busca semântica** - Encontre informações relevantes por significado, não apenas palavras
- **Privacidade total** - Nenhum dado sai do seu computador
- **Performance** - Rápido e eficiente com modelos locais

```yaml
embeddings:
  provider: ollama
  model: nomic-embed-text
  base_url: "http://localhost:11434"
  dimension: 768
```

### API de Memória

| Comando | Descrição |
|---------|-----------|
| `garraia memory list` | Listar todos os fatos |
| `garraia memory search <query>` | Buscar fatos por相似idade |
| `garraia memory add <fato>` | Adicionar um fato manualmente |
| `garraia memory clear` | Limpar todos os fatos |
| `garraia memory export` | Exportar fatos para JSON |

## Segurança

O GarraIA foi desenvolvido para os requisitos de segurança de agentes de IA que ficam sempre ativos, acessam dados privados e se comunicam externamente.

- **Cofre de credenciais criptografadas** - Chaves de API e tokens armazenados com criptografia AES-256-GCM em `~/.garraia/credentials/vault.json`. Nunca em texto puro no disco.
- **Tokens MCP protegidos por vault** - Variáveis de ambiente sensíveis dos servidores MCP (`API_KEY`, `TOKEN`, `SECRET`, etc.) são automaticamente movidas para o vault no primeiro `save`. O `mcp.json` armazena apenas referências `vault:mcp.<server>.<key>`. Sem `GARRAIA_VAULT_PASSPHRASE`, salva em plaintext com aviso — nunca quebra o boot.
- **Tokens de sessão criptograficamente seguros** - Cada sessão WebSocket recebe um token de 256 bits (URL-safe base64). Suportados via cookie `garraia_session` (HttpOnly, SameSite=Strict), header `Authorization: Bearer` ou `X-Session-Key`. TTL e idle-timeout configuráveis. Rotação automática no resume.
- **Autenticação por padrão** - Gateway WebSocket requer códigos de pareamento. Sem acesso não autenticado fora da caixa.
- **Listas de permissões por usuário** - Listas de permissões por canal controlam quem pode interagir com o agente. Mensagens não autorizadas são descartadas silenciosamente.
- **Detecção de injeção de prompt** - Validação e saneamento de entrada antes do conteúdo chegar ao LLM.
- **Confirmação de comandos arriscados** - `tool_confirmation_enabled: true` pausa o agente antes de executar comandos bash destrutivos (`rm -r`, `git reset --hard`, `drop database`, etc.) e aguarda aprovação do usuário ("sim"/"yes"). Default: `false` (opt-in).
- **Sandboxing de processos MCP** - Limites de memória virtual por processo (Unix, via `setrlimit`), timeout de inicialização configurável e restart automático com backoff exponencial (base × 2ⁿ, cap 300s). Após `max_restarts` tentativas, o servidor fica offline até restart manual via API admin.
- **Sandbox WASM** - Plugin opcional em sandbox via runtime WebAssembly com acesso controlado ao host (compile com `--features plugins`).
- **Binding apenas em localhost** - Gateway faz bind em `127.0.0.1` por padrão, não em `0.0.0.0`.

### Arquitetura Local e Sob Controle do Usuário

O GarraIA foi projetado para funcionar 100% no seu computador:

- **Sem dependência de nuvem** - Execute tudo localmente
- **Seus dados são seus** - Conversas, facts e configurações ficam no seu PC
- **Sem telemetria** - Nenhum dado é enviado para servidores externos
- **Controle total** - Você decide onde e como executar
- **Offline capable** - Funciona com modelos locais Ollama sem internet

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
  # GAR-202: tokens de sessão — TTL, idle timeout e exigência de autenticação
  session_ttl_secs: 86400       # validade do token (1 dia). Padrão: 86400
  session_idle_secs: 3600       # timeout por inatividade (1h). Padrão: 3600
  session_tokens_required: false # exige token nas rotas /api/* . Padrão: false

llm:
  claude:
    provider: anthropic
    model: claude-sonnet-4-5-20250929
    # api_key resolvido de: vault > config > variável de ambiente ANTHROPIC_API_KEY

  openai:
    provider: openai
    model: gpt-4o
    # api_key resolvido de: vault > config > variável de ambiente OPENAI_API_KEY

  # OpenRouter - acesso a +100 modelos diferentes
  openrouter:
    provider: openrouter
    model: openai/gpt-4o  # modelos: openai/gpt-4o, anthropic/claude-3.5-sonnet, meta-llama/llama-3.1-70b-instruct, etc.
    # api_key resolvido de: vault > config > variável de ambiente OPENROUTER_API_KEY
    # O GarraIA envia automaticamente os headers HTTP-Referer e X-Title para o OpenRouter
    # Isso faz o app aparecer como "GarraIA" no dashboard do OpenRouter (não "Unknown")

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
  max_tool_calls: 50        # limite de tool calls por tarefa (padrão: 50)
  # GAR-210: fallback automático quando o provider primário retorna 429/5xx
  fallback_providers:
    - openrouter
    - ollama-local
  # GAR-187: confirmação humana antes de comandos bash destrutivos (opt-in)
  tool_confirmation_enabled: false
  # GAR-208: janela deslizante de contexto — só os últimos N turnos vão ao LLM
  max_history_messages: 20
  # GAR-208: sumarização automática — gera resumo a cada N novos turnos desde o último
  summarize_threshold: 40
  summarizer_model: "openrouter/mistral-7b-instruct"  # modelo barato para sumarização

memory:
  enabled: true
  auto_extract: true
  extraction_interval: 5

embeddings:
  provider: ollama
  model: nomic-embed-text
  base_url: "http://localhost:11434"

# Servidores MCP para ferramentas externas
mcp:
  filesystem:
    command: npx
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    # GAR-293: limites de recursos e política de restart
    memory_limit_mb: 512      # máximo de memória virtual (Unix). Padrão: sem limite
    max_restarts: 5           # tentativas de restart automático após crash. Padrão: 5
    restart_delay_secs: 5     # delay base do backoff exponencial (máx 300s). Padrão: 5

# Voice mode (TTS)
voice:
  enabled: true
  tts_endpoint: "http://127.0.0.1:7860"
  language: "pt"

# GAR-261: glob e ignore para ferramentas de busca de arquivos
fs:
  glob:
    mode: picomatch   # picomatch (padrão) | bash
    dot: false        # se true, * e ? casam dotfiles (.hidden)
  ignore:
    use_gitignore: true  # respeita .gitignore durante varredura

# Timeouts configuráveis por tipo (valores em segundos)
timeouts:
  llm:
    default_secs: 120   # modelos grandes podem demorar; 30s era curto demais
  tts:
    default_secs: 120
  mcp:
    default_secs: 60
  health:
    default_secs: 5
```

Consulte a [referência completa de configuração](docs/) para todas as opções, incluindo Discord, Slack, WhatsApp, iMessage, voice mode, embeddings e configuração de servidor MCP.

### .garraignore

Crie um `.garraignore` na raiz do projeto para controlar quais arquivos o agente ignora durante buscas (`file_read`, `repo_search`, `list_dir`). Sintaxe idêntica ao `.gitignore`, com suporte adicional a extglob (`!(*.txt)`, `*(src)`, etc.):

```gitignore
# .garraignore — não afeta o git, apenas o scanner do agente
target/
Cargo.lock
*.db
*.ps1
.env*
credentials/
```

## Arquitetura

GarraIA é um workspace Rust com **19 crates** de alta qualidade, cada um com responsabilidade única:

```text
crates/
├── garraia-cli/        # CLI, assistente de init, gerenciamento de daemon
├── garraia-gateway/    # Gateway WebSocket, API HTTP, admin console
├── garraia-config/     # Carregamento YAML/TOML, hot-reload, config MCP
├── garraia-channels/   # Discord, Telegram, Slack, WhatsApp, iMessage
├── garraia-agents/     # Provedores de LLM, ferramentas, cliente MCP, runtime do agente
├── garraia-auth/       # ✅ verify path real + extractor + endpoints + RLS matrix (GAR-391a/b/c + GAR-392) — IdentityProvider trait, InternalProvider, LoginPool/SignupPool BYPASSRLS newtypes, JWT HS256 (15min) + refresh HMAC, Argon2id+PBKDF2 dual-verify, Role/Action enums + fn can() (110-case test), Principal extractor + RequirePermission, RedactedStorageError. Migration 008/010 (login/signup roles). GAR-392 RLS matrix ✅ (plan 0013 path C, 81 cenários × 3 dedicated roles × 10 FORCE RLS tables). GAR-391d (app-layer cross-group matrix via HTTP) deferido ao plan 0014 — aguarda endpoints REST /v1/{chats,messages,memory,tasks,groups,me} da Fase 3.4; epic GAR-391 permanece aberto.
├── garraia-voice/      # Pipeline de voz: Whisper STT → LLM → Chatterbox/Hibiki TTS
├── garraia-tools/      # Trait Tool + ToolRegistry, execução com timeout
├── garraia-runtime/    # Executor com máquina de estados, meta-controller, gerenciador de turn
├── garraia-db/         # Memória SQLite, busca vetorial (sqlite-vec), sessões
├── garraia-glob/       # Glob pattern matching (picomatch + bash extglob), .garraignore, scanner de arquivos
├── garraia-plugins/    # Sandbox de plugins WASM (wasmtime)
├── garraia-media/      # Processamento de mídia: PDF, imagens
├── garraia-security/   # Cofre de credenciais, listas de permissões, pareamento, validação
├── garraia-skills/     # Parser de SKILL.md, scanner, instalador
├── garraia-common/     # Tipos compartilhados, erros, utilitários
├── garraia-telemetry/  # ✅ OpenTelemetry + Prometheus baseline (GAR-384) — feature-gated
├── garraia-workspace/  # ✅ Postgres 16 + pgvector multi-tenant — Fase 3 schema completo (25 tabelas em 8 migrations: 001/002/004/005/006/007/008/009)
└── garraia-desktop/    # Assistente desktop Clippy-style (Tauri v2) — overlay transparente, hotkey Alt+G, sprite animado
```

Além dos crates Rust, o repositório inclui o app mobile:

```text
apps/
└── garraia-mobile/     # Cliente Android/iOS Flutter — Garra Cloud Alpha
    ├── lib/
    │   ├── router/     # GoRouter com redirect JWT
    │   ├── services/   # Dio + interceptor Bearer
    │   ├── providers/  # Riverpod: AuthState, ChatMessages, MascotState
    │   ├── screens/    # Splash, Login, Register, Chat
    │   └── widgets/    # MascotWidget (4 estados), ChatBubble
    └── android/ ios/ web/
```

**Endpoints mobile (GAR-334/335/339):**

| Endpoint | Método | Descrição |
|----------|--------|-----------|
| `/auth/register` | POST | Criar conta — PBKDF2-HMAC-SHA256 (600k iter) |
| `/auth/login` | POST | Autenticar, retorna JWT 30 dias |
| `/me` | GET | Dados do usuário autenticado |
| `/chat` | POST | Conversa com Garra (personalidade PT-BR) |
| `/chat/history` | GET | Histórico dos últimos 50 turnos |

### Fluxo de Execução do Runtime

O [`garraia-runtime`](crates/garraia-runtime/src/lib.rs) gerencia o ciclo de vida completo da execução do agente:

```text
┌─────────────────────────────────────────────────────────────────┐
│                    GARRAIA RUNTIME FLOW                          │
├─────────────────────────────────────────────────────────────────┤
│  1. STATE MACHINE                                               │
│     ┌──────────┐    ┌──────────┐    ┌──────────┐             │
│     │  IDLE    │───▶│ RUNNING  │───▶│  DONE    │             │
│     └──────────┘    └──────────┘    └──────────┘             │
│         ▲               │                │                      │
│         └───────────────┴────────────────┘                      │
│                                                                 │
│  2. TURN EXECUTION                                              │
│     ┌─────────────┐  ┌─────────────┐  ┌─────────────┐        │
│     │  RECEIVE    │─▶│   EXECUTE   │─▶│   RESPOND   │        │
│     │  MESSAGE    │  │   TOOLS     │  │   STREAM    │        │
│     └─────────────┘  └─────────────┘  └─────────────┘        │
│                                                                 │
│  3. META CONTROLLER                                             │
│     - Gerenciamento de estado com history                       │
│     - Budget de execução (max_turns, timeouts)                  │
│     - Retry com backoff exponencial                             │
└─────────────────────────────────────────────────────────────────┘
```

### Pipeline de Voz (STT → LLM → TTS)

O [`garraia-voice`](crates/garraia-voice/src/lib.rs) implementa o pipeline de voz end-to-end:

```text
┌─────────────────────────────────────────────────────────────────┐
│                    VOICE PIPELINE                                │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│  ┌─────────┐    ┌─────────┐    ┌─────────┐    ┌─────────┐   │
│  │  AUDIO  │───▶│   STT   │───▶│   LLM   │───▶│   TTS   │   │
│  │  INPUT  │    │ Whisper │    │ Provider│    │Chatterbox│   │
│  └─────────┘    └─────────┘    └─────────┘    │  Hibiki  │   │
│                                                └─────────┘   │
│                                                                 │
│  STT Providers:          TTS Providers:                        │
│  - Whisper (local)       - Chatterbox (GPU, multilíngue)       │
│  - OpenAI Whisper API    - Hibiki (GPU)                        │
│                          - OpenAI TTS API                       │
│                                                                 │
│  Features:                                                      │
│  - Conversão de formato via ffmpeg                             │
│  - Streaming de áudio em tempo real                            │
│  - Suporte multilíngue (pt, en, es, fr, de, it, hi)           │
└─────────────────────────────────────────────────────────────────┘
```

### Arquitetura Multi-Agente

O GarraIA suporta múltiplos agentes com roteamento inteligente:

| Recurso | Descrição |
|---------|-----------|
| **Agent Registry** | Múltiplos agentes nomeados com configurações independentes |
| **Priority Router** | Roteamento baseado em prioridade (1-100) |
| **Session Continuity** | Sessões persistentes entre canais |
| **A2A Protocol** | Comunicação agent-to-agent via JSON-RPC 2.0 |
| **Agent Cards** | Auto-descoberta via `/.well-known/agent.json` |

### Suporte MCP (Model Context Protocol)

O GarraIA implementa o protocolo MCP com:

- **Transporte stdio** - Servidores MCP locais (processo filho)
- **Transporte HTTP / SSE / StreamableHttp** - Servidores MCP remotos (`mcp-http` feature)
- **Tool Bridging** - Ferramentas aparecem como `server.tool` namespaced
- **Resource API** - Arquivos, prompts, e custom resources
- **Health Monitor** - Auto-reconexão com verificação periódica (30s)
- **Admin API** - `GET /admin/api/mcp` lista servidores com status em tempo real; `POST /admin/api/mcp` adiciona novos servidores sem reiniciar
- **Diagnostic API** - `GET /api/mcp/tools` lista todas as tools ativas no AgentRuntime (built-ins + MCP); `GET /api/mcp/health` retorna status por servidor com contagem de tools e indicador `all_connected | partial | all_disconnected`
- **CLI Commands** - `garraia mcp list`, `mcp inspect`, `mcp resources`, `mcp prompts`

Configure em `config.yml` ou `~/.garraia/mcp.json` (compatível com Claude Desktop). Veja `mcp.json.example` para referência de formato sem tokens.

| Componente | Status |
|-----------|--------|
| Gateway (WebSocket, HTTP, admin console) | ✅ Funcionando |
| Telegram (streaming, comandos, pareamento) | ✅ Funcionando |
| Discord (comandos slash, sessões) | ✅ Funcionando |
| Slack (Socket Mode, streaming) | ✅ Funcionando |
| WhatsApp (webhooks) | ✅ Funcionando |
| iMessage (macOS, grupos) | ✅ Funcionando |
| Google Chat (Google Workspace) | ✅ Funcionando |
| Microsoft Teams (Bot Framework) | ✅ Funcionando |
| Matrix (federado, E2EE) | ✅ Funcionando |
| LINE (Messaging API) | ✅ Funcionando |
| IRC (multi-canal, multi-rede) | ✅ Funcionando |
| Signal (signal-cli) | ✅ Funcionando |
| Provedores de LLM (15: Anthropic, OpenAI, Ollama + 12 compatíveis com OpenAI) | ✅ Funcionando |
| Ferramentas do agente (bash, file_read, file_write, web_fetch, web_search, schedule_heartbeat) | ✅ Funcionando |
| Cliente MCP (stdio, HTTP/SSE/StreamableHttp, bridge de ferramentas, admin API) | ✅ Funcionando |
| Skills (SKILL.md, auto-descoberta) | ✅ Funcionando |
| Configuração (YAML/TOML, hot-reload) | ✅ Funcionando |
| Memória (SQLite, busca vetorial, facts.json) | ✅ Funcionando |
| Auto-learning (extrator LLM) | ✅ Funcionando |
| Embeddings locais (Ollama) | ✅ Funcionando |
| Segurança (cofre, lista de permissões, pareamento) | ✅ Funcionando |
| Agendamento (cron, intervalo, único) | ✅ Funcionando |
| Voice Mode (Chatterbox TTS, Hibiki TTS, Whisper STT) | ✅ Funcionando |
| Health checks centralizados (`/api/health`, boot table, background) | ✅ Funcionando |
| Timeouts configuráveis (LLM, TTS, MCP, Health) | ✅ Funcionando |
| CLI (init, start/stop/restart, update, migrate, mcp, skills, memory) | ✅ Funcionando |
| Sistema de plugins (Sandbox WASM) | ✅ Funcionando |
| MCP Marketplace (install, discover) | ✅ Funcionando |
| Skills Editor CRUD (API + WebChat UI) | ✅ Funcionando |
| OAuth2/OIDC + TOTP 2FA | ✅ Funcionando |
| EU AI Act Compliance (X-AI-Model headers) | ✅ Funcionando |
| TLS/HTTPS nativo | ✅ Funcionando |
| Processamento de mídia (PDF, imagens) | ✅ Funcionando |
| Garra Cloud Alpha — app mobile Flutter (Android/iOS) | ✅ Funcionando |
| Mobile Auth (register/login/me, JWT, PBKDF2) | ✅ Funcionando |
| Mobile Chat (`/chat`, `/chat/history`, persona PT-BR) | ✅ Funcionando |

## Testes Automatizados

O GarraIA utiliza o **TestSprite MCP** para geração e execução automatizada de testes da API do backend.
Os testes validam os contratos REST e o comportamento do sistema de forma contínua, garantindo estabilidade durante refatorações.

## Contribuindo

O GarraIA é código aberto sob licença MIT. Junte-se ao [Discord](https://discord.gg/aEXGq5cS) para conversar com contribuidores, fazer perguntas ou compartilhar o que você está construindo. Consulte [CONTRIBUTING.md](CONTRIBUTING.md) para instruções de configuração, diretrizes de código e visão geral dos crates.

### Roteiro de Desenvolvimento (Roadmap)

Acompanhe as próximas entregas e contribua através dos nossos **[Projects no Linear (time GarraIA-RUST)](https://linear.app/chatgpt25/team/GAR/projects)**. O plano completo está consolidado em [`ROADMAP.md`](ROADMAP.md) e distribuído em 7 fases:

1. **[Fase 1 — Core & Inferência](https://linear.app/chatgpt25/project/fase-1-core-and-inferencia-dc084beb8656)** — TurboQuant+ (KV cache, PagedAttention, quantização), Superpowers workflow, config reativo.
2. **[Fase 2 — Performance, RAG & MCP](https://linear.app/chatgpt25/project/fase-2-performance-rag-and-mcp-75d77421bfd6)** — Embeddings locais, vector store, plugins WASM sandboxed, OpenTelemetry.
3. **[Fase 3 — Group Workspace](https://linear.app/chatgpt25/project/fase-3-group-workspace-850d2a440e35)** — Multi-tenant família/equipe: arquivos, chats, memória IA, tasks, docs, RBAC com RLS Postgres. **Caminho crítico.**
4. **[Fase 4 — UX Multi-Plataforma AAA](https://linear.app/chatgpt25/project/fase-4-ux-multi-plataforma-aaa-b4f6bbe546c1)** — Desktop Tauri AAA, Mobile Android/iOS, CLI interativa.
5. **[Fase 5 — Qualidade, Segurança & Compliance](https://linear.app/chatgpt25/project/fase-5-qualidade-seguranca-and-compliance-f174cd2c73c0)** — Security hardening, fuzz, LGPD/GDPR, first-run wizard.
6. **[Fase 6 — Lançamento & SRE](https://linear.app/chatgpt25/project/fase-6-lancamento-and-sre-35277d8571eb)** — Helm, Terraform, SLOs, runbooks, beta → GA.
7. **[Fase 7 — Pós-GA & Evolução](https://linear.app/chatgpt25/project/fase-7-pos-ga-and-evolucao-14dc29a5f581)** — Multi-região, federation, marketplace, voice, vision, enterprise.

Marcos já entregues incluem Core Hardening, Voice E2E, Commands Registry, Admin Console, Garra Desktop overlay (Tauri v2 GAR-303..316), Garra Cloud Alpha (Flutter mobile GAR-334..345), bootstrap dos 7 projects AAA (GAR-371..410), **[GAR-384 — OpenTelemetry + Prometheus baseline](https://linear.app/chatgpt25/issue/GAR-384)** via o novo crate `garraia-telemetry` (Jaeger + Prometheus + Grafana via `ops/compose.otel.yml`, feature flag opt-out, PII redaction by design), **[GAR-373 — ADR 0003 Database para Group Workspace](https://linear.app/chatgpt25/issue/GAR-373)** que fixa **PostgreSQL 16 + pgvector + pg_trgm** como backend multi-tenant da Fase 3 (benchmark empírico em [`benches/database-poc/`](benches/database-poc/) provando 124x vantagem em ANN HNSW e validando RLS cross-group com FORCE ROW LEVEL SECURITY), **[GAR-407 — garraia-workspace bootstrap](https://linear.app/chatgpt25/issue/GAR-407)** que materializa a migration 001 (users, user_identities, sessions, api_keys, groups, group_members, group_invites + pgcrypto/citext) com smoke test testcontainers verde em ~7s e `Workspace` handle PII-safe, **[GAR-386 — Migration 002 RBAC + audit_events](https://linear.app/chatgpt25/issue/GAR-386)** que adiciona 5 roles × 22 permissions × 63 role_permissions seedados estaticamente, `audit_events` sem FK (sobrevive CASCADE para LGPD erasure demonstrável) e partial unique index `group_members_single_owner_idx`, **[GAR-388 — Migration 004 chats + messages + FTS](https://linear.app/chatgpt25/issue/GAR-388)** que adiciona `chats`, `chat_members`, `messages` (com `body_tsv tsvector GENERATED STORED` + GIN index + compound FK `(chat_id, group_id)` contra cross-group drift) e `message_threads`, e o **schema set completo da Fase 3** através de **[GAR-389](https://linear.app/chatgpt25/issue/GAR-389)** (memory_items + memory_embeddings com pgvector HNSW cosseno), **[GAR-408](https://linear.app/chatgpt25/issue/GAR-408)** (Row-Level Security FORCE em 10 tabelas com NULLIF fail-closed + prova empírica de FORCE via ownership transfer scopeguard-safe + hard blocker documentado para GAR-391 login flow) e **[GAR-390](https://linear.app/chatgpt25/issue/GAR-390)** (8 tabelas do módulo Tasks Tier 1 Notion-like — listas/tasks/subtasks/assignees/labels/comments/subscriptions/activity — com RLS FORCE embutido na própria migration e erasure survival via `created_by_label`/`author_label`/`actor_label` cached). **Atualização 2026-04-13:** GAR-391c shipped — Axum `Principal` extractor + `RequirePermission(Action)` + `Role`/`Action` enums + `fn can()` central com 110-case table-driven test + endpoints `/v1/auth/{login,refresh,logout,signup}` wired no `AppState` real (feature flag `auth-v1` removida) + `garraia_signup NOLOGIN BYPASSRLS` role + `SignupPool` newtype + `RedactedStorageError` wrapper + `AuthConfig` em `garraia-config` + métricas Prometheus baseline + **migration 010** com `GRANT SELECT ON sessions TO garraia_login` (Gap A), `GRANT SELECT ON group_members TO garraia_login` (Gap C), e role `garraia_signup` separado (Gap B). Próximo: GAR-392 / 391d (suite cross-group authz ≥100 cenários) fecha o epic GAR-391.

A Fase 3.3 destravou em 2026-04-13 com **[GAR-375 — ADR 0005 Identity Provider](https://linear.app/chatgpt25/issue/GAR-375)** (BYPASSRLS dedicated role + Argon2id RFC 9106 + HS256 JWT + lazy upgrade dual-verify PBKDF2→Argon2id, trait `IdentityProvider` shape congelada) e **GAR-391a — `garraia-auth` skeleton** (crate skeleton + migration 008 criando `garraia_login NOLOGIN BYPASSRLS` com 4 GRANTs exatos do ADR 0005 + `LoginPool` newtype com `current_user` validation + `static_assertions::assert_not_impl_all!(LoginPool: Clone)` + smoke tests integration). Migration 009 (prereq estrutural de GAR-391b) adicionou `user_identities.hash_upgraded_at` para o lazy upgrade transacional. Próximo: **GAR-391b** (`verify_credential` real impl + audit + JWT issuance + endpoint `/v1/auth/login` sob feature flag).

Navegue por todas as [issues abertas no Linear](https://linear.app/) ou filtre por [`good-first-issue`](https://github.com/michelbr84/GarraRUST/issues?q=label%3Agood-first-issue+is%3Aopen) no GitHub para encontrar um lugar para começar.

## Licença

MIT
