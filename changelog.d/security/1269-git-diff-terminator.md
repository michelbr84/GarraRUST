- **O `file_path` do modelo na `git_diff` deixa de cair em posicao de flag —
  a injecao de prompt nao desfaz mais o `--no-ext-diff` (correcao da #1269).**
  A tool empurrava `file_path` e `from_commit`/`to_commit` da tool call como
  argumentos posicionais, sem terminador. O PR #1075 ja mitiga com
  `--no-ext-diff` na frente do comando, mas no git, quando a mesma flag
  aparece mais de uma vez, **a ultima vence**: um `file_path` igual a
  `--ext-diff` reabria a execucao de comando externo via `diff.external` de
  um `.git/config` plantado — execucao arbitraria alcancavel por prompt, na
  mesma classe do #1266 corrigido no PR #1268. Agora os argumentos sao
  montados por uma funcao pura que poe o `file_path` sempre **depois** do `--`
  (pathspec, nunca opcao), e o filho git sobe com a entrada padrao nula, como
  os filhos da `repo_search`.
- **`from_commit`/`to_commit` da `git_diff` com prefixo `-` sao recusados
  fail-closed (#1269, segundo achado).** O token de range `{from}..{to}` nao
  pode ir depois do `--` sem perder a semantica de revisoes, entao ele ganhou
  validacao propria: revisao que comeca com `-` poe o token inteiro em
  posicao de opcao — `--output=/tmp/../alvo` escreve a saida do diff em
  caminho escolhido pelo modelo, `-O<arquivo>` le ordem de arquivo plantado.
  Revisao legitima nao comeca com `-`, entao a recusa e controlada e cita a
  revisao recusada.
- **O `git_diff` volta a produzir diff real: o contexto de linhas agora vai
  colado (`-U{context}`).** O git recusa a forma `-U 3` separada — o `3`
  sobrava como argumento posicional e o comando morria com `bad revision
  '3'` (verificado no git 2.43), logo toda resposta de diff devolvia o erro
  em vez do diff. A quebra tambem mascarava a correcao de seguranca: com o
  git morrendo antes de qualquer efeito observavel, nenhum teste de injecao
  conseguia falhar por efeito. Testes de regressao exercitam a tool pelo
  caminho do runtime real, com repositorio plantado, controle positivo de
  diff funcional e prova por efeito (marcador que o driver externo ou o
  `--output` escreveriam).
