- O `sha` do corpo de `POST /api/learning/skills/{name}/rollback` passa a ser
  validado (hexadecimal, 7 a 40 caracteres) antes de virar argumento do `git`.
  Ate aqui o valor ia cru para `git revert` e para o `git diff` do versioning,
  num modulo que e auth-free por politica: nenhum shell esta envolvido, mas o
  `git` le um valor comecando com `-` como **opcao**, e `--output=<caminho>` no
  caminho do diff e escrita de arquivo arbitraria. O mesmo valor tambem
  derrubava a task com `end byte index 8 is not a char boundary` quando um
  caractere multi-byte caia na fronteira que o `short_sha` fatia. Recusa
  fail-closed: nenhum processo e criado com valor invalido, e a resposta 400 nao
  ecoa o que foi recusado (#1086).
- `git revert` e `git add` passam a separar os operandos com `--`, para que um
  sha ou um caminho nunca possam ser lidos como flag (#1086).
