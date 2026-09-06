- **Memoria do agente ganha prazo de validade e fixacao (#959).** A memoria de
  workspace (`rest_v1/memory.rs`) ja tinha `ttl_expires_at` e `pinned_at`; a
  memoria que alimenta o recall semantico nao tinha nenhum dos dois — memoria
  obsoleta (preferencia que mudou, fato de sessao antiga) ficava para sempre
  poluindo o recall, e nao havia como proteger da compactacao o que importava.
  Agora `memory_entries` tem as duas colunas (migracao aditiva, forward-only:
  banco existente ganha as colunas na abertura) e a CLI tem
  `garra memory pin <id> [--unpin]` e `garra memory ttl <id> <dias|--clear>`.
  Entrada vencida sai do recall **na hora**, pelo caminho textual e pelo KNN —
  o indice vec0 so conhece distancia, entao o filtro tambem foi para o fetch
  dos candidatos, como o #971 teve de fazer para tenant. Entrada fixada nunca e
  apagada pela compactacao, automatica ou manual. `garra memory stats` conta as
  fixadas e as vencidas.
