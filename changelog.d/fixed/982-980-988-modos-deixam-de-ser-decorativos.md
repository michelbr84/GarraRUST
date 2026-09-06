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
- Um `ExecContext` no lugar de mais dois `Option<&str>`: o metodo ja tinha dez
  parametros e dois `#[allow(clippy::too_many_arguments)]`, e a #986 traria
  mais um. Os wrappers legados passam `ExecContext::default()`, entao os sete
  consumidores que so querem texto ficam intactos.
