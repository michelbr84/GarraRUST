- **O modo escolhido passa a valer de verdade (#982, #988).** Ate aqui `/mode
  code` respondia "modo definido", gravava no banco, e a proxima mensagem rodava
  igual — o modo era salvo e nunca lido no caminho de execucao, e a
  `ToolPolicy` de cada modo era declarada e nunca verificada. Modos anunciados
  como somente-leitura (`search`, `review`, `architect`) nao bloqueavam
  `file_write` nem `bash`: apenas pediam no prompt. Agora o modo chega ao
  runtime pelos dez pontos que atendem usuario, e a politica e aplicada.
- **Aplicada em dois niveis, de proposito.** O filtro na montagem tira a
  ferramenta da lista que o modelo ve — e UX, o modelo nao gasta turno pedindo
  o que nao pode. O guard antes de cada `tool.execute` e a garantia: o criterio
  de aceite e "nenhuma ferramenta proibida e executada, **mesmo que solicitada
  pelo LLM**", e o modelo pode pedir um nome que nunca esteve na lista.
- **"Sem modo escolhido" nao e "modo Ask", e essa distincao evita a regressao
  mais provavel do lote.** O default do enum e `Ask`, e `ask` nega
  `file_write`. Se sessao sem modo resolvesse para `Ask`, ligar a politica
  quebraria escrita por padrao em **todo** canal, CLI incluso — que nunca seta
  modo. O criterio de aceite pede que o comportamento padrao nao regrida, e o
  comportamento padrao de hoje e nao ter politica.
- **Ferramenta MCP nao e barrada por whitelist, e isso e um limite conhecido.**
  Tool de servidor MCP se chama `{servidor}__{tool}`, e os whitelists de cinco
  dos nove modos listam so nomes nativos — aplicar ao pe da letra derrubaria
  toda integracao MCP nesses modos, em silencio. Ela passa pelo whitelist e
  continua sujeita ao `denied`. **A consequencia:** um modo somente-leitura nao
  restringe ferramenta MCP. Se o operador conectou um servidor que escreve
  arquivo, o modo `search` nao o impede. Um whitelist que entenda servidor MCP
  precisa ser desenhado, e nao cabia aqui.
- **O `working_dir` chega as ferramentas de arquivo (#980).** Caminho relativo
  passa a resolver contra o diretorio do projeto em vez do cwd do processo.
  **Isto nao e um sandbox**, ao contrario do que a issue afirma: o
  `resolve_tool_path` rejeita `..` e junta relativo com o diretorio, mas nao
  canonicaliza nem confina — caminho absoluto passa igual, antes e depois. E o
  `bash_tool` ignora o campo por completo. O que a issue chama de validacao ja
  testada (`is_path_allowed`, `ProjectToolContext`) e codigo morto, alcancado
  so pelos proprios testes.
- **Deduzir nao e consentir.** O auto-router (GAR-227) classifica a mensagem e
  grava o modo na sessao. Enquanto o modo era decoracao de prompt isso era
  inofensivo; com a politica valendo no executor, ler dali aplicaria restricao
  a quem nunca escolheu nada — bastava a heuristica achar que a pergunta
  parecia busca para `file_write` sumir. O store passa a registrar **quem**
  escolheu (`agent_mode_source`): o `/mode` e o `GET /api/mode/current`
  continuam mostrando o modo deduzido, e so o escolhido liga a politica. Sessao
  gravada antes do marcador conta como nao-escolhida — e o comportamento que
  ela ja tinha, e o primeiro `/mode` regulariza.
- **O `X-Agent-Mode` nunca chegava ao banco.** O gateway gravava o modo antes
  de `hydrate_session_history` criar a linha da sessao, e `set_agent_mode` e um
  `UPDATE ... WHERE id = ?`: zero linhas casadas, `Ok(())` devolvido, `let _ =`
  no chamador. O header dizia `search` e o banco ficava vazio — bug anterior a
  este lote, achado rodando o binario, invisivel a qualquer teste que so
  chamasse o setter numa sessao existente. Agora a gravacao acontece depois da
  hidratacao, o setter devolve erro quando nao encontra a sessao, e o `/mode` do
  Telegram avisa em vez de responder "modo definido" com o banco intacto.
- **Nome de modo invalido para de virar escolha.** O `/mode` e o
  `PUT /api/mode` ja recusavam nome desconhecido com mensagem; o header
  `X-Agent-Mode` e o prefixo `mode:` gravavam a string crua. Depois da politica
  isso era uma escolha registrada que nao resolve para perfil nenhum: portao
  aberto, `/mode` exibindo um modo inexistente, ninguem sabendo. Agora valida,
  loga `warn!` e segue como "nao pediu modo" — o request nao cai por causa de
  um typo em header.
- **Slack, Discord, WhatsApp, iMessage e o terceiro braco do `POST /api/chat`
  entram junto.** Eles chamavam os wrappers `_with_context`, que passam
  `ExecContext::default()`: `/mode search` respondia "modo definido" e a
  restricao valia so no Telegram. Assimetria silenciosa e pior que ausencia,
  porque o usuario acredita na restricao. Um teste varre o fonte para o proximo
  canal — que sera escrito copiando um destes — nao reintroduzir o buraco.
  `a2a.rs` fica de fora com o motivo escrito: a sessao `a2a:{task_id}` nasce e
  morre na requisicao, entao nao ha escolha para ler.
- Um `ExecContext` no lugar de mais dois `Option<&str>`: o metodo ja tinha dez
  parametros e dois `#[allow(clippy::too_many_arguments)]`, e a #986 traria
  mais um.
