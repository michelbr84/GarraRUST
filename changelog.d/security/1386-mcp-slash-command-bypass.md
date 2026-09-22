- **Ferramenta MCP nao vira mais slash command, que era bypass do ToolGate (#1386).**
  O boot registrava um comando `/mcp_<tool>` por ferramenta MCP conectada, e o
  fechamento desse comando chamava `McpManager::call_tool` direto — sem
  `ToolGate`, sem o modo da sessao, sem `ToolApproval` e sem `HardwareGate`. O
  unico controle era o `Role::User` do comando, que todo mundo tem: uma sessao
  criada em modo `search` (read-only) executava ferramenta de mutacao por
  `POST /api/sessions/{id}/messages`, e `GET /api/slash-commands` listava os
  nomes de graca. O caminho legitimo nao muda — as ferramentas MCP continuam
  chegando ao modelo pelo `AgentRuntime`, despachadas atras do `ToolGate`.
