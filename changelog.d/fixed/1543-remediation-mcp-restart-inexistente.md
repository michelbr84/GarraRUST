- **Remediation do MCP deixa de mandar rodar um subcomando que nao existe (#1543).**
  O registro de capacidades e o check `tools.capabilities` do `/api/diagnostics`
  diziam `garraia mcp restart <nome>`, mas `garra mcp` so tem `list`, `inspect`,
  `resources` e `prompts` — o operador tentava o comando, tomava erro de clap e
  perdia tempo justamente no momento em que um servidor MCP estava fora do ar.
  Os textos agora apontam para o caminho real, `POST /admin/api/mcp/<nome>/restart`
  (o mesmo do botao Restart na aba MCP Servers do console), e um teste varre o
  fonte do gateway para o comando fantasma nao voltar.
