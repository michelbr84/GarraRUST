- **Cobertura e Quality Ratchet editam um comentario so por PR (#1228).** Os
  dois postavam um comentario novo a cada push, e num PR com dez pushes a
  conversa de review sumia no meio de vinte relatorios quase iguais. Agora
  `scripts/ci/upsert_pr_comment.py` edita no lugar o comentario achado pelo
  marcador **e** pelo autor `github-actions[bot]` — um comentario humano que
  cite o marcador nunca e sobrescrito. O comentario de cobertura encolheu para
  a linha TOTAL e o link do run; a tabela por arquivo vai para o job summary.
  Permissoes, gatilhos e a guarda de fork continuam os mesmos.
