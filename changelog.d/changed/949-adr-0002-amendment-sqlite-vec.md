- **A ADR 0002 passa a dizer o que foi construido (#949).** Ela decidia
  "pgvector, 768 dimensoes fixas, mxbai" e ninguem voltou nela quando a memoria
  do agente foi construida em **sqlite-vec**, com uma tabela `vec_embeddings_{dims}`
  por dimensao, criada sob demanda a partir do vetor que o provider devolve.
  Quem lia o ADR de cima a baixo acreditava num sistema que nao existe em
  instalacao nenhuma. A Amendment de 2026-09-06 registra a divergencia campo a
  campo, explica por que ela aconteceu (memoria do agente e local-first e
  mono-usuario; exigir Postgres teria matado o caso de uso principal) e delimita
  o que do ADR continua valendo — tudo que ele decide para o workspace
  multi-tenant em Postgres. O status segue `Accepted`: o documento nao estava
  errado, estava sem escopo. O crate orfao `garraia-embeddings` ganha um aviso
  no topo dizendo que **nao** e o caminho em uso e apontando para o par que e
  (`garraia_agents::embeddings` + `garraia_db::vector_store`). Remover ou alinhar
  o crate fica como decisao propria, com ADR, porque o `CLAUDE.md` o registra
  como scaffold deliberado da Fase 2.1.
