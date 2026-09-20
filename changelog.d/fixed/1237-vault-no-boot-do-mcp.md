- **MCP: referencias `vault:` no `env` de servidores do config.yml/mcp.json
  nao eram resolvidas no boot** (#1237). A resolucao de `vault:` vivia so no
  registry (`McpPersistenceService::load_registry`, GAR-291), e o caminho de
  boot (`ConfigLoader::merged_mcp_config` -> `build_mcp_tools`) copiava o
  mapa `env` como estava: o filho recebia a string literal como valor da
  variavel — o operador acreditava que o segredo estava cifrado, e o
  servidor falhava de forma enganosa. Agora o boot resolve com o MESMO
  helper do registry (`resolver_env_com_vault`), e a politica e **fail-
  closed**: referencia que nao resolve (cofre ausente, `GARRAIA_VAULT_`
  `PASSPHRASE` sem set, chave inexistente) impede o servidor de subir, com
  aviso nomeando servidor e chave, nunca o valor — sem pending de proposito,
  porque o pending guardaria o env literal e um retry bem-sucedido entregaria
  a string crua ao filho. O `garra config check` passa a avisar quando ha
  `vault:` no `env` de um servidor MCP com o cofre indisponivel (config
  mergeada: config.yml + mcp.json). Docs voltam a recomendar `vault:` no
  `env` (`mcp-capacidades`, `config.basic.yml`, threat-model E). Teste de
  integracao com o fixture `--expose-env` prova o valor resolvido chegando
  ao filho e o servidor bloqueado sem valor no texto; a mutacao desligando
  a resolucao derruba o teste.
