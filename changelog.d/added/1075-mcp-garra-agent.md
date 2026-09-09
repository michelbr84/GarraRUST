- **Nova tool MCP `garra_agent`: agente completo com ferramentas, opt-in do
  operador.** O servidor MCP (`garra mcp-server`) continua expondo o
  `garra_ask` LLM-only como antes; quando o operador inicia o processo com
  `GARRAIA_MCP_ENABLE_TOOLS=1`, `tools/list` passa a anunciar tambem o
  `garra_agent`, que roda um turno de agente completo em sessao nova por
  chamada — bash (full-auto, apenas o DENY_LIST do safety_gate), file_read,
  file_write, web_fetch, git_diff e web_search (com chave Brave). Sem a env,
  nem o anuncio nem o dispatch existem: o servidor rejeita `garra_agent` como
  tool desconhecida e o comportamento fica identico ao de hoje.

  A resposta vem no envelope `garra.agent.v1`, com o mesmo formato do
  `garra.ask.v1` mais `session_id` e `tool_calls` (nome, duracao, sucesso,
  resumo — ja redigidos na origem), tambem em falha e timeout, para o host MCP
  ver o que o agente executou. `GARRAIA_MCP_MAX_TIMEOUT_SECS` passa a limitar
  tambem a nova tool (teto proprio de 1800s; default 300s). O handler mora em
  `mcp_agent.rs`, modulo novo que os testes de auditoria de `mcp_server.rs`
  nao escaneiam por desenho — o arquivo de dispatch continua sem registrar
  ferramenta, sem spawnar processo e sem escrever no stdout.

  O system prompt default do agente obriga a relatar na resposta final
  qualquer ferramenta que falhar ou for bloqueada — nunca reportar sucesso
  sem saida real e nunca contornar silenciosamente um bloqueio de seguranca
  (observado em repro real: `file_write` recusado e contornado via redirect
  bash, com a falha omitida da prosa; ver issue #1075).
