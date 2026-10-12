- **Teste de contrato do `bash` pela ponte MCP HTTP (#1626, item 3).** Dois testes
  em `mcp_http_contract.rs` exercitam `garra_agent` de ponta a ponta com um
  provider falso que pede a tool `bash`: o comando roda e a saida volta ao modelo,
  e um comando destrutivo e barrado pela denylist do `BashTool`. Sem mudanca de
  comportamento em producao.
