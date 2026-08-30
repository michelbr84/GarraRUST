# Plan 0360 — `garra agents setup`: provisionamento e roteamento multi-agente

**Status:** ✅ Entregue 2026-08-30 (horário da Flórida)
**Branch:** `claude/garra-agents-setup-command-6u1phw`
**Repo irmão:** `michelbr84/AgentDeck`, mesma branch
**ADR:** [`0014-anthropic-messages-shim.md`](../docs/adr/0014-anthropic-messages-shim.md)

## Problema

O usuário mantém quatro agentes na mesma máquina — GarraIA, Hermes, OpenClaw e
Claude Code. Cada um com seu instalador, seu arquivo de config e seu jeito de
escolher provedor/modelo/chave. Trocar de modelo significava editar quatro
arquivos em quatro formatos, e não havia lugar nenhum onde os quatro
aparecessem juntos ou pudessem se falar.

## Restrição estruturante

O motor é o **AgentDeck**, não o GarraRUST. Ele já tinha adapters completos
para exatamente esses quatro agentes (detect/install/upgrade/health/backup/
rollback), Rooms com cinco modos de roteamento, SQLite e um deck web.
Reimplementar isso em Rust seria manter duas cópias da mesma lógica em duas
linguagens. O `garra agents` é casca fina que delega.

O que faltava no AgentDeck era **qualquer noção de provedor, modelo ou chave**:
`AgentInstance.modelAlias` era gravado e exibido, mas nunca chegava ao
`execute()`.

## Entregue

### Neste repositório

1. **`garra config set-routing`** (`config_cmd.rs`) — escreve primário + backup
   numa tacada. Função pura `apply_set_routing` + 8 testes. Chave por **stdin**,
   nunca por flag: argv é legível por qualquer processo local. A chave do `llm:`
   é o tipo do provedor, não o papel, então re-executar com os lados trocados
   atualiza as mesmas entradas em vez de acumular órfãos. Trocar de modelo sem
   passar chave preserva a credencial existente.

2. **`POST /v1/messages`** (`anthropic_api.rs`, 700 LOC + 13 testes) — shim
   Anthropic-compatible para o Claude Code apontar `ANTHROPIC_BASE_URL` ao
   gateway. É proxy, não agente: vai direto ao `LlmProvider`, sem o loop de
   tools do runtime. Detalhes e consequências na ADR 0014.
   `complete_with_fallback` / `stream_complete_with_fallback` viraram `pub` para
   o shim herdar o failover de graça.

3. **`garra agents {setup,status,link,rollback,web}`** (`agents.rs`) — descobre
   o binário `agentdeck`, oferece instalar via npm, repassa as flags verbatim.
   Early-intercept tier: precisa funcionar antes de existir config. `DeckProbe`
   com `RealProbe`/`FakeProbe`, no padrão do `EnvProbe` do wizard.

### No AgentDeck

| PR | Conteúdo |
|---|---|
| P0 | Verdade dos adapters: installers inexistentes, versão hardcoded, path de config errado, manifesto de backup incompleto, comparação de versão por string. Mais o estreitamento do `redactSecrets` e `apps/**` no include do vitest |
| P1 | Tipos de provider, `SecretStore` (um arquivo por provedor, 0600), migration v3, catálogo com validação de modelo ao vivo |
| P4 | `LlmConfigurable` como interface separada e opcional + implementação nos 4 adapters |
| P5 | `agentdeck agents setup/status/rollback` |
| P7 | `agentdeck mcp-server` + `agents link` + guardrails de interop |
| P9 | Painel de controle e construtor de grupos, em Garra Glass |

## Defeitos encontrados durante a implementação

| # | Defeito | Correção |
|---|---|---|
| 1 | `redactSecrets` apagava `AgentInstallationState.authentication` e `tokensUsed` — `GET /api/v1/agents` já devolvia `"[REDACTED_SECRET]"` em produção | Padrões ancorados + teste de regressão nos dois sentidos |
| 2 | Adapter do Hermes clonava um repositório que não existe | Instalador oficial da NousResearch |
| 3 | Adapter do GarraIA reportava `getLatestVersion()` fixo em `0.2.1` contra a 0.3.4 real → tudo sempre "desatualizado" | API de releases do GitHub, com fallback honesto offline |
| 4 | `rollback()` do Claude Code era no-op silencioso: `settings.json` não estava no manifesto de backup | Manifesto corrigido + invariante testada `configFiles ⊆ manifesto` |
| 5 | Ids de modelo têm ponto (`glm-5.3-flash`), e o setter de path pontilhado os quebrava em chaves aninhadas | Atribuição direta no mapa + teste de regressão |
| 6 | Mapa de presença de credenciais chamado `credentials` era apagado pelo próprio `redactSecrets` | Renomeado para `credentialPresence` |
| 7 | `usage: 0` reportado pelo provedor era repassado, e zero desliga o auto-compact do cliente | Zero tratado como "não sei" → estimativa |
| 8 | `rollback` reportava sucesso restaurando zero arquivos | Passa a remover o que criamos quando nada existia antes, e a dizer quantos arquivos tocou |

## Verificação executada

- **Rust:** `cargo fmt --check` limpo, `clippy -p garraia-gateway` sem
  warnings, **1216 testes** verdes (853 gateway + 214 CLI + 149 agents).
- **`/v1/messages` contra gateway real** com `dev-echo-provider`: resposta
  não-stream com `usage` estimado, sequência SSE completa e **nomeada**
  (`message_start` → `content_block_start` → `content_block_delta` →
  `content_block_stop` → `message_delta` → `message_stop`), `count_tokens`, e
  `/v1/models` intacto — sem colisão de rota, sem pânico no boot.
- **`set-routing` ponta a ponta**: dry-run, escrita real com chave por stdin,
  `config.yml` em 0600, backup igual ao primário rejeitado (exit 2), config
  ilegível → exit 65 sem tocar no arquivo.
- **AgentDeck:** build, typecheck, lint e **173 testes** verdes. `agents setup`
  aplicado num HOME de teste e conferido nos arquivos dos agentes; segunda
  execução reporta "já atual" sem escrever; rollback restaura e remove
  corretamente; a chave existe só em `~/.agentdeck/secrets/` e não aparece em
  nenhuma resposta da API.

## Fora de escopo / pendente do mantenedor

- **Smoke real com `claude -p`** contra o gateway: exige uma instalação do
  Claude Code apontada ao gateway e um provedor com chave. É o teste que fecha
  a ADR 0014 — o `stop_reason: tool_use` só se prova de verdade quando o Claude
  Code executa uma tool através do shim.
- **`qwen3.5:2b` como backup do Claude Code**: 2,3B parâmetros não dirigem um
  agente de tool-use. O wizard já avisa quando o modelo não anuncia `tools`,
  mas vale considerar um binding de modelo separado para o Claude Code.
- **Prompt caching** não é repassado pelo shim (ADR 0014).
- Refactor completo do `App.tsx` (1517 linhas) — as páginas novas saíram como
  componentes separados, mas o arquivo original segue monolítico.
