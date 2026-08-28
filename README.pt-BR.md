<!-- markdownlint-disable MD033 MD041 MD060 -->

<p align="right"><a href="README.md">🇺🇸 English</a> · <strong>🇧🇷 Português</strong></p>

<p align="center">
  <img src="assets/logo.png" alt="GarraIA" width="280" />
</p>

<h1 align="center">GarraIA</h1>

<p align="center">
  <strong>O framework seguro e leve de código aberto para agentes de IA.</strong>
</p>

<p align="center">
  <a href="https://github.com/michelbr84/GarraRUST/actions"><img src="https://github.com/michelbr84/GarraRUST/actions/workflows/ci.yml/badge.svg?branch=main" alt="CI"></a>
  <a href="https://github.com/michelbr84/GarraRUST/actions/workflows/codeql.yml"><img src="https://github.com/michelbr84/GarraRUST/actions/workflows/codeql.yml/badge.svg?branch=main" alt="CodeQL"></a>
  <a href="https://github.com/michelbr84/GarraRUST/actions/workflows/cargo-audit.yml"><img src="https://github.com/michelbr84/GarraRUST/actions/workflows/cargo-audit.yml/badge.svg?branch=main" alt="Security Audit"></a>
  <a href="https://github.com/michelbr84/GarraRUST/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="Licença: MIT"></a>
  <a href="https://github.com/michelbr84/GarraRUST/stargazers"><img src="https://img.shields.io/github/stars/michelbr84/GarraRUST" alt="Estrelas"></a>
  <a href="https://github.com/michelbr84/GarraRUST/issues"><img src="https://img.shields.io/github/issues/michelbr84/GarraRUST" alt="Issues"></a>
  <a href="https://github.com/michelbr84/GarraRUST/issues?q=label%3Agood-first-issue+is%3Aopen"><img src="https://img.shields.io/github/issues/michelbr84/GarraRUST/good-first-issue?color=7057ff&label=good%20first%20issues" alt="Boas Primeiras Issues"></a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/rust-1.94%2B-orange?logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/license-MIT-blue" alt="License">
  <img src="https://img.shields.io/badge/crates-22-green" alt="Crates">
  <img src="https://img.shields.io/badge/channels-5%20wired-purple" alt="Channels">
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

**O assistente de IA brasileiro que roda na sua máquina.** Um único binário nativo (47 MiB medidos, LTO + strip) que executa seus agentes de IA no Telegram, Discord, Slack, WhatsApp e iMessage — com cofre de credenciais AES-256-GCM opcional, recarregamento de configuração a quente, sistema completo de memória e pico de 8,6 MiB de RAM na invocação do CLI ([medições versionadas](benches/agent-framework-comparison/results/)). Desenvolvido em Rust para a segurança e confiabilidade que os agentes de IA exigem.

**Local-first** — Todo o estado (conversas, memória, config, credenciais) fica na sua máquina, sem telemetria. Seus prompts vão apenas para o provedor de LLM que você configurar — use Ollama para um setup 100% offline, sem egress.

