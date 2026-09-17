- **Reiniciar um servidor MCP pela admin API nao apaga mais a allowlist de tools dele (#1242).**
  `POST /admin/api/mcp/{id}/restart` reconectava passando `vec![]` como `allowed_tools`
  nos dois transportes, e lista vazia significa "permite tudo" — enquanto o `disconnect`
  imediatamente antes ja tinha descartado a unica copia da allowlist, guardada na
  conexao. O efeito era um fail-open silencioso: depois de um hot-reload de rotina, toda
  tool descoberta daquele servidor voltava chamavel pelo LLM, sem nenhum sinal para o
  operador. O boot e o reconnect automatico do monitor de saude sempre fizeram certo; so
  este caminho esquecia. Agora o handler resolve a allowlist ANTES do `disconnect`, por
  uma ordem nomeada cujo invariante e preciso: **o restart nunca alarga o que esta em
  vigor, e aplica a lista declarada quando nada esta em vigor.** Uma lista viva nao
  vazia no `McpManager` (conexao viva ou entrada em `pending`) e o que de fato
  restringe o servidor agora e vence; caso contrario — `None`, ou `Some(vec![])`, que e
  como `is_tool_allowed` escreve "permite tudo" — vale o `allowed_tools` declarado. Um
  `Some(vec![])` quer dizer "ninguem nunca restringiu este nome", nao "o operador
  escolheu nao restringir": o manager nao distingue os dois casos e o config escrito
  distingue, entao tratar a lista viva vazia como resposta autoritativa deixava um
  servidor que subiu antes da allowlist ser declarada sem nenhuma forma de ser apertado
  por um restart. So quando nem o manager nem a declaracao conhecem uma restricao a
  lista fica vazia — caso dos servidores criados por `POST /admin/api/mcp`, que hoje nao
  tem como carregar allowlist nenhuma. O que o handler deliberadamente NAO faz: aplicar
  uma declaracao mais estreita que uma lista viva nao vazia; a regra 1 preserva a lista
  viva, entao o restart segue sem alargar, mas esse aperto pede restart do gateway.
- **A allowlist declarada no `mcp.json` passou a ser honrada pelo restart (#1242).**
  A resolucao consultava so a secao `mcp:` do `config.yml`. O boot nao le isso: le
  `ConfigLoader::merged_mcp_config`, que e `mcp.json` **mais** aquela secao, e a entrada
  do `mcp.json` desserializa em `garraia_config::McpServerConfig`, que tem
  `allowed_tools`. Uma allowlist escrita no `mcp.json` valia no boot e era invisivel
  para o restart, que caia no ramo "nunca restringido" e reconectava o servidor aberto.
  O restart agora resolve contra o mesmo merge que o boot le.
- **Um restart recusado deixou de ser a ultima forma de perder a allowlist (#1242).**
  As duas validacoes de 400 (`stdio` sem `command`, HTTP sem `url`) rodavam DEPOIS do
  `disconnect`, entao a recusa voltava de um servidor ja derrubado cuja allowlist
  resolvida morria no stack frame — e o restart seguinte lia `None` e reconectava
  aberto. Alcancavel em duas chamadas de admin, porque `POST /admin/api/mcp` aceita
  `{"url": ..., "transport": "stdio"}` e sobrescreve a entrada de um servidor vivo.
  `command`/`url` passaram a ser extraidos antes de qualquer teardown: a recusa agora
  nao derruba nada e a conexao viva segue restrita.
- **Dois buracos vizinhos, fechados junto (#1242).** Um restart cujo reconnect falha
  estaciona o servidor em `pending` com a allowlist resolvida, entao o restart seguinte
  nao le mais `None`; e o boot passou a estacionar tambem falha de handshake em
  transporte HTTP, nao so stdio, entao o primeiro restart de um servidor HTTP com
  allowlist nao reconecta mais aberto. Junto, `tool_info()` passou a filtrar pela
  allowlist como `take_tools` e `call_tool` ja faziam, entao o `tool_count` da resposta
  do restart para de contar tool bloqueada e os slash-commands MCP param de registrar
  tool que toda execucao ia recusar.
