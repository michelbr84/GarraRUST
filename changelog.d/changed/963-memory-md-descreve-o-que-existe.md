- **O `docs/src/memory.md` passa a descrever o sistema que existe (#963).** A
  pagina documentava um produto que nunca foi construido: `garra memory
  add/clear/export/disable`, um `facts.json` com array de fatos datados, e as
  chaves `memory.auto_extract`, `extraction_interval` e `max_facts`. Nenhum
  desses comandos existe; nenhuma dessas chaves e lida. Quem seguisse a pagina
  batia num `error: unrecognized subcommand` e concluia que o produto estava
  quebrado — documentacao errada e pior que documentacao ausente, porque a
  ausente manda a pessoa ler o `--help`.
- Agora estao la os nove subcomandos reais (`stats`, `list`, `search`,
  `reindex`, `backup`, `pin`, `ttl`, `delete`, `compact`), as chaves reais
  (`memory.ingestion.*`, `memory.retention.*`, `embeddings.<nome>`), e as
  quatro metricas do #957. Cada afirmacao foi conferida contra o codigo e
  contra o binario — a tabela de comandos bate com o `garra memory --help`, e a
  frase "diz se rodou semantica ou textual" foi verificada rodando a busca.
- Registra tambem o que **nao** existe, que e metade do valor: nao ha `garra
  memory add`; o retriever do `garraia-learning` segue stub ate a Fase 2.1; e
  os dois gauges de tamanho do indice da #957 ainda nao foram entregues, com o
  motivo (precisam de worker proprio, porque pendura-los no worker de retencao
  os deixaria mortos para quem nao liga a retencao — que e o padrao).
- Esclarece uma confusao que a pagina antiga criava: `fatos.json` existe, mas
  e um **perfil estatico** escrito a mao e injetado no boot, e nao onde os
  fatos extraidos ficam. Os fatos extraidos por LLM (confianca >= 0,80) moram
  no mesmo `memory.db`, como entradas `[FACT]`, e nao passam pelo filtro de
  ruido — um fato extraido e, por definicao, o que o extrator julgou ser sinal.
