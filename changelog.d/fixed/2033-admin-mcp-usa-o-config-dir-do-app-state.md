- **Os handlers admin do MCP gravam e leem no config dir do `AppState`
  (#2033).** O `POST /admin/api/mcp` e o `DELETE /admin/api/mcp/{id}`
  persistiam pelo `mcp.json` e pelo cofre do config dir padrao, e o
  `POST /admin/api/mcp/{id}/restart` lia a allowlist declarada no `mcp.json`
  padrao. Um estado montado sobre outro diretorio
  (`AppState::with_config_dir`) carregava o registry de la, mas:
  - gravava o registry inteiro e os segredos no diretorio padrao;
  - o delete apagava as credenciais do servidor do cofre padrao;
  - o restart reconectava o servidor com a allowlist do outro arquivo.

  Agora os tres usam o `mcp.json` e o cofre do config dir do estado, os
  mesmos de onde o registry foi carregado (`AppState::config_dir` e
  `AppState::mcp_persistence`). Producao nao muda: o `AppState::new` segue no
  config dir padrao. Os testes desses handlers montam o estado sobre um
  diretorio proprio e deixam de apontar o `GARRAIA_CONFIG_DIR` pelo ambiente.
