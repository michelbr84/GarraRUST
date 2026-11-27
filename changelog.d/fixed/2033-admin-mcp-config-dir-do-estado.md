- **Os handlers admin do MCP gravam e releem o config dir do proprio
  `AppState`, e nao o padrao (#2033).** O `admin_create_mcp` e o
  `admin_delete_mcp` montavam a persistencia sobre o `mcp.json` e o cofre do
  config dir padrao, e o `admin_restart_mcp` lia a allowlist declarada do
  `mcp.json` padrao. Um estado montado pelo `AppState::with_config_dir` sobre
  outro diretorio gravava o retrato do registry no `mcp.json` real do
  operador, mandava os segredos para o cofre real, apagava `mcp.<nome>.*` do
  cofre real no delete e reiniciava com a allowlist errada. O `AppState`
  passa a guardar o config dir que recebeu (`config_dir()`), e os tres
  handlers usam o mesmo par `mcp.json` + cofre que o boot usa para carregar.
  Producao nao muda: o `AppState::new` segue no config dir padrao.
