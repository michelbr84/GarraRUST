- **Mensagem de ferramenta nao entrega o caminho absoluto do host em turno
  restrito (#1909).** Num turno restrito — a superficie somente-leitura dos
  canais remotos, cujo remetente nao e necessariamente o operador — o
  `garra_status` ja retinha de proposito o `working_dir` da sessao (#1347).
  As mensagens de `list_dir`, `file_read` e `file_write` traziam esse mesmo
  caminho de volta pela porta dos fundos: o cabecalho da listagem e o
  `describe()` dos erros mostravam o caminho absoluto resolvido sempre que o
  alvo caia FORA do `working_dir` da sessao (ou quando a sessao nao tinha
  `working_dir`, caso em que o relativo resolve contra o CWD do processo do
  gateway). Agora o portao e um so no crate
  (`tool_context::pode_revelar_caminho_do_host`, o mesmo que o `repo_search`
  consulta desde a #1932/#1380): alvo dentro do `working_dir` continua saindo
  relativo em qualquer turno; fora dele, o absoluto so sai no turno que o
  runtime abriu como aberto (o `garra chat` local, onde ele e acionavel), e no
  restrito sai `<outside the session's directory>`, com o argumento do modelo
  preservado em `pedido:` e o `hint` de resolucao intacto. Fail-closed fora de
  um turno despachado pelo runtime. Nenhuma raiz, jail, modo ou mensagem de
  recusa mudou; os logs do operador seguem com o caminho inteiro.
