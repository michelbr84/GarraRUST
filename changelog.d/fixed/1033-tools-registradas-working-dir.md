- **Quatro ferramentas do agente existiam e nunca foram registradas; e o
  diretorio da sessao nao chegava ao turno HTTP (#1033, #1035).** `list_dir`,
  `repo_search`, `run_tests` e `code_review` tinham schema e testes verdes, mas o
  unico `new()` delas no repo era dentro dos proprios testes — as whitelists dos
  modos `search`, `debug` e `review` anunciavam `list_dir` e `repo_search` que o
  modelo nunca recebia. Agora entram no bootstrap do gateway (`code_review` com o
  provider e modelo default do boot). E `exec_context_for` passou a levar
  `SessionState::working_dir` para o `ExecContext`: antes todo turno via
  `/api/sessions/{id}/messages` saia sem diretorio, e o `resolve_tool_path`
  recusava qualquer caminho relativo — o Garra no celular dizia que nao conseguia
  olhar os proprios arquivos, e nao conseguia mesmo. O modo `debug` ganha
  `run_tests` e o `review` ganha `code_review` na whitelist.
