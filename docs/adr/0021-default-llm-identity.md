# 21. `z-ai/glm-5.3-flash` via OpenRouter como LLM padrão; local como segunda opção

- **Status:** Accepted (2026-09-13, decisão do dono registrada na issue #1180)
- **Deciders:** @michelbr84 (decisão final) + Claude (levantamento e execução,
  sessão autônoma 2026-09-13)
- **Date:** 2026-09-13
- **Tags:** llm, provider, configuracao, custo, onboarding
- **Supersedes:** none
- **Superseded by:** none
- **Links:**
  - Issue: [#1180](https://github.com/michelbr84/GarraRUST/issues/1180) —
    "Fixar `z-ai/glm-5.3-flash` (OpenRouter) como LLM padrão oficial;
    local/Ollama como segunda opção"
  - Issues relacionadas (experiência de primeira execução):
    [#1177](https://github.com/michelbr84/GarraRUST/issues/1177),
    [#1178](https://github.com/michelbr84/GarraRUST/issues/1178)
  - Precedência de resolução de modelo: `docs/configuration.md`
    §"Provider / model resolution precedence" (GAR-576)
  - Guardrail de custo do MCP: `docs/cli-mcp-server.md` §"Cost policy"
    (GAR-587)

## Context and Problem Statement

O projeto não tinha **uma** identidade de LLM padrão. A pergunta "qual modelo
o Garra usa quando o usuário não escolhe nada?" tinha quatro respostas
diferentes, e qual delas valia dependia de por onde o usuário entrou:

| Superfície | Padrão antes deste ADR |
| --- | --- |
| `garra chat` / `garra ask` | `openrouter/auto` (`chat.rs::hardcoded_default_model`) |
| Wizard `garraia init` | `openrouter/auto`, e "Local-first" pré-selecionado quando havia GPU |
| MCP (`garra mcp-server`, `garra mcp-agent`) | `openrouter/free` |
| Garra Desktop (`config.default.yml`) | `default_provider: "lmstudio"` |

Cada valor tinha sido escolhido por um motivo local e defensável — o
`openrouter/free` do MCP, em particular, era um guardrail de custo explícito
(GAR-587): um host MCP pode chamar `garra_ask` em loop, sem ninguém olhando,
e o padrão precisava ser um modelo incapaz de gerar fatura. O problema não era
nenhuma das escolhas isoladas; era não existir uma decisão de projeto acima
delas, o que fez a resposta derivar quatro vezes e deixou a instalação nova
imprevisível.

Enquanto isso, `z-ai/glm-5.3-flash` já era o modelo que o projeto de fato usava
e documentava como exemplo canônico (`garraia-gateway/src/anthropic_api.rs`,
`garraia-cli/src/config_cmd.rs`, `garraia-cli/src/main.rs`,
`docs/cli-mcp-server.md`, `.claude/agents/implementer.md`) — sem nunca ter sido
promovido a padrão.

## Decision Drivers

- **Previsibilidade da instalação nova.** O que o usuário recebe deve ser uma
  decisão do projeto, não um efeito colateral da porta de entrada.
- **Custo não-assistido.** O padrão roda em loop dentro de agentes; precisa ser
  barato o bastante para isso não ser um risco financeiro.
- **Qualidade suficiente para trabalho real.** `openrouter/free` era barato e
  ruim; era um padrão que o usuário tinha que trocar antes de fazer qualquer
  coisa séria.
- **Local continua sendo cidadão de primeira classe** — mas como fallback com
  circuit breaker, não como primário imposto por detecção de GPU.
- **Nada deixa de ser possível.** Demover um modelo de "padrão" não pode
  torná-lo inalcançável.

## Considered Options

1. **OpenRouter + `z-ai/glm-5.3-flash` como padrão único, Ollama como
   fallback** (escolhida).
2. **Manter `openrouter/auto` como padrão em todas as superfícies.** Unifica,
   mas transforma o padrão num roteador de preço variável — exatamente o que
   o guardrail do MCP existia para evitar.
3. **Manter `openrouter/free` como padrão em todas as superfícies.** Unifica e
   é gratuito, mas entrega uma primeira experiência ruim o bastante para que
   o usuário conclua que o produto é ruim.
4. **Local-first universal (Ollama `qwen3.8:latest`).** Zero custo e privado,
   mas exige ~18 GB de download e uma GPU antes do primeiro "olá" — e em
   máquina sem GPU não há padrão nenhum.

## Decision Outcome

Escolhida a opção 1, conforme decisão do dono na issue #1180:

1. `z-ai/glm-5.3-flash` via OpenRouter é o LLM padrão oficial de toda
   instalação nova.
2. Local (Ollama, `qwen3.8:latest`) é sempre a **segunda** opção: entra em
   `agent.fallback_providers`, e só vira primário quando o usuário pede.
3. `openrouter/auto` e `openrouter/free` deixam de ser padrão em qualquer
   superfície. Continuam disponíveis como escolha explícita; nenhum caminho de
   código os seleciona sozinho.

A implementação central é uma constante única em
`crates/garraia-config/src/defaults.rs`, lida por `chat.rs`, `wizard/mod.rs` e
`mcp_server.rs` (e, através dele, `mcp_agent.rs`) no lado da CLI, e por
`bootstrap/mod.rs` no lado do gateway, com teste travando a tabela nos dois
crates. É isso que impede a próxima divergência: não existe mais literal
repetido para alguém "melhorar" sozinho.

A constante nasceu em `crates/garraia-cli/src/defaults.rs`, que é
`pub(crate)` dentro do binário `garraia`. Isso deixava **o gateway de fora**:
um bloco `openrouter` sem `model:` explícito caía num `openai/gpt-4o`
hardcoded em `bootstrap/mod.rs` — a quinta fonte de verdade, invisível para o
teste de trava da CLI. Por isso a constante desceu para `garraia-config`, o
crate que CLI e gateway já compartilham; `crates/garraia-cli/src/defaults.rs`
virou um reexport e guarda o único lock que só ele pode guardar (o
`config.default.yml` do Desktop, um recurso YAML que não lê constante Rust).

**O guardrail de custo do MCP foi preservado de propósito, não por acaso.** O
doc-comment de `MODEL_DEFAULT` diz explicitamente que aquela linha é um
guardrail de gasto e que o flash-tier foi escolhido para manter a propriedade;
`GARRAIA_MCP_MODEL_ALLOWLIST` continua sendo o teto duro do operador.

### Consequences

- **Boa:** uma instalação limpa (Linux, Windows, Desktop, Termux) termina em
  `agent.default_provider: openrouter` + `z-ai/glm-5.3-flash` sem o usuário
  escolher nada, e isso é verificável por teste.
- **Boa:** o wizard deixa de empurrar ~18 GB de download como caminho
  principal só porque detectou uma GPU. Concretamente: no caminho
  "Cloud-first" o confirm de instalar o Ollama nasce em `false` e o seletor
  de modelo local nasce na linha "pular o download" — o download passa a ser
  um "sim" digitado, nunca a consequência de apertar Enter. No caminho
  "Local-first" os defaults continuam onde estavam (instalar sim, primeiro
  modelo da lista), porque ali é o que o usuário acabou de pedir. A regra
  mora em `LocalStackRole` (`wizard/mod.rs`), testada sem prompt.
- **Boa:** o autodetect do `garra chat` (a heurística que roda quando não há
  `agent.default_provider`) tenta a nuvem **antes** do Ollama. Antes ele
  tentava o Ollama local primeiro, incondicionalmente, o que contradizia a
  decisão 2 em toda máquina com `OPENROUTER_API_KEY` exportada e um
  `ollama serve` ligado. A ordem vive em `autodetect_order` (`chat.rs`),
  função pura e testada; o Ollama continua na lista, sempre por último, que é
  o papel de fallback local que a decisão 2 quer preservar.
- **Boa:** o boot do gateway aplica `agent.default_provider` explicitamente.
  Sem isso, o default efetivo era o primeiro provider registrado ao iterar
  `config.llm` — um `HashMap`, ou seja, ordem não-determinística: com dois
  providers configurados (o `config.default.yml` do Desktop tem exatamente
  dois), "o Desktop nasce em OpenRouter" era sorteio a cada boot. Um
  `default_provider` que não corresponda a nenhum provider registrado vira
  aviso no log e mantém o que havia — fail-safe, não fail-fast, porque
  derrubar o boot por causa de uma chave de API ausente seria pior.
- **Ruim / aceita:** o padrão agora exige uma chave de API. Sem
  `OPENROUTER_API_KEY` (ou chave no `config.yml`), a cadeia de autodetect cai
  para o fallback local — que é exatamente o papel reservado a ele, mas é uma
  mudança real para quem instalava sem chave e caía em `lmstudio`/Ollama por
  padrão. O perfil endurecido (`config.hardened.example.yml`) segue local de
  propósito, agora com a divergência anotada no próprio arquivo.
- **Ruim / aceita:** o padrão passa a ter custo diferente de zero. É por isso
  que ele é flash-tier e não `auto`, e por isso o allowlist do operador
  continua existindo.
- **Risco fechado (validação externa):** o slug `z-ai/glm-5.3-flash` **não
  pôde ser validado contra `https://openrouter.ai/api/v1/models`** de dentro
  do ambiente de execução deste PR — o egress para `openrouter.ai` é bloqueado
  por política (403 no CONNECT). A validação foi feita **fora** desse
  ambiente, durante a revisão do #1180: o modelo existe na OpenRouter, é
  **pago** (~US$0,15/M tokens de entrada, ~US$0,50/M de saída) e **não tem
  tier gratuito**. Isso fecha o risco de "slug errado deixa toda instalação
  nova sem LLM primário" e reforça a consequência de custo acima: é
  exatamente por o padrão ser pago que `GARRAIA_MCP_MODEL_ALLOWLIST` continua
  existindo como teto duro do operador.

## Compliance / Follow-ups

- `crates/garraia-config/src/defaults.rs` é a fonte de verdade;
  `crates/garraia-cli/src/defaults.rs` reexporta. Os testes que travam a
  tabela: `defaults::tests::default_llm_identity_is_locked` e
  `no_default_is_a_retired_literal` (garraia-config),
  `hardcoded_default_model_table_is_locked` e
  `desktop_default_config_matches_the_shared_constants` (garraia),
  `openrouter_sem_model_explicito_cai_no_default_do_projeto`
  (garraia-gateway). Mudar o padrão exige mexer em todos, nos docs e neste
  ADR.
- Operadores que fixaram `GARRAIA_MCP_MODEL_ALLOWLIST=openrouter/free`
  precisam incluir `z-ai/glm-5.3-flash` no allowlist (ou removê-lo): o
  allowlist é avaliado **depois** de `resolve_overrides` aplicar o default, e
  sem o ajuste toda chamada MCP sem `model` explícito passa a responder
  `invalid_params`. Está no fragmento de changelog do #1180.
- Instaladores (`install.sh` / `install.ps1`) **não** fixam modelo — nenhum
  dos dois precisou mudar, e a paridade da regra 16 do `CLAUDE.md` segue
  intacta.
- O slug foi validado fora deste ambiente durante a revisão do #1180 (ver
  "Risco fechado" acima) — follow-up encerrado.