<!-- TODO: Adicionar GIF de demonstração do terminal VHS aqui (#103) -->

## 🐾 Conheça o Garra

O **Garra** não é só um endpoint de API — ele é o seu assistente pessoal, e fala
como um. Desde o primeiro `garra start`, ele se apresenta com nome, conversa em
português do Brasil na primeira pessoa e mantém um tom caloroso, direto e honesto
(sem bajulação). Quando algo dá errado, ele explica o que aconteceu e qual é o
próximo passo — nada de despejar códigos de erro crus.

> _"Oi! 👋 Eu sou o Garra, seu assistente pessoal. Pode falar comigo como você
> falaria com um amigo."_

Essa personalidade é **o padrão**, mas você manda: defina `agent.system_prompt`
para dar a ele uma personalidade própria, ou `agent.persona = "neutral"` para um
tom totalmente neutro. Veja [ADR 0012](docs/adr/0012-garra-persona.md).

## 🗺️ Roadmap AAA

O desenvolvimento do GarraRUST segue um plano ambicioso de evolução para o tier AAA em 7 fases, consolidado no [ROADMAP.md](ROADMAP.md). Inclui GarraMaxPower nativo (`garra max-power`, registry de skills e Agent Team MVP), Superpowers, TurboQuant+ (KV cache), RAG local (lancedb), MCP + plugins WASM, zero-latency streaming (OpenTelemetry), e a nova direção **Group Workspace** — espaço compartilhado família/equipe multi-tenant com arquivos, chats, memória IA e módulo tipo-Notion (tasks + docs + databases), desenhado em [`deep-research-report.md`](deep-research-report.md). Planejamento interno migrado do Linear para o tracker interno em 2026-08-18; o acompanhamento fase a fase vive em [ROADMAP.md](ROADMAP.md).

Nota de sincronizacao (2026-05-24): a decisao GarraMaxPower esta formalizada em [ADR 0011](docs/adr/0011-garra-max-power.md), e o backlog operacional curto agora vive em [`TODO.md`](TODO.md) para pendencias seguras do `ROADMAP.md` (planejamento interno no tracker interno desde 2026-08-18).

## Início Rápido

```bash
# Requer Rust 1.94+ (alinhado com MSRV declarado em Cargo.toml — GAR-895)
cargo build --release -p garraia

# Configuração interativa - escolha seu provedor de LLM; opcionalmente
# armazene chaves no cofre criptografado (default do wizard: config.yml 0600)
./target/release/garra init

# Iniciar
./target/release/garra start

# Conversa rápida não-interativa (GAR-579) — ideal para Claude Code, CI, scripts
./target/release/garra ask --provider openrouter --model openrouter/free \
  --json --timeout-secs 30 "Responda apenas: GAR-ASK-OK"

# MCP server stdio (GAR-583) — expõe `garra_ask` para Claude Desktop / Claude Code
./target/release/garra mcp-server   # ver docs/cli-mcp-server.md

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
cp target/release/garra crates/garraia-desktop/src-tauri/binaries/garra-$(rustc -vV | grep host | cut -d' ' -f2)

# 3. Compilar o desktop
cargo build --release -p garraia-desktop
```

</details>

<details>
<summary>Instalar via script (Linux, macOS) — usa binários publicados no release</summary>

```bash
curl -fsSL https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh | sh
```

> Se o `raw.githubusercontent.com` responder **HTTP 429** (rate limit por IP —
> comum em pods cloud com IP de saída compartilhado), o mesmo script está
> publicado em dois canais alternativos:
>
> ```bash
> curl -fsSL https://github.com/michelbr84/GarraRUST/releases/latest/download/install.sh | sh
> curl -fsSL https://cdn.jsdelivr.net/gh/michelbr84/GarraRUST@main/install.sh | sh
> ```

Desde plan 0127 (PR-B, 2026-05-14) o instalador encadeia automaticamente:
1. download + verificação SHA256 do binário `garraia`,
2. `garraia init </dev/tty` (o wizard do plan 0126 — detecção de GPU/Ollama, prompts opcionais para instalar Qwen3-14B GGUF, geração de `config.yml` server-friendly),
3. `garraia start </dev/tty` em foreground.

**Toggles** (env vars, todos opt-out):
- `GARRAIA_SKIP_INIT=1` — pula o wizard.
- `GARRAIA_SKIP_START=1` — pula o `garraia start` final.
- `GARRAIA_BOOTSTRAP_LOCAL=0` — suprime os prompts de GPU/Ollama dentro do wizard, mesmo com `nvidia-smi` disponível.

Em contextos sem TTY real (docker build, CI puro), o instalador imprime os "Next steps" legados e sai com código 0 — nunca trava aguardando input.

> A partir de `v0.2.1` (2026-05-14) — primeira release **não-prerelease** do repo — o script consome `GET /repos/michelbr84/GarraRUST/releases/latest` e verifica o binário baixado contra o arquivo `SHA256SUMS` da release (um único manifesto para todos os assets, via `sha256sum -c`). O formato `<asset>.sha256` é usado pelo `garraia update`, não pelo `install.sh`.
> Em ARM, certifique-se de que `uname -m` reporta `aarch64`/`arm64` — os assets `garraia-linux-aarch64` e `garraia-macos-aarch64` são publicados normalmente desde a **v0.3.2** (2026-08-18) — o cross-compile ARM64 foi resolvido com `cross` moderno + sqlx em rustls ([release.yml](.github/workflows/release.yml)).

</details>

<details>
<summary>Atualizar uma instalação existente — <code>garraia update</code></summary>

```bash
# Verifica a release mais recente, baixa o binário da sua plataforma,
# confere SHA-256 contra <asset>.sha256 e troca o executável atomicamente.
garraia update          # interativo
garraia update --yes    # não interativo (CI)

# Volta para o binário anterior se algo der errado:
garraia rollback
```

> `garraia update` falava com **404** em todas as versões anteriores a `v0.2.1` porque toda release publicada era marcada como prerelease (e o endpoint `releases/latest` ignora prereleases). Detalhamento em [`CHANGELOG.md`](CHANGELOG.md#021---2026-05-14).

</details>

As releases atuais (v0.3.x) publicam os 5 binários CLI (Linux x86_64/aarch64, macOS x86_64/aarch64, Windows `.exe`); instaladores desktop (Windows `.msi`) e mobile (Android `.apk`) foram publicados até a v0.2.1, nas [Versões do GitHub](https://github.com/michelbr84/GarraRUST/releases).

## Por que GarraIA?

### vs OpenClaw, ZeroClaw e outros frameworks de agentes de IA

Números **medidos** (não afirmados) pelo harness versionado em
[benches/agent-framework-comparison](benches/agent-framework-comparison/) —
cenários 001–005, evidência bruta em
[`results/2026-08-28-vm/`](benches/agent-framework-comparison/results/2026-08-28-vm/).
Performance medida em `openclaw@2026.7.1-2` (npm `latest` na run) e num
clone fresco do `master` do ZeroClaw (`d355e3b`); a auditoria de
segurança usa checkouts de inspeção pinados por commit (OpenClaw
`343252a`, ZeroClaw `d5617f1`):

| | **GarraIA** | **OpenClaw** (Node.js) | **ZeroClaw** (Rust) |
|---|---|---|---|
| **Footprint instalado** | 47 MiB (binário único) | 370 MiB `node_modules` + runtime Node ≥22.22.3 | 40 MiB (binário, build default) |
| **Pico de RSS (`--help`)** | **8,6 MiB** | 49,2 MiB | 15,3 MiB |
| **Início a frio (`--help`)** | **4,1 ms** | 46,2 ms | 8,5 ms |
| **Credenciais em repouso** | Cofre AES-256-GCM disponível, **opt-in** — default do wizard é `config.yml` (0600) | Sem criptografia em repouso (docs deles) — permissões POSIX; SecretRefs/1Password opt-in | ChaCha20-Poly1305 **por default** (única postura cifrada por default das três); chave no mesmo disco; refs 1Password opt-in |
| **Auth default do gateway** | Canais deny-by-default (pareamento); API local aberta no loopback, token opt-in | Token exigido out-of-the-box (fail-closed) | Pareamento exigido por default; bind público só avisa |
| **Bind default** | 127.0.0.1 | loopback | 127.0.0.1 |
| **Dependências** | 1.061 crates (Cargo.lock) | 66 deps diretas de produção (~377 no fechamento) | 1.265 crates (Cargo.lock) |
| **Canais** | 5 ligados fim-a-fim (+6 implementados no crate, sem wiring) | 27 plugins bundled | ~40 adapters (6 no build default) |
| **Provedores de LLM** | 15 built-in (100+ modelos via OpenRouter) | via plugins | vários, feature-gated |
| **Agendamento** | Tarefas one-shot persistidas (cron no roadmap) | Cron/automations completo | Cron + SOP engine |
| **Suporte MCP** | Stdio + Streamable HTTP | Stdio/SSE/StreamableHttp + modo servidor | Stdio/http/sse, escopo fail-closed por agente |
| **Recarregamento de config a quente** | Sim (file watch) | Sim (modo híbrido) | Só endpoint explícito |
| **Plugins WASM (sandbox)** | Opcional (wasmtime) | Não (in-process, confiável) | Sim (component model; assinatura default off) |
| **Workspace multi-tenant (Postgres RLS)** | Em desenvolvimento (Fase 3) | Não-objetivo declarado | Não |
| **Persona nativa PT-BR** | ✅ Sim | Não | Não |

**Onde os outros ganham, honestamente:** OpenClaw tem o maior ecossistema
(27 canais, 150+ extensões, cron maduro) e exige auth no gateway
out-of-the-box; ZeroClaw cifra secrets por default, publica 10 targets
pré-compilados com proveniência SLSA e tem sandbox de exec a nível de SO.
Se esta tabela algum dia parecer marketing, abra uma issue citando o
cenário — corrigimos a tabela, não o resultado.

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
- **iMessage** - nativo macOS via polling de chat.db, grupos de chat, envio via AppleScript ([guia de configuração](docs/src/channels/imessage.md))
- **VS Code** - via API OpenAI-compatible, integrado ao mesmo histórico de conversas

Implementados no crate `garraia-channels`, **aguardando wiring no
gateway** (acompanhe no [ROADMAP](ROADMAP.md)): Google Chat, Microsoft
Teams, Matrix (com E2EE), LINE, IRC e Signal.

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
- **Ativação** - `garra start --with-voice` habilita o modo de voz
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
- CLI: `garra mcp list`, `garra mcp inspect <name>`

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

Consulte a [documentação completa de integração com Continue](docs/src/continue-modes.md).

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
- Tarefas agendadas - one-shot persistidas em SQLite (heartbeats de até 30 dias; recorrência cron no roadmap)

### Skills

- Defina skills de agente como arquivos Markdown (SKILL.md) com frontmatter YAML
- Auto-descoberta de `~/.garraia/skills/` - injetado no prompt do sistema
- CLI: `garra skill list`, `garra skill install <url>`, `garra skill remove <name>`

### MCP Tool Integration com Marketplace

- Conecte qualquer servidor compatível com MCP (filesystem, GitHub, bancos de dados, busca na web)
- **Marketplace de ferramentas** - descubra e instale servidores MCP pelo Web Console (`/api/mcp/marketplace`) ou pela API admin; CLI: `garra mcp list|inspect|resources|prompts`
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
- CLI: `garra skill list`, `garra skill install <url>`, `garra skill remove <name>`
- CRUD completo via API REST (`GET/POST/PATCH/DELETE /api/skills`)

### Autenticacao OAuth2/OIDC + TOTP 2FA

- **OAuth2/OIDC** - suporte a provedores externos de identidade
- **TOTP 2FA** - autenticacao de dois fatores via aplicativo (Google Authenticator, Authy)
- **JWT** - dois stacks, sem confusão: Auth v1 (`/v1/auth/*`) usa access tokens HS256 de 15 minutos + refresh token opaco com HMAC; os endpoints mobile legados (`/auth/*`) ainda emitem JWT de 30 dias e estão sendo migrados para o v1
- **PBKDF2-HMAC-SHA256** - 600k iteracoes para hash de senhas
- **Pareamento por codigo** - whitelist de usuarios por canal

### EU AI Act Compliance

- **Headers X-AI-Model** - todas as respostas incluem o modelo usado (`X-AI-Model`, `X-AI-Provider`)
- **Transparencia** - identificacao clara de conteudo gerado por IA
- **Logging auditavel** - registros estruturados de todas as interacoes com LLMs

### TLS/HTTPS (builds de fonte)

- **Suporte TLS** - compile com `--features tls` e aponte `tls_cert_path`/`tls_key_path` para seus certificados (ex.: emitidos via certbot/Let's Encrypt). Não há cliente ACME embutido. Caveats honestos: os binários release atuais **não** incluem a feature TLS, e com certs configurados mas sem a feature o gateway loga warning e serve HTTP puro — ambos itens de hardening no roadmap. Para produção, recomenda-se reverse proxy com TLS na frente do bind loopback.
- **Binding seguro** - `127.0.0.1` por padrao, `0.0.0.0` com TLS para producao

### Health Checks Centralizados

- **Boot** - tabela visual no terminal com ✅/❌ e latência por provider
- **Endpoint** - `GET /api/health` retorna JSON com status de todos os providers
- **Background** - verificação periódica (60s) com detecção de mudança de status
- **Providers** - Ollama, OpenRouter, OpenAI, Anthropic, Chatterbox TTS
- **Cache** - resultados cacheados para respostas instantâneas no endpoint

### Infraestrutura

- **Recarregamento de config a quente** - edite `config.yml`, as alterações são aplicadas sem reiniciar
- **Daemonização** - `garra start --daemon` com gerenciamento de PID
- **Auto-atualização** - `garraia update` baixa a versão mais recente com verificação SHA-256, `garraia rollback` para reverter
- **Reinicialização** - `garra restart` para graciosamente parar e iniciar o daemon
- **Troca de provedor em runtime** - adicione ou troque provedores de LLM via interface webchat ou API REST sem reiniciar
- **Fallback automático de providers** - em caso de erro 429/5xx, tenta automaticamente o próximo provider configurado em `fallback_providers` com backoff exponencial e circuit breaker
- **Timeouts configuráveis** - timeouts por tipo (LLM: 30s, TTS: 120s, MCP: 60s, Health: 5s) via `config.yml`
- **Rate limiting por IP** - proteção automática configurável (`per_second`, `burst_size`) via `config.yml`
- **Logs estruturados** - campos rastreáveis (`request_id`, `session_id`, `source`, `model`, `latency_ms`); JSON format via `GARRAIA_LOG_FORMAT=json`
- **Ferramenta de migração** - `garra migrate openclaw` importa skills e canais (credenciais são reinseridas via `garra init`)
- **Configuração interativa** - `garra init` wizard para configuração de provedor e chave de API

## Web Console "Garra Glass"

Servido em `GET /` pelo binário `garraia start`, o Web Console é uma SPA sem build step (HTML + CSS custom properties + JS vanilla) com identidade visual "Garra Glass" — glassmorphism com `backdrop-filter: blur(18px)`, gradiente multi-radial ouro/cyan/roxo, acentos `#ffd400` (gold) para CTAs e `#16d9ff` (cyan) para foco. **Zero dependência CDN** — todos os ícones inline SVG, fontes via Google Fonts (cacheável offline). Dual `data-theme` + `data-bs-theme` (compatível com migrações futuras estilo AdminLTE).

**9 páginas** roteadas por hash (`#/dashboard`, `#/chat`, ...), todas consumindo dados reais do gateway:

| Página | Endpoint principal | Recursos |
| --- | --- | --- |
| **Dashboard** | `/api/health` + `/api/capabilities` | Hero card + MetricCards (port, providers, channels, sessions, secrets=0) + Health checklist |
| **Chat** | `/ws` + `/api/sessions/*` | Conversa em tempo real — superfície original, redesign Garra Glass com avatares cyan/ouro e gold send button |
| **Providers & Models** | `/api/providers` + `POST /api/providers/test` + `PATCH /api/providers/default` | Cards por provider com Test/Set-default, sem expor API keys |
| **Channels** | `/api/channels` | 10 canais (web/api/telegram/discord/slack/whatsapp/imessage/openclaw/mcp/cli) com status pill |
| **Sessions** | `/api/sessions` + `/api/sessions/{id}/history` | Tabela com Open/Export (blob)/Delete |
| **Settings Registry** | `/api/settings/schema` + `/api/settings/effective` + `PATCH /api/settings` | Editor schema-driven, **secrets write-only** (`configured: true\|false`), dry-run |
| **Diagnostics** | `/api/diagnostics` | 12 checks (gateway/port/config/.env/provider/canais/secrets/bind/TLS/sessions) + copy report |
| **Logs** | `/api/logs` | Filtro por nível + search + auto-scroll + Export blob |
| **Themes & Skins** | localStorage (server-side em plan 0121a) | 4 skins (Garra Blue / Aurora Admin / Editorial / Cyber Garra) |

**Segurança invariantes:**

- Nenhum secret (API key, JWT secret, refresh HMAC) **jamais** é retornado por qualquer endpoint `/api/*` — apenas `configured: true\|false`.
- `PATCH /api/settings` valida cada campo contra o schema, rejeita ids desconhecidos, registra audit log estruturado sem o valor, e é **dry-run** até plan 0121a (zero risco de corromper `garraia.toml`).
- Sidebar dark intencional (`linear-gradient(#031126 → #061b3d)`) em ambos os temas para reforçar a identidade.

Decisões de design em [ADR 0009](docs/adr/0009-web-console-design-system.md). Plans `0116a`, `0116b`, `0117`-`0123` em [`plans/`](plans/).

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

### Gerenciando a memória

A memória é gerenciada pelo Web Console e pela API do gateway (listar,
buscar por similaridade, adicionar, limpar e exportar fatos). Um
subcomando `garra memory` na CLI está no roadmap.

## Segurança

O GarraIA foi desenvolvido para os requisitos de segurança de agentes de IA que ficam sempre ativos, acessam dados privados e se comunicam externamente.

- **Cofre de credenciais criptografadas (opt-in)** - AES-256-GCM em `~/.garraia/credentials/vault.json`, chave derivada via PBKDF2-HMAC-SHA256 (600k iterações) de `GARRAIA_VAULT_PASSPHRASE`. Dito com clareza: o default recomendado do wizard `garra init` grava as chaves de provider em `config.yml` (modo 0600, texto puro); escolha a opção do cofre e exporte a passphrase a cada start para ter criptografia em repouso. Tornar o cofre o default está no roadmap.
- **Tokens MCP protegidos por vault** - Variáveis de ambiente sensíveis dos servidores MCP (`API_KEY`, `TOKEN`, `SECRET`, etc.) são automaticamente movidas para o vault no primeiro `save`. O `mcp.json` armazena apenas referências `vault:mcp.<server>.<key>`. Sem `GARRAIA_VAULT_PASSPHRASE`, salva em plaintext com aviso — nunca quebra o boot.
- **Tokens de sessão criptograficamente seguros** - Cada sessão WebSocket recebe um token de 256 bits (URL-safe base64). Suportados via cookie `garraia_session` (HttpOnly, SameSite=Strict), header `Authorization: Bearer` ou `X-Session-Key`. TTL e idle-timeout configuráveis. Rotação automática no resume.
- **Canais deny-by-default** - Usuários desconhecidos nos canais de mensageria precisam apresentar código de pareamento. A API local (WS/HTTP) faz bind em `127.0.0.1` e é aberta por default — habilite `gateway.api_key` e/ou `session_tokens_required: true` para exigir auth nela (endurecer esse default está no roadmap).
- **Listas de permissões por usuário** - Listas de permissões por canal controlam quem pode interagir com o agente. Mensagens não autorizadas são descartadas silenciosamente.
- **Filtragem heurística de entrada** - Saneamento de caracteres de controle + triagem por palavras-chave de frases comuns de prompt injection nos canais de chat e no WebSocket. É heurística, não garantia — trate prompt injection como problema em aberto, como todo framework deveria.
- **Confirmação de comandos arriscados** - `tool_confirmation_enabled: true` pausa o agente antes de executar comandos bash destrutivos (`rm -r`, `git reset --hard`, `drop database`, etc.) e aguarda aprovação do usuário ("sim"/"yes"). Default: `false` (opt-in).
- **Limites de recursos para processos MCP** - Cap opcional de memória virtual por servidor (Unix, via `setrlimit`; desligado por default), timeout de inicialização e restart automático com backoff exponencial (base × 2ⁿ, cap 300s). São limites de recursos, não sandbox: processos MCP mantêm acesso a filesystem/rede.
- **Sandbox WASM** - Plugin opcional em sandbox via runtime WebAssembly com acesso controlado ao host (compile com `--features plugins`).
- **Binding apenas em localhost** - Gateway faz bind em `127.0.0.1` por padrão, não em `0.0.0.0`.

### Arquitetura Local e Sob Controle do Usuário

O GarraIA foi projetado para funcionar 100% no seu computador:

- **Sem dependência de nuvem** - Execute tudo localmente
- **Seus dados são seus** - Conversas, facts e configurações ficam no seu PC
- **Sem telemetria** - Nenhum phone-home de analytics; seus prompts vão apenas para o provedor de LLM configurado (Ollama = zero egress)
- **Controle total** - Você decide onde e como executar
- **Offline capable** - Funciona com modelos locais Ollama sem internet

## Migrando do OpenClaw?

Um comando importa suas skills e configurações de canais:

```bash
garra migrate openclaw
```

Use `--dry-run` para visualizar as alterações antes de confirmar. Use `--source /caminho/para/openclaw` para especificar um diretório de configuração personalizado do OpenClaw. Arquivos de credenciais são detectados e listados, mas **não copiados** — reinsira as chaves de API via `garra init` para que entrem no cofre criptografado.

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
  # ATENÇÃO sobre `api_key`: a resolução é vault > config > variável de ambiente.
  # O tier do vault só funciona quando GARRAIA_VAULT_PASSPHRASE está presente no
  # ambiente do gateway, a CADA start — sem ela o cofre fica ilegível e o provider
  # é PULADO no boot (`skipping <tipo> provider <nome>: no API key`). Por isso o
  # `garraia init` grava a chave no próprio config.yml (criado com modo 0600).
  # Rode `garraia config check` para confirmar que cada provider resolve.
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

GarraIA é um workspace Rust com **22 crates** de alta qualidade, cada um com responsabilidade única:

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
├── garraia-workspace/  # ✅ Postgres 16 + pgvector multi-tenant — Fase 3 (37 tabelas em 32 migrations, FORCE RLS nos dados de tenant)
├── garraia-embeddings/ # Traits EmbeddingProvider/VectorStore + DeterministicProvider (Fase 2.1)
├── garraia-learning/   # Garra Learning Agent — miner/generator/safety gate/versioning (Fase 1.4)
├── garraia-storage/    # ObjectStore: LocalFs + S3 (SSE-S3, HMAC integrity, presigned URLs)
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
- **Transporte Streamable HTTP** - Servidores MCP remotos (`mcp-http` feature). A config aceita `http`/`sse`/`streamable-http` como valores, todos servidos pelo cliente Streamable HTTP — servidores legados SSE-only não são suportados
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
| Google Chat (Google Workspace) | 🧩 Implementado no crate — wiring no gateway pendente |
| Microsoft Teams (Bot Framework) | 🧩 Implementado no crate — wiring no gateway pendente |
| Matrix (federado, E2EE) | 🧩 Implementado no crate — wiring no gateway pendente |
| LINE (Messaging API) | 🧩 Implementado no crate — wiring no gateway pendente |
| IRC (multi-canal, multi-rede) | 🧩 Implementado no crate — wiring no gateway pendente |
| Signal (signal-cli) | 🧩 Implementado no crate — wiring no gateway pendente |
| Provedores de LLM (15: Anthropic, OpenAI, Ollama + 12 compatíveis com OpenAI) | ✅ Funcionando |
| Ferramentas do agente (bash, file_read, file_write, web_fetch, web_search, schedule_heartbeat) | ✅ Funcionando |
| Cliente MCP (stdio + Streamable HTTP, bridge de ferramentas, admin API) | ✅ Funcionando |
| Skills (SKILL.md, auto-descoberta) | ✅ Funcionando |
| Configuração (YAML/TOML, hot-reload) | ✅ Funcionando |
| Memória (SQLite, busca vetorial, facts.json) | ✅ Funcionando |
| Auto-learning (extrator LLM) | ✅ Funcionando |
| Embeddings locais (Ollama) | ✅ Funcionando |
| Segurança (cofre, lista de permissões, pareamento) | ✅ Funcionando |
| Agendamento (tarefas one-shot persistidas; cron no roadmap) | ✅ Funcionando |
| Voice Mode (Chatterbox TTS, Hibiki TTS, Whisper STT) | ✅ Funcionando |
| Health checks centralizados (`/api/health`, boot table, background) | ✅ Funcionando |
| Timeouts configuráveis (LLM, TTS, MCP, Health) | ✅ Funcionando |
| CLI (init, start/stop/restart, update, migrate, mcp, skills, ask, chat, config) | ✅ Funcionando |
| Sistema de plugins (Sandbox WASM) | ✅ Funcionando |
| MCP Marketplace (discover + install via Web Console/API) | ✅ Funcionando |
| Skills Editor CRUD (API + WebChat UI) | ✅ Funcionando |
| OAuth2/OIDC + TOTP 2FA | ✅ Funcionando |
| EU AI Act Compliance (X-AI-Model headers) | ✅ Funcionando |
| TLS/HTTPS (`--features tls`; fora dos binários release atuais) | ✅ Funcionando |
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

Acompanhe as próximas entregas em [`ROADMAP.md`](ROADMAP.md) — o planejamento interno migrou do Linear para o tracker interno em 2026-08-18. O plano completo está distribuído em 7 fases:

1. **Fase 1 — Core & Inferência** — TurboQuant+ (KV cache, PagedAttention, quantização), GarraMaxPower nativo (skills registry + agent team MVP), Superpowers workflow, config reativo.
2. **Fase 2 — Performance, RAG & MCP** — Embeddings locais, vector store, plugins WASM sandboxed, OpenTelemetry.
3. **Fase 3 — Group Workspace** — Multi-tenant família/equipe: arquivos, chats, memória IA, tasks, docs, RBAC com RLS Postgres. **Caminho crítico.**
4. **Fase 4 — UX Multi-Plataforma AAA** — Desktop Tauri AAA, Mobile Android/iOS, CLI interativa.
5. **Fase 5 — Qualidade, Segurança & Compliance** — Security hardening, fuzz, LGPD/GDPR, first-run wizard.
6. **Fase 6 — Lançamento & SRE** — Helm, Terraform, SLOs, runbooks, beta → GA.
7. **Fase 7 — Pós-GA & Evolução** — Multi-região, federation, marketplace, voice, vision, enterprise.

Marcos já entregues incluem Core Hardening, Voice E2E, Commands Registry, Admin Console, Garra Desktop overlay (Tauri v2 GAR-303..316), Garra Cloud Alpha (Flutter mobile GAR-334..345), bootstrap dos 7 projects AAA (GAR-371..410), **GAR-384 — OpenTelemetry + Prometheus baseline** via o novo crate `garraia-telemetry` (Jaeger + Prometheus + Grafana via `ops/compose.otel.yml`, feature flag opt-out, PII redaction by design), **GAR-373 — ADR 0003 Database para Group Workspace** que fixa **PostgreSQL 16 + pgvector + pg_trgm** como backend multi-tenant da Fase 3 (benchmark empírico no PoC `benches/database-poc/` — removido em 2026-08-16 após estabilização, números preservados no ADR 0003 — provando 124x vantagem em ANN HNSW e validando RLS cross-group com FORCE ROW LEVEL SECURITY), **GAR-407 — garraia-workspace bootstrap** que materializa a migration 001 (users, user_identities, sessions, api_keys, groups, group_members, group_invites + pgcrypto/citext) com smoke test testcontainers verde em ~7s e `Workspace` handle PII-safe, **GAR-386 — Migration 002 RBAC + audit_events** que adiciona 5 roles × 22 permissions × 63 role_permissions seedados estaticamente, `audit_events` sem FK (sobrevive CASCADE para LGPD erasure demonstrável) e partial unique index `group_members_single_owner_idx`, **GAR-388 — Migration 004 chats + messages + FTS** que adiciona `chats`, `chat_members`, `messages` (com `body_tsv tsvector GENERATED STORED` + GIN index + compound FK `(chat_id, group_id)` contra cross-group drift) e `message_threads`, e o **schema set completo da Fase 3** através de **GAR-389** (memory_items + memory_embeddings com pgvector HNSW cosseno), **GAR-408** (Row-Level Security FORCE em 10 tabelas com NULLIF fail-closed + prova empírica de FORCE via ownership transfer scopeguard-safe + hard blocker documentado para GAR-391 login flow) e **GAR-390** (8 tabelas do módulo Tasks Tier 1 Notion-like — listas/tasks/subtasks/assignees/labels/comments/subscriptions/activity — com RLS FORCE embutido na própria migration e erasure survival via `created_by_label`/`author_label`/`actor_label` cached). **Atualização 2026-04-13:** GAR-391c shipped — Axum `Principal` extractor + `RequirePermission(Action)` + `Role`/`Action` enums + `fn can()` central com 110-case table-driven test + endpoints `/v1/auth/{login,refresh,logout,signup}` wired no `AppState` real (feature flag `auth-v1` removida) + `garraia_signup NOLOGIN BYPASSRLS` role + `SignupPool` newtype + `RedactedStorageError` wrapper + `AuthConfig` em `garraia-config` + métricas Prometheus baseline + **migration 010** com `GRANT SELECT ON sessions TO garraia_login` (Gap A), `GRANT SELECT ON group_members TO garraia_login` (Gap C), e role `garraia_signup` separado (Gap B). Próximo: GAR-392 / 391d (suite cross-group authz ≥100 cenários) fecha o epic GAR-391.

A Fase 3.3 destravou em 2026-04-13 com **GAR-375 — ADR 0005 Identity Provider** (BYPASSRLS dedicated role + Argon2id RFC 9106 + HS256 JWT + lazy upgrade dual-verify PBKDF2→Argon2id, trait `IdentityProvider` shape congelada) e **GAR-391a — `garraia-auth` skeleton** (crate skeleton + migration 008 criando `garraia_login NOLOGIN BYPASSRLS` com 4 GRANTs exatos do ADR 0005 + `LoginPool` newtype com `current_user` validation + `static_assertions::assert_not_impl_all!(LoginPool: Clone)` + smoke tests integration). Migration 009 (prereq estrutural de GAR-391b) adicionou `user_identities.hash_upgraded_at` para o lazy upgrade transacional. Próximo: **GAR-391b** (`verify_credential` real impl + audit + JWT issuance + endpoint `/v1/auth/login` sob feature flag).

Filtre por [`good-first-issue`](https://github.com/michelbr84/GarraRUST/issues?q=label%3Agood-first-issue+is%3Aopen) no GitHub para encontrar um lugar para começar.

## Licença

MIT
