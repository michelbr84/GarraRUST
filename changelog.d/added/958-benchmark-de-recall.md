- **`benches/recall_quality/` — benchmark de qualidade do recall em portugues
  (#958).** O benchmark que existia mede desempenho (tamanho de binario, RSS,
  cold start); este mede se a memoria devolve a lembranca **certa**:
  recall@k, precision@k e MRR sobre 40 consultas com ground truth, em 13
  grupos que separam tipos de falha (parafrase, sinonimo, consulta de uma
  palavra, consulta em espanhol contra corpus em portugues).
- **`ruido@k` para consulta que nao deve casar com nada.** E o que a issue pede
  sem nomear: ela relata que "quem e Michel" devolveu "oi" no top-K. Um
  benchmark que so mede acerto daria nota cheia a um sistema que devolve tudo
  para tudo, entao o grupo `ruido-puro` tem ground truth vazio e e pontuado
  numa escala separada, onde menor e melhor. As consultas desse grupo perguntam
  por assuntos que o corpus nao tem, e o `run.sh` recusa o dataset se alguma
  delas usar palavra que aparece no corpus — sem isso a sonda mediria acerto
  como se fosse ruido. As saudacoes seguem entre os **documentos**, que e onde
  importam: como distratoras das consultas de verdade.
- **`garra memory add` novo.** O `garra memory` sabia inspecionar (`list`,
  `search`, `stats`) e podar (`delete`, `compact`, `ttl`), mas nao **semear**:
  a unica forma de por algo na memoria era conversar com o agente, o que exige
  um provider de LLM. Sem isso nao havia como medir recall de forma
  reproduzivel. O embedding e gerado na hora — uma entrada sem vetor nao
  aparece na busca semantica, e um comando de semear que deixa a entrada
  invisivel ate um segundo comando e uma armadilha. Quando nao ha provider, ele
  **diz** e aponta o `reindex`.
- **Nao e gate de CI, e nao deve virar um.** A execucao depende de um provider
  de embeddings, e um numero que varia com a maquina e com o modelo instalado
  nao pode reprovar o PR de ninguem.
- **Primeira medida, commitada como artefato:** MRR 0,108 na **busca textual**
  (sem provider configurado). O que o numero diz nao e "a memoria e ruim" — e
  que o fallback textual e quase inutil para pergunta em linguagem natural.
  `recall@1` igual a `recall@10` e a assinatura: a busca e
  `LIKE '%frase inteira%'`, entao ou a frase casa ou nao casa. As quatro
  consultas que acertaram, das 37 com resposta esperada, sao exatamente aquelas
  cuja string aparece **literal** no documento — nao as mais curtas.
