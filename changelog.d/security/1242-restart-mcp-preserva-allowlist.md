- **Reiniciar um servidor MCP pela admin API nao apaga mais a allowlist de tools dele (#1242).**
  `POST /admin/api/mcp/{id}/restart` reconectava passando `vec![]` como `allowed_tools`
  nos dois transportes, e lista vazia significa "permite tudo" — enquanto o `disconnect`
  imediatamente antes ja tinha descartado a unica copia da allowlist, guardada na
  conexao. O efeito era um fail-open silencioso: depois de um hot-reload de rotina, toda
  tool descoberta daquele servidor voltava chamavel pelo LLM, sem nenhum sinal para o
  operador. O boot e o reconnect automatico do monitor de saude sempre fizeram certo; so
  este caminho esquecia. Agora o handler captura a allowlist em vigor ANTES do
  `disconnect`, via `McpManager::allowed_tools_for`, que le a conexao viva com fallback
  em `pending` e distingue "servidor desconhecido" (`None`) de "conhecido e sem
  allowlist" (`Some(vec![])`) — servidor que nunca teve allowlist continua sem nenhuma.
  Junto, `tool_info()` passou a filtrar pela allowlist como `take_tools` e `call_tool` ja
  faziam, entao o `tool_count` da resposta do restart para de contar tool bloqueada e os
  slash-commands MCP param de registrar tool que toda execucao ia recusar.
