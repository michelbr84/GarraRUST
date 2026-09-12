- **rmcp 2.2.0 → 3.3.0 (portado do repo privado, tracker GarraIA/GarraIA#163).**
  O SDK MCP salta um major com a API pós-2.2 preservada: no cliente, a
  ponte de tools usa `Peer::call_tool_once` — o enum MRTR-aware
  `CallToolResponse` (`Complete`/`InputRequired`/`Task`) — com os braços
  `InputRequired` (SEP-2322) e `Task` (SEP-2663) **fail-closed**: o bridge
  não dirige rodadas interativas nem polling de `tasks/get`, e o LLM vê o
  motivo. No servidor (`garra mcp-server`), o trait `ServerHandler` passa a
  devolver `CallToolResponse` (só `Complete`, via `From<CallToolResult>`) e
  `ListToolsResult` usa o construtor `with_all_items` com os campos novos
  SEP-2322/2549 (`result_type`/`ttl_ms`/`cache_scope`). O supervisor
  `get_info_advertises_only_tools_capability` acompanha: `tasks` saiu do
  `ServerCapabilities` e agora viaja como extensão em `extensions`.