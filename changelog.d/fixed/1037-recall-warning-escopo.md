- **O aviso de recall vazio nomeia a causa certa (#1037).** Quando o indice
  vetorial achava vizinhos e nenhum sobrevivia ao reescopo, o log culpava
  "troca de modelo de embeddings sem reindexacao" em todos os casos — inclusive
  no mais comum, sessao nova com memoria de outras sessoes, onde reindexar nao
  muda nada. Agora o store conta os candidatos por modelo antes de avisar:
  `WARN` de troca de modelo so quando nenhum candidato tem o modelo ativo
  (com os modelos encontrados); `INFO` "fora do escopo pedido" quando a
  memoria existe com o modelo certo mas e de outra sessao, com os filtros
  aplicados (tenant, sessao, continuidade); e `WARN` de vetor orfao quando o
  indice aponta para ids sem linha. A secao Isolamento de `docs/src/memory.md`
  passa a documentar os quatro filtros do recall — inclusive o de `session_id`,
  que era o omitido — e o que `memory.shared_continuity` faz de fato (#1038).
