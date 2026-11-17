- **Mensagem de ferramenta nao entrega o caminho absoluto do host em turno
  restrito (#1909).** No turno restrito o `garra_status` ja retinha de
  proposito o `working_dir` da sessao (#1347). As mensagens de `list_dir`,
  `file_read` e `file_write` traziam esse mesmo caminho de volta: o cabecalho
  da listagem, o `describe()` dos erros e as linhas de sucesso/readonly do
  `file_write` mostravam o caminho absoluto resolvido sempre que o alvo caia
  FORA do `working_dir` da sessao. Agora essas quatro tools consultam o mesmo
  portao que o `repo_search` (`tool_context::pode_revelar_caminho_do_host`):
  alvo dentro do `working_dir` continua saindo relativo em qualquer turno;
  fora dele, o absoluto so sai no turno aberto, e no restrito sai
  `<outside the session's directory>`, com o argumento do modelo preservado
  em `pedido:`. Fail-closed fora de um turno despachado pelo runtime.
  **Alcance, sem exagero:** "turno restrito" e o modo com allowlist de tools
  (ou teto por principal que tenha) — nao "remetente remoto". Entao os modos
  `plan`, `search`, `edit`, `debug`, `review`, `architect` e `orchestrator`
  retem o caminho ate no `garra chat` local, e um modo sem allowlist (o
  `ask`) continua mostrando o caminho mesmo num canal remoto sem teto.
  `git_diff`/`code_review` (via `RepoDir::descricao`) e o pedido de
  confirmacao do `run_tests` ainda nao passam por este portao. Nenhuma raiz,
  jail, modo ou mensagem de recusa mudou; os logs do operador seguem com o
  caminho inteiro.
