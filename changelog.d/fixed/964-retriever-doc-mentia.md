- **O stub do retriever de skills parava de mentir (#964).** A doc dizia
  "Returns empty list until then" e o corpo devolvia `Err` — duas frases sobre a
  mesma funcao, discordando. Quem lesse o comentario escreveria
  `retrieve(q)?.is_empty()` e levaria um erro em producao. O comportamento que
  ficou e o `Err`, porque `Ok(vec![])` seria pior: lista vazia e indistinguivel
  de "procurei e nao achei nada", e o chamador seguiria em frente com um recall
  que nunca rodou. A doc tambem citava a dependencia errada (`garraia-embeddings`,
  o crate orfao do #949) e agora aponta o par que de fato funciona na memoria do
  agente. Nao ha chamador no workspace hoje.
