- **`z-ai/glm-5.3-flash` via OpenRouter passa a ser o LLM padrao oficial (#1180).**
  A pergunta "qual LLM o Garra usa quando o usuario nao escolhe nada?" tinha
  quatro respostas diferentes conforme a porta de entrada: `garra chat` e o
  wizard diziam `openrouter/auto`, o `garra mcp-server` dizia `openrouter/free`
  e o Garra Desktop nascia em `lmstudio`. Agora ha uma constante unica em
  `crates/garraia-cli/src/defaults.rs` que as quatro superficies leem, travada
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
