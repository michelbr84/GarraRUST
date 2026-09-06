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

- **`garra memory reindex` tambem repara o indice, sem custo de provider (#950).**
  O `remember_sync` grava a linha e insere no indice em best-effort: com o
  sqlite-vec fora do ar por um momento, a entrada fica com vetor na coluna e
  **fora** da busca semantica, e o reindex normal nao a alcanca porque ele
  procura `embedding IS NULL`. O reparo agora roda antes, sem chamar provider
  nenhum — o vetor ja existe, so falta indexa-lo — e aparece como
  `index_repaired` no relatorio. Na mesma linha, `set_embedding` virou
  fail-closed: se o indice recusar o vetor, a coluna volta a NULL, para a
  entrada continuar na fila em vez de sair dela sem ter sido indexada.
