- **Servidor MCP stdio deixa de herdar o ambiente inteiro do gateway (completa
  o hardening da #1075; sandbox do processo segue na #1225).** Ate agora o
  `McpManager::connect` montava o `Command` do processo filho sem
  `env_clear()`. Consequencia direta: todo servidor MCP recebia, no proprio
  ambiente, `GARRAIA_JWT_SECRET`, `GARRAIA_REFRESH_HMAC_SECRET`, as chaves de
  provider (`ANTHROPIC_API_KEY`, `OPENROUTER_API_KEY`, ...),
  `GarraIA_VAULT_PASSPHRASE`, `DATABASE_URL` e qualquer outra variavel que o
  `dotenvy` tivesse carregado do `.env`. O mapa `env` da config era aplicado
  POR CIMA dessa heranca, nao no lugar dela, entao declarar `env` nunca
  restringiu nada.
  O que torna isso grave no MCP e a origem do binario: um servidor MCP e
  tipicamente um pacote de terceiro resolvido na hora (`npx -y algum-server`),
  e ler `std::env` no proprio `main()` nao exige tool call, prompt injection
  nem rede do agente — basta ser spawnado. O #1075 ja tinha fechado esse mesmo
  buraco para as tools de shell (`bash_tool`, `run_tests`, `git_diff`,
  `code_review`, `repo_search`) com a `R3_ENV_ALLOWLIST`; o caminho MCP ficou
  de fora, e uma varredura por `env_clear` no repositorio nunca encontrava o
  `manager.rs`.
  Agora o ambiente do filho e construido do zero, nesta ordem: allowlist
  minima do ambiente do gateway (`PATH`, `HOME`, locale, `TMPDIR`, `TZ`,
  bundles de CA, mais o complemento de plataforma — `SystemRoot`/`COMSPEC`/
  `APPDATA` no Windows, `PREFIX`/`LD_PRELOAD` no Termux) e, por cima, o mapa
  `env` daquele servidor, que e onde o operador coloca de proposito o
  `GITHUB_TOKEN` e afins. A allowlist MCP e um superset deliberado da R3 e vive
  ao lado dela em `garraia_common::safety_gate`: um servidor MCP e um programa
  completo (`npx`/`uvx`/`python`) e precisa de cache, temp e locale so para
  subir, enquanto a R3 serve um comando de shell efemero — fundir as duas faria
  qualquer afrouxamento aqui afrouxar junto o `bash_tool`. Variaveis de proxy
  (`HTTP_PROXY`/`HTTPS_PROXY`) ficaram de fora de proposito, porque a URL pode
  embutir usuario e senha.
  A valvula de escape e `inherit_env: true` por servidor no `config.yml` /
  `mcp.json` (default `false`): devolve ao filho o ambiente completo, emite
  `warn!` nomeando o servidor no primeiro connect (reconnects automaticos caem
  para `debug!`, para nao afogar o log de um servidor em loop de restart) e
  nunca registra nome nem valor de variavel. Ela existe para destravar um
  servidor legado enquanto o operador migra a variavel para o mapa `env`, e
  nao esta disponivel na admin API — servidor criado OU reiniciado por ali
  conecta sempre isolado, mesmo que o `config.yml` declare `inherit_env: true`
  para aquele nome. A politica viaja junto dos parametros de conexao, entao um
  reconnect automatico do proprio gateway nao a troca em silencio.
  Nota para quem for escrever `env` no `config.yml`: referencias
  `vault:<chave>` sao resolvidas apenas no caminho `mcp.json` + admin API
  (`McpPersistenceService::load_registry`). No boot do `config.yml`
  (`ConfigLoader::merged_mcp_config`) o valor e copiado como esta, entao um
  `vault:...` escrito ali chega ao filho como a string literal.
  A lacuna era de teste tanto quanto de codigo: o fixture de MCP agora expoe,
  atras da flag `--expose-env`, uma tool que relata o ambiente que o filho de
  fato recebeu, e os testes plantam uma variavel no processo de teste e afirmam
  que ela NAO chega ao filho por padrao, que chega com `inherit_env: true`, e
  que o mapa `env` explicito chega e vence a heranca.
