- **`DELETE /admin/api/mcp/{id}` agora revoga de verdade: derruba a conexao
  viva, limpa o `pending` e solta as tools do `AgentRuntime` (issue #1262,
  fail-open de revogacao).** Ate agora o handler removia o servidor do
  registry, apagava as credenciais do cofre e reescrevia o `mcp.json`, mas
  nunca chamava o `McpManager`. A conexao viva mora no manager, nao no
  registry, entao o servidor deletado continuava conectado e entregando tools
  ao agente pelo resto da vida do processo. Pior: depois do #1242 (PR #1255),
  um restart que falha estaciona a entrada em `pending` com o `env` ja
  resolvido do cofre; o monitor de saude varre `pending` e **ressuscita o
  servidor deletado** com os segredos que o operador acabou de revogar. Ou
  seja, "deletar e revogar" nao revogava, e o caminho era silencioso — a UI
  mostrava o servidor como removido. Um segundo DELETE ainda devolvia 404 (o
  servidor sumiu do registry), deixando o operador sem nenhum handle pela
  admin API para desligar o processo que continuava rodando.
  O conserto adiciona `McpManager::forget(name)`, que remove a entrada de
  `pending` **e** de `restart_states` (para o monitor nao ter de onde
  ressuscitar nem contador orfao), e faz o handler chamar
  `manager.disconnect` + `manager.forget` **antes** de `remove_server`, na
  ordem que deixa uma falha de persistencia no par seguro (manager sem,
  registry com) e nunca no perigoso (registry sem, manager com). As tools do
  servidor sao dropadas do inventario do runtime via
  `replace_mcp_tools(server, [])` — `sync_mcp_tools` nao visita servidores
  ausentes do manager, entao sem a remocao explicita os `McpTool` mortos
  ficariam listados ate o proximo restart do gateway. Testes de regressao
  exercitam o handler real e o `health_tick` real, com mutacao provada:
  remover `disconnect` vermelheia o teste da conexao viva; remover `forget`
  vermelheia o teste da ressurreicao.
