- **Listar e ler dizem quando o diretorio da sessao e que morreu (#1909).**
  Com o `working_dir` da sessao apontando para um caminho que nao existe mais
  (projeto movido, worktree removido), `list_dir` respondia com a recusa do
  jail ("acesso negado") e `file_read`, com "nao foi possivel ler metadados" —
  as duas leem como politica de seguranca ou arquivo ausente, quando a causa e
  a sessao. As duas tools passam a devolver a mesma frase que o `repo_search`
  ja devolvia desde a #1932: o diretorio da sessao nao esta acessivel (nao
  existe / nao e um diretorio), nada foi listado ou lido, e a acao e
  selecionar um projeto existente. A checagem so vale para caminho RELATIVO ao
  `working_dir` e roda antes do jail; caminho absoluto, `..`, symlink de fuga e
  a politica de raizes continuam exatamente como estavam, e o caminho da sessao
  nao entra na mensagem (#1626).
