- **Turno curto ou puramente social deixa de poluir a busca semantica (#952).**
  `"oi"`, `"ok"`, `"kkkk"` e `"bom dia"` eram embeddados como qualquer outra
  mensagem e disputavam o top-K com memoria de verdade — texto curto tem
  cosseno alto com quase tudo. Numa base de ~7.100 entradas, "quem e Michel"
  trazia entradas `"oi"` entre os primeiros resultados. Agora a ingestao
  decide se vale gastar um vetor: a entrada **continua gravada** e achavel
  pelo recall textual, so nao entra no indice vetorial. A regra de frase so
  casa com o conteudo **inteiro** (`"obrigado"` e ruido, `"obrigado pela
  ajuda com o deploy"` nao), e nada e apagado nem retroativo — vetor de ruido
  ja indexado continua onde esta. Configuravel em `memory.ingestion`
  (`filter_noise`, `min_chars`, `extra_noise_phrases`), validado pelo
  `garra config check`, documentado em `docs/src/memory-ingestion.md`.
  `garra memory reindex` usa a **mesma** politica — com politicas divergentes
  ele reembeddaria uma por uma as entradas que a ingestao acabou de pular — e
  passa a separar no relatorio o que seria reindexado do que fica sem vetor
  de proposito, para que o total de "sem vetor" do `stats` pare de parecer
  defeito.
