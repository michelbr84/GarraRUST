- **O crate `garraia-embeddings` passa a avisar no compilador que e orfao
  (#949).** Ele nao e usado por nada no workspace e descreve um sistema que nao
  roda: pgvector com 768 dimensoes fixas, quando o caminho vivo e
  `garraia_agents::embeddings` + `garraia_db::vector_store` (sqlite-vec, com a
  dimensao derivada do vetor que o provider devolve). O aviso deixou de ser so
  um comentario no `lib.rs` e virou `#![deprecated]`: como o CI roda
  `clippy -D warnings`, um `use garraia_embeddings::...` novo **quebra o
  build** em vez de passar despercebido.
- **A `EMBEDDING_DIM = 768` casa por acaso com o modelo padrao de hoje**
  (`nomic-embed-text`), e e por isso que ela engana: a constante parece certa
  enquanto a afirmacao que ela faz — "o sistema tem uma dimensao, e e esta" — e
  falsa. Trocar o numero consertaria o valor e manteria o erro.
- **A decisao de remover ou alinhar o crate esta na ADR 0018, com status
  `Proposed`** — a regra absoluta 8 pede o ADR antes da decisao, e a decisao e
  do dono. Nada foi removido.
