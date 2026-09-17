- **Uma escrita de admin nao apagava mais o que os outros servidores declaravam em `mcp.json`.**
  O gateway mantinha dois tipos com o mesmo nome para o mesmo arquivo: o `McpServerConfig` do
  `garraia-config` (que o boot le) carregava `allowed_tools`, `inherit_env` e `enabled`, mas o
  `McpServerConfig` do registry — o que `save_from_registry` serializa de volta para o arquivo a
  cada POST/DELETE em `/admin/api/mcp` — nao carregava nada disso. Uma unica criacao ou remocao
  de servidor pela admin API reescrevia o `mcp.json` sem a allowlist GAR-190 de TODOS os
  servidores do arquivo (a sonda do corpo da issue provava: apos um `add_server` +
  `save_from_registry`, `allowed_tools` tinha saido do disco). O mesmo defeito derrubava o
  `inherit_env` (#1075), a chave de boot `enabled` — um servidor desligado religava no proximo
  boot — e o tuning snake_case (`memory_limit_mb`, `max_restarts`, `restart_delay_secs`), que o
  registry nem lia. O tipo do registry agora carrega os tres campos, com aliases de leitura que
  fazem os dois lados concordarem em um schema so: `allowed_tools` e `inherit_env` sao gravados
  na grafia snake_case canonicamente lida pelo boot, e o tuning snake_case escrito a mao agora
  sobrevive ao round-trip. `POST /admin/api/mcp` passa a aceitar `allowed_tools`, e o `GET`
  expoe `allowed_tools` e `inherit_env`. O timeout continua sem alias camelCase no loader de
  proposito: ler o `timeoutSecs: 30` default do writer sobrescreveria
  `timeouts.mcp.default_secs` em servidores em que o operador nunca escolheu um timeout. O
  restart da admin API mantem o isolamento forcado de ambiente (#1075) — o campo agora viaja no
  tipo, mas honrar a declaracao no restart e mudanca de stance propria.
