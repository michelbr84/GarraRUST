- **Sessao com diretorio de trabalho morto pausa a ferramenta na primeira
  recusa, e nao na terceira (#1909).** Quando o `working_dir` da sessao nao
  existe mais (projeto movido, worktree removido), `file_read`, `list_dir` e
  `repo_search` recusam igual com qualquer caminho, mas o circuit breaker
  tratava a recusa como erro generico e so abria depois de tres repeticoes
  identicas no turno: duas voltas de LLM gastas com uma resposta ja conhecida.
  A recusa passa a ser falha deterministica (`session_dir_unavailable`), como
  `no_roots` e `no_repository`: a ferramenta fica em pausa ate o fim do turno
  e o modelo recebe o motivo, sem caminho. Como `file_write` e `bash` podem
  criar o diretorio no meio do turno, o sucesso de qualquer um dos dois fecha
  essa pausa (e so essa) nas outras ferramentas. A frase so classifica quando
  e a saida inteira da ferramenta; conteudo que a cite no meio segue generico.
  Nenhuma raiz, jail, modo ou teto foi alterado.
