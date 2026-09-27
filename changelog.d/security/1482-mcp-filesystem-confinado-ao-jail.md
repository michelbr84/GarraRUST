- **Chamadas MCP de filesystem confinadas ao jail da sessao (#1482, nucleo da
  #1383).** Em `standard` a raiz do servidor `filesystem` autoprovisionado e
  `<data_dir>/workspace`, o pai de todo diretorio de sessao (#1449); com a
  leitura MCP liberada no piso `search` (#1384), um contato admitido listava
  o pai e lia o que o agente escreveu para outra pessoa. Agora `McpTool`
  passa `path`/`paths`/`source`/`destination` das operacoes de filesystem pelo
  MESMO `FileJail` das file tools nativas, com o `working_dir` da sessao;
  caminho relativo resolve contra o diretorio da sessao; fora do jail a recusa
  e a frase unica das tools nativas; `list_allowed_directories` responde as
  raizes efetivas da sessao sem chamar o servidor. O gateway entrega o jail ao
  `McpManager` no boot (guarda de fonte); a CLI local segue sem jail.
