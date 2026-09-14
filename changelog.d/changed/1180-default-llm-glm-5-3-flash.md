- **`z-ai/glm-5.3-flash` via OpenRouter passa a ser o LLM padrao oficial (#1180).**
  A pergunta "qual LLM o Garra usa quando o usuario nao escolhe nada?" tinha
  cinco respostas diferentes conforme a porta de entrada: `garra chat` e o
  wizard diziam `openrouter/auto`, o `garra mcp-server` dizia `openrouter/free`,
  o gateway caia em `openai/gpt-4o` para um bloco `openrouter` sem `model:`
  explicito, e o Garra Desktop nascia em `lmstudio`. Agora ha uma constante
  unica em `garraia_config::defaults` que as cinco superficies leem, travada
  por teste. O local (Ollama, `qwen3.8:latest`) continua inteiro, mas como
  **segunda** opcao: entra em `agent.fallback_providers` e so vira primario
  se o usuario pedir. O wizard passa a destacar "Cloud-first" em vez de
  "Local-first" mesmo em maquina com GPU.
- **`openrouter/auto` e `openrouter/free` deixam de ser padrao em qualquer
  superficie (#1180).** Os dois continuam validos como escolha explicita
  (`--model openrouter/auto`, `model: "openrouter/free"` numa chamada MCP);
  o que muda e que nenhum caminho de codigo os seleciona sozinho. No MCP o
  `openrouter/free` era um guardrail de custo deliberado — um host pode chamar
  `garra_ask` em loop — e essa intencao foi preservada e documentada: o
  flash-tier e barato o bastante para ser o padrao nao-assistido, e
  `GARRAIA_MCP_MODEL_ALLOWLIST` continua sendo o teto duro do operador.
- **ATENCAO, quebra para quem fixou o `GARRAIA_MCP_MODEL_ALLOWLIST` (#1180).**
  Quem seguiu `docs/hermes-integration.md` e exportou
  `GARRAIA_MCP_MODEL_ALLOWLIST=openrouter/free` precisa atualizar o allowlist
  para incluir `z-ai/glm-5.3-flash` (ou remover a variavel). O allowlist e
  avaliado DEPOIS de o default novo ja ter sido aplicado, entao toda chamada
  MCP sem `model` explicito passa a falhar com `invalid_params` ate o
  allowlist ser ajustado. Exemplo:
  `export GARRAIA_MCP_MODEL_ALLOWLIST=z-ai/glm-5.3-flash,openrouter/free`.
- **O gateway passa a respeitar `agent.default_provider` no boot (#1180).**
  Com mais de um provider em `llm:`, o default efetivo era o primeiro que a
  iteracao do `HashMap` devolvia — ou seja, sorteio a cada boot. Agora o
  provider nomeado em `agent.default_provider` e aplicado explicitamente; um
  nome que nao corresponda a nenhum provider registrado vira aviso no log e
  mantem o que havia, sem derrubar o boot.
- **O autodetect do `garra chat` tenta a nuvem antes do Ollama (#1180).** Sem
  `agent.default_provider` no config, a cadeia legada tentava o Ollama local
  primeiro mesmo com `OPENROUTER_API_KEY` exportada. Agora os provedores de
  nuvem com credencial vem primeiro (Anthropic > OpenAI > OpenRouter) e o
  Ollama e o que sobra quando nenhuma credencial de nuvem existe. Quem contava
  com o Ollama vencendo uma chave de nuvem exportada deve fixar
  `agent.default_provider: ollama` no `config.yml`.
- **O wizard nao preseleciona mais o download local no caminho cloud-first
  (#1180).** Escolhendo "Cloud-first", o confirm de instalar o Ollama nasce em
  "nao" e o seletor de modelo local cai na linha de pular o download — os
  ~18 GB viraram um sim explicito. No caminho "Local-first" nada mudou.
