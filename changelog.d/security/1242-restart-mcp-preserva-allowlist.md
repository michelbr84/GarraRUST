- **Reiniciar um servidor MCP pela admin API nao apaga mais a allowlist de tools dele (#1242).**
  `POST /admin/api/mcp/{id}/restart` reconectava passando `vec![]` como `allowed_tools`
  nos dois transportes, e lista vazia significa "permite tudo" — enquanto o `disconnect`
  imediatamente antes ja tinha descartado a unica copia da allowlist, guardada na
  conexao. O efeito era um fail-open silencioso: depois de um hot-reload de rotina, toda
  tool descoberta daquele servidor voltava chamavel pelo LLM, sem nenhum sinal para o
  operador. O boot e o reconnect automatico do monitor de saude sempre fizeram certo; so
  este caminho esquecia. Agora o handler resolve a allowlist ANTES do `disconnect` por
  uma ordem nomeada que so estreita: o que o `McpManager` tem em maos (conexao viva ou
  entrada em `pending`) vence, inclusive quando a resposta e `Some(vec![])` — "conhecido
  e sem allowlist" e resposta, nao ausencia; se o manager nao conhece o servidor, a
  secao `mcp:` da config em vigor e consultada, que e de onde o boot le `allowed_tools`;
  e so quando nenhum dos dois conhece o nome a lista fica vazia, caso dos servidores
  criados por `POST /admin/api/mcp`, que hoje nao tem como carregar allowlist nenhuma.
  Dois buracos fechados junto: um restart cujo reconnect falha agora estaciona o servidor
  em `pending` com a allowlist resolvida, entao o restart seguinte nao le mais `None` e
  reabre o servidor; e o boot passou a estacionar tambem falha de handshake em transporte
  HTTP, nao so stdio, entao o primeiro restart de um servidor HTTP com allowlist nao
  reconecta mais aberto. Junto, `tool_info()` passou a filtrar pela allowlist como
  `take_tools` e `call_tool` ja faziam, entao o `tool_count` da resposta do restart para
  de contar tool bloqueada e os slash-commands MCP param de registrar tool que toda
  execucao ia recusar.
