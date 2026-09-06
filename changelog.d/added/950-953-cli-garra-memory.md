- **`garra memory` inspeciona, repara e limpa a memoria semantica (#950, #953).**
  Ate aqui a memoria era caixa-preta: nao havia como saber quantas entradas
  existiam, quantas tinham vetor, se o indice estava consistente, nem como
  consertar as que a cadeia de perda silenciosa (#948, #951, #962) deixou sem
  embedding — e o `docs/src/memory.md` chegava a documentar seis subcomandos
  que nunca existiram. Agora sao seis de verdade: `stats` mostra o relatorio de
  integridade do #960 (inclusive a fila legada de vetor sem modelo), `list`
  lista as entradas recentes ou so a fila de reindexacao, `search` roda o mesmo
  recall do agente e diz se foi semantico ou textual, `reindex` reprocessa as
  entradas gravadas sem vetor, `delete` apaga uma entrada com o vetor dela e
  `compact` apaga tudo anterior a N dias. Os dois destrutivos exigem confirmacao
  e, sem terminal, exigem `--yes` explicito. `stats`, `list`, `search` e
  `reindex` tem `--json` para script. O reindex para no primeiro lote que falha
  em vez de insistir: a fila e derivada do banco, entao rodar de novo depois
  continua de onde parou. A CLI abre o **mesmo** `memory.db` do gateway
  (`AppConfig::memory_db_path`, agora fonte unica) e pede o **mesmo** provider
  de embeddings (`bootstrap::build_embedding_provider`, extraido de
  `build_agent_runtime`) — o que o `reindex` grava e exatamente o que o recall
  do agente le de volta.
