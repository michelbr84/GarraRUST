- **Entrada HTTP de `mcp.json` deixa de ser descartada pelo loader e perde a
  `allowed_tools` (#1274, fail-open de allowlist em restart do admin).** O
  `McpServerConfig` de config exigia `command: String`, entao toda entrada
  URL-only de `mcp.json` — o formato legitimo de um servidor MCP remoto, e
  tambem o formato que o proprio gateway grava no arquivo ao criar um servidor
  HTTP pelo admin — falhava a desserializacao e era descartada em silencio
  pelo `load_mcp_json` (skip por entrada, que existe para nao perder o resto
  do arquivo por causa de uma so). A consequencia passava despercebida ate o
  proximo restart: o nome do servidor nunca chegava ao merge declarado que o
  `resolve_allowlist` do restart consulta, a resolucao caia em
  `AllowlistOrigin::NeverRestricted` e o servidor voltava a conectar com
  TODAS as tools expostas — a `allowed_tools` que o operador escreveu no
  arquivo era descartada no exato momento em que importava. Duas varreduras
  de seguranca anteriores (o comentario do braço `NeverRestricted` nomeava so
  os servidores criados via API como moradores desse braço) nao pegaram o
  buraco porque o descarte acontece no loader, dois crates antes do restart.
  Agora `command` e `#[serde(default)]` (entrada HTTP sem command e valida),
  a recusa de entrada sem `command` nem `url` e explicita no loader (mesma
  regra que o `garra config check` reporta, com `warn!` no lugar do silencio),
  o braço stdio do boot recusa command vazio antes do spawn, e o
  `garra mcp list` imprime a URL no lugar de command vazio.
  Cobertura: `load_mcp_json_keeps_http_entry_without_command` (regressao da
  forma exata da issue), `load_mcp_json_skips_stdio_entry_without_command`
  (a recusa explicita), teste de composicao no gateway
  (`restart_resolves_the_allowlist_of_an_http_entry_declared_in_mcp_json`:
  arquivo real em disco -> `merged_mcp_config` -> `resolve_allowlist` ->
  `AllowlistOrigin::Config` com a allowlist) e guard de boot. Prova por
  mutacao: remover o `#[serde(default)]` recoloca os testes na cor vermelha.
