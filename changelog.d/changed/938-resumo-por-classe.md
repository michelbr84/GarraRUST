- **O resumo de ferramenta passa a dizer o resultado, e nao a primeira linha
  (#938).** Ate aqui `cargo test` aparecia como `Compiling garraia-agents
  v0.3.9 (+104 linha(s))` — a primeira linha nao-vazia, que nao diz nada sobre
  ter passado. Agora aparece `85 passou`, e um `cargo test --workspace` soma os
  binarios (uma linha `test result:` por alvo; "148 passou" espalhado em 54
  linhas nao e resposta). Em falha, `3 falhou, 145 passou`, com a falha na
  frente porque e ela que muda o que a pessoa faz em seguida.
- **Em erro de compilacao o resumo e o proprio erro.** `error[E0382]: borrow of
  moved value` em vez de `Compiling ...`, que era o que a issue pedia como
  "concise relevant excerpt". Mais de um erro mostra o primeiro e diz quantos
  faltam.
- **A classificacao olha a forma da saida, nao o comando.** A mesma contagem
  sai de `cargo test`, `cargo nextest` e de um `make test` que embrulhe
  qualquer um dos dois — e o comando nem chega ate a funcao de resumo. Formato
  sem caso escrito continua no comportamento generico: nao se adivinha, pela
  mesma razao que o resumo do **input** nao adivinha campo (adivinhar foi como
  ele chegou a exibir connection string).
- **Sanear vem antes de classificar.** Saida de terminal costuma vir colorida,
  e um reconhecedor rodando no texto cru procuraria `test result:` numa linha
  que comeca com escape ANSI. O caso comum falharia em silencio, caindo no
  resumo generico sem nada indicando por que.
