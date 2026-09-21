- **O MCP `filesystem` autoprovisionado deixa de nascer com `$HOME` como raiz
  (ADR 0024, #1329).** `McpPersistenceService::provision_filesystem_if_missing`
  nao le mais `HOME`/`USERPROFILE` nem cai em `.`: as raizes chegam de fora,
  de `bootstrap::raizes_do_mcp_filesystem` — `agent.file_roots` ou
  `<data_dir>/workspace` em `standard`, `execution.pod_root` ou o mesmo
  workspace em `isolated-pod` — e TODAS entram como argumentos finais de
  `@modelcontextprotocol/server-filesystem`, com cada diretorio criado antes
  da escrita. Era o contorno do jail: as file tools nativas ficavam presas em
  `agent.file_roots` enquanto `filesystem__read_file` lia a home inteira.
  Fail-closed: lista vazia ou raiz que nao da para criar e "nao provisiona"
  com aviso, nunca fallback. O opt-out `GARRAIA_DISABLE_MCP_AUTOPROVISION` e
  a regra "arquivo existente nunca e tocado" seguem iguais — instalacoes
  anteriores continuam com o `mcp.json` que ja tem, e e o `/api/diagnostics`
  que agora aponta a raiz legada fora do jail.
