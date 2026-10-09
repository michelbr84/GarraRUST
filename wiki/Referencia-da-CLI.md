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
| `garraia runs list` | Lê o ledger de runs de agente (`agent_runs` do `sessions.db`) direto do disco, sem o gateway: `--status running\|done\|error\|cancelled\|interrupted`, `--limit` (padrão 50), `--json` com instantes UTC ISO 8601 — #1227 |
| `garra doctor` | Diagnóstico da instalação: plataforma, diretórios, config, providers, daemon (`--json`, `--strict` trata warnings como erro) |
| `garra update` / `rollback` | Auto-atualização com verificação SHA-256 (`--yes`; `--check-binaries` só varre a PATH atrás de outros binários `garraia`/`garra`, sem download) / volta à versão anterior |
| `garra uninstall` | Desinstala a CLI da máquina: binário, alias `garra`, wrappers do Termux e backups `.old`/`.new` (`--yes` pula o prompt, exigível num pipe; `--purge` inclui config, dados e o legado `~/.garraia`; `--all-binaries` varre a PATH; exit 64 sem terminal e sem `--yes`, 78 se o daemon é de uma unit systemd) |
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
| `garra whatsapp cloud` | Configura um WhatsApp Business pela Cloud API oficial da Meta (exit 69 sem terminal) |
| `garra whatsapp status` | Diz se há WhatsApp pessoal vinculado, onde a sessão está e se ela abre; desde a v0.4.4 imprime também o perfil de execução (`standard` \| `isolated-pod`), o piso do dono e a contagem de `owners` (ADR 0024) |
| `garra whatsapp logout` | Desvincula e apaga a sessão deste aparelho |
| `garra whatsapp restore` | Traz de volta a sessão arquivada (`session.enc.prev`) por um re-vínculo que não terminou; nunca passa por cima de sessão em uso |
| `garraia whatsapp allow <número> [--owner] [--yes]` | Autoriza um número (com `+` e código do país) ou um LID `<id>@lid` a falar com o GarraIA pelo WhatsApp pessoal, sem precisar de terminal; vale sem reiniciar quando o gateway subiu com o canal ligado e com o `config.yml` no disco (#1345). Exit 64 para `--owner` fora de `isolated-pod` ou num pipe sem `--yes`, 65 para número inválido |
| `garraia whatsapp link --allow <número> [--owner]` | O `link` com a pergunta pós-QR já respondida; continua exigindo terminal (exit 69 num pipe). Desde a v0.4.6 o wizard termina imprimindo a política de acesso em vigor (#1429) |

### Access Policy v2 — quem entra e até onde vai (v0.4.6, ADR 0025)

O nível é um **teto que compõe com o modo da sessão: só tira, nunca põe.** Identidade aparece só pelos quatro últimos dígitos em toda tela e em todo `--json`; `--reveal` mostra os valores da config, localmente. Todo comando abaixo deixa rastro no audit (#1414). Exit codes: 0 ok · 65 número ou combinação inválida · 70 config ilegível · 73 gravou mas o audit falhou.

| Comando | O que faz |
|---|---|
| `garraia whatsapp access [--reveal]` | Sem subcomando, imprime a política **efetiva** pelo mesmo motor do turno: admissão (`restricted` \| `open`), default do desconhecido, grupos, e cada principal com piso, nível e o que pode de fato (#1396) |
| `garraia whatsapp users [--json]` | Só lê: quem pode falar, com o papel (`allow` ou `owners`) e os quatro últimos dígitos (#1393) |
| `garraia whatsapp level <número> chat\|read\|full [--dry-run]` | Teto de acesso de um número; `--dry-run` mostra o impacto sem gravar. Nível em dono não vale — use `unowner` (#1398) |
| `garraia whatsapp write <número> on\|off` | Mexe **só** em escrita de arquivo (nativa e MCP): não liga `bash`, não desliga sandbox, jail nem confirmação (#1397) |
| `garraia whatsapp block <número>` · `unblock` | Bloqueio vence `open`, `allow` e pareamento; vale na mensagem seguinte, sem restart |
| `garraia whatsapp remove <número> [--yes]` | Espelho do `allow`: tira de `allow` e de `owners` sem desligar o canal. Remover um DONO exige confirmação (exit 64 num pipe sem `--yes`); quem não estava na lista sai 0, porque revogar é idempotente (#1394) |
| `garraia whatsapp owner <número> [--yes]` | Promove a DONO — só com `execution.profile = isolated-pod` (exit 64 fora dele). Quem ainda não estava autorizado passa a estar, e a tela avisa (#1395) |
| `garraia whatsapp unowner <número>` | Tira o papel de dono **sem** tirar o acesso: a entrada passa para `allow` na mesma escrita. Funciona em qualquer perfil (#1395) |
| `garraia doctor whatsapp [--json]` | Percorre o caminho inteiro: vínculo, chave da sessão, gateway, política de acesso, perfil de execução, workspace, visibilidade do MCP e provedor. Exit 0 ok, 69 indisponível — com a linha que diz o quê (#1419) |

Grupos ligados e desligados valem na mensagem seguinte, sem restart, e cada grupo tem política própria (#1412). O `access.groups.enabled` declarado vence o `reply_in_groups` legado (#1501). No Web Console, a mesma política pela mesma API está na página **WhatsApp Access**, com preview do efeito antes de confirmar (#1402–#1405), e o botão **Test WhatsApp** roda o mesmo motor do `doctor whatsapp` (#1420).

Guia completo: [docs/whatsapp.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/whatsapp.md).

## Memória, configuração e admin

| Comando | O que faz |
|---|---|
| `garra memory stats` · `list` · `search` | Inspeciona a memória semântica (contagens + integridade do índice vetorial; busca pelo mesmo recall do agente) — #950 |
| `garra memory add` · `reindex` · `backup` | Semeia uma entrada, re-embeda as sem vetor, snapshot consistente via `VACUUM INTO` com retenção |
| `garra memory pin` · `ttl` · `delete` · `compact` | Fixa contra retenção, define/limpa expiração, apaga por id, apaga mais antigas que N dias |
| `garraia config check [--json] [--strict]` | Carrega a configuração efetiva e reporta precedência + findings; exit 0 / 2 / 65. Desde a v0.4.5 também roda em todo `start`/`restart`/`start -d` (#1247), e só o TLS pela metade recusa o boot (exit 78; escotilha `GARRAIA_ALLOW_INVALID_CONFIG=1`). Desde a v0.4.4 o sumário traz `execution profile  : <perfil> (source: default \| file \| env)`; valor inválido em `execution.profile`/`GARRAIA_EXECUTION_PROFILE` é `Error` (exit 2); `execution.pod_root` fora de `isolated-pod` ou relativo, e `channels.whatsapp_linked.owners` fora de `isolated-pod` (só a contagem, nunca as identidades) são `Warning` (ADR 0024) |
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
