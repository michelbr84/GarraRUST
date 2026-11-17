- **`repo_search` em sessao com diretorio de trabalho morto pausa na primeira
  recusa, e nao na terceira (#1909).** Quando o `working_dir` da sessao nao
  existe mais (projeto movido, worktree removido), o `repo_search` recusa
  igual com qualquer consulta, mas o circuit breaker tratava a recusa como
  erro generico e so abria depois de tres repeticoes identicas no turno: duas
  voltas de LLM gastas com uma resposta ja conhecida. A recusa passa a ser
  falha deterministica (`session_dir_unavailable`), como `no_roots` e
  `no_repository`: a ferramenta fica em pausa ate o fim do turno e o modelo
  recebe o motivo, sem caminho. Como `file_write` e `bash` podem criar o
  diretorio no meio do turno, o sucesso de qualquer um dos dois fecha essa
  pausa (e so essa). `file_read` e `list_dir` emitem a mesma frase, mas so
  para caminho relativo — um absoluto dentro de uma raiz declarada ainda pode
  dar certo — e por isso seguem na regra de tres repeticoes. Nenhuma raiz,
  jail, modo ou teto foi alterado.
