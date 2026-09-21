# Referência da CLI

O binário `garraia` (alias `garra`) concentra toda a operação. Fonte: [`crates/garraia-cli/src/main.rs`](https://github.com/michelbr84/GarraRUST/blob/main/crates/garraia-cli/src/main.rs) (enum `Commands` e os enums de subcomando). Onde há exit codes, eles seguem `sysexits`: 0 ok · 2 falha de validação/step · 65 arquivo que não parseia · 69 recurso indisponível · 70 erro interno · 124 timeout.

## Instalação e processo

| Comando | O que faz |
|---|---|
| `garra init` | Wizard interativo: provedor LLM, chave no cofre criptografado; em bind exposto gera `gateway.api_key` sem imprimir (#1241) |
| `garra start` | Inicia o gateway (`--daemon`, `--with-voice`, `--host`, `--port`; `HOST`/`PORT` de ambiente valem, flag explícita vence) |
| `garra stop` / `restart` / `status` | Controle do daemon |
| `garra about` | Banner completo do GarraIA e o que cada comando faz (sem cor fora de TTY) |
| `garra logs` | Lê o `garraia.log` canônico direto do disco, sem precisar do gateway (`-f` segue, `-n` últimas linhas, `--path` só imprime o caminho) — #943 |
| `garra doctor` | Diagnóstico da instalação: plataforma, diretórios, config, providers, daemon (`--json`, `--strict` trata warnings como erro) |
| `garra update` / `rollback` | Auto-atualização com verificação SHA-256 (`--yes`; `--check-binaries` só varre a PATH atrás de outros binários `garraia`/`garra`, sem download) / volta à versão anterior |
| `garra verify` | Pipeline local: fmt, clippy, test, flutter analyze, gitleaks (`--json`, `--skip <step>`; exit 0/2) |

## Conversar com o agente

| Comando | O que faz |
|---|---|
| `garra chat` | REPL interativo local-first (`--provider`, `--model`, `--url`, `--timeout-secs`, `--yes`). `--persist` grava a conversa em `sessions.db`; `--resume [id\|latest]` retoma uma sessão (sem valor, a mais recente — #1300). Comandos do REPL: `/status`, `/context` (alias `/contexto`), `/tools`, `/tool <n>`, `/history`, `/resume`, `/logs`, `/models`, `/model <nome>` (transacional, #1298) |
| `garra ask "<pergunta>"` | Pergunta única, LLM-only, sem tools e sem ANSI; `--json` emite o envelope `garra.ask.v1`; `--timeout-secs` estourado vira exit 124 — [docs/cli-ask.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/cli-ask.md) |
| `garra mcp-server` | Expõe `garra_ask` como servidor MCP stdio (Claude Desktop/Code); stdout é só JSON-RPC — [docs/cli-mcp-server.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/cli-mcp-server.md) |
| `garra max-power [--goal …] [--mode new\|existing\|auto]` | Modo agent-advanced nativo (ADR 0011): menu de pipeline ou roteamento por objetivo |

## Canais

| Comando | O que faz |
|---|---|
| `garra channel list` · `channel status <nome>` | Canais de chat configurados |
| `garra whatsapp` | Menu de duas opções: WhatsApp pessoal por QR ou Business pela Cloud API (#1238, ADR 0023). Sem TTY imprime as opções e sai 0 |
| `garra whatsapp link` | Vincula o WhatsApp pessoal lendo um QR code no terminal — precisa de Node.js 20+ (exit 69 sem Node ou QR não lido) |
| `garra whatsapp cloud` | Configura um WhatsApp Business pela Cloud API oficial da Meta |
| `garra whatsapp status` | Diz se há WhatsApp pessoal vinculado, onde a sessão está e se ela abre |
| `garra whatsapp logout` | Desvincula e apaga a sessão deste aparelho |
| `garra whatsapp restore` | Traz de volta a sessão arquivada (`session.enc.prev`) por um re-vínculo que não terminou; nunca passa por cima de sessão em uso |

Guia completo: [docs/whatsapp.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/whatsapp.md).

## Memória, configuração e admin

| Comando | O que faz |
|---|---|
| `garra memory stats` · `list` · `search` | Inspeciona a memória semântica (contagens + integridade do índice vetorial; busca pelo mesmo recall do agente) — #950 |
| `garra memory add` · `reindex` · `backup` | Semeia uma entrada, re-embeda as sem vetor, snapshot consistente via `VACUUM INTO` com retenção |
| `garra memory pin` · `ttl` · `delete` · `compact` | Fixa contra retenção, define/limpa expiração, apaga por id, apaga mais antigas que N dias |
| `garra config check [--json] [--strict]` | Carrega a configuração efetiva e reporta precedência + findings; exit 0 / 2 / 65. É **opt-in** (também rodado por `garra doctor`), não gate de boot (#1247) |
| `garra config set-model` | Aponta o GarraIA para um modelo sem prompt, escrevendo uma entrada `llm:` e `agent.default_provider` (instalações headless, `ollama launch garraia`) |
| `garra config set-routing` | Provider primário **e** backup numa escrita só; a chave vem por `--api-key-stdin`, nunca por flag |
| `garra admin recovery start --username X` | Gera um código de recuperação de senha do painel admin, de uso único, gravado num arquivo `0600` no host (#1122) |
| `garra admin recovery complete` | Troca a senha consumindo o código — uma única vez |

## Extensões e integrações

| Comando | O que faz |
|---|---|
| `garra plugin list/install/remove/watch` | Plugins WASM (feature `plugins`); `watch` faz hot-reload do diretório |
| `garra skill list/install/remove` | Skills do agente |
| `garra mcp list/inspect/resources/prompts <nome>` | Servidores MCP configurados e o que cada um expõe |
| `garra agents setup` · `status` · `link` · `rollback` · `web` | Provisiona GarraIA, Hermes, OpenClaw e Claude Code com um mesmo provedor+modelo via AgentDeck; flags após o subcomando passam direto |
| `garra desktop [--status] [--no-launch]` | Localiza e lança o GarraIA Desktop instalado (instalador da plataforma → PATH → diretório da CLI); exit 0 / 69 não instalado / 70 não abriu — #1181 |

## Dados e utilitários

| Comando | O que faz |
|---|---|
| `garra migrate openclaw` | Importa dados do OpenClaw |
| `garra migrate workspace` | SQLite (single-user) → Postgres multi-tenant (Group Workspace) |
| `garra glob test` | Testa padrões glob/ignore (`--mode bash`, `--debug-regex`, `--json`) — [semântica](https://github.com/michelbr84/GarraRUST/blob/main/docs/src/glob-semantics.md) |

Para flags completas de qualquer comando: `garra <comando> --help`.
