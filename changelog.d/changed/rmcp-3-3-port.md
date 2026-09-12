- Fecha a migracao do MCP SDK para 3.x (tracker interno #163): `rmcp`
  2.2 -> 3.3 no workspace (e `process-wrap` 9.1 -> 10.0, transitivo do
  client stdio). O `ServerHandler::call_tool` do servidor `garra mcp-server`
  agora devolve `CallToolResponse` (`Complete` para resultado de tool comum),
  `tools/list` usa o construtor `ListToolsResult::with_all_items` (o struct
  ganhou `result_type`/`ttl_ms`/`cache_scope`), e o teste de capabilities
  passou a sondar a extension `io.modelcontextprotocol/tasks` (SEP-2663) via
  `supports_tasks()` — o campo `ServerCapabilities.tasks` nao existe mais.
  Sem mudanca de comportamento para os hosts MCP: `garra_ask`/`garra_agent`
  continuam anunciando e respondendo igual.
