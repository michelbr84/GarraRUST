- **`agent.sandbox.mode = all` passa a dizer o que NAO cobre (#1225 S2, parte segura).**
  O docstring de `SandboxMode::All` prometia "toda tool shell-listada roda no sandbox", e
  so o `BashTool` consulta a policy: `run_tests` (cargo/flutter/npm/python), `git_diff`,
  `code_review` e `repo_search` spawnam no host sem olhar `mode` nenhum. Quem ligava `all`
  esperando "nada roda no host" estava enganado sobre quatro tools, e fora de um paragrafo
  do threat model nada no produto dizia isso. Agora a lista e uma constante publica
  (`garraia_agents::sandbox::HOST_ONLY_SPAWNING_TOOLS`, espelhada em
  `garraia_config::sandbox::TOOLS_SO_NO_HOST` porque config nao depende de agents), dita
  em tres lugares que o operador ve: os docstrings de `SandboxMode::All`/`Allowlist`; um
  `warn!` unico por processo (`avisa_cobertura_do_sandbox`) na subida do gateway, do
  `garra chat` e do `garra mcp-server` com a tool `garra_agent` ligada (so nomes de tool,
  nenhum valor de config) — separado da construcao da policy de proposito, porque no MCP
  `sandbox_policy_from` roda a cada chamada de `garra_agent` e o aviso sairia por chamada;
  e um Warning do `garra config check` quando uma dessas quatro aparece em
  `sandboxed_tools`/`elevated`, nomeando a tool, o que o sandbox cobre e o proximo passo.
  So quando listada: `--strict` promove Warning a exit 2, e a secao recomendada
  (`mode = all` + docker) continua saindo com exit 0.
  A constante e presa por um teste que varre `crates/garraia-agents/src/tools/`: toda tool
  com `Command::new` em codigo de producao tem de estar na lista OU chamar
  `sandbox.wrap_command(`, e nada pode estar nas duas. Quando a metade estrutural da S2
  rotear uma delas pelo sandbox, o teste obriga a tira-la da lista — e a documentacao nao
  volta a prometer o contrario do codigo. Um segundo teste, no gateway (a unica crate que
  ve as duas), confere o espelho da config. Rotear as quatro tools pelo sandbox continua
  na #1225.
