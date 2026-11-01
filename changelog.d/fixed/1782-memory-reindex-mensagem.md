- **`garra memory stats` deixa de prometer conserto que o reindex nao faz
  (#1782).** A fila do reindex inclui so entradas sem vetor (`embedding IS
  NULL`), mas o aviso dizia que `garra memory reindex` tambem conserta
  entradas com vetor e sem modelo registrado - remedicao que provadamente nao
  fazia nada. O aviso novo diz isso e aponta o bloco "Por modelo" do proprio
  `garra memory stats` como o diagnostico do mismatch. Teste em `garraia-db`
  fixa a semantica atual da fila (entrada com vetor e sem modelo nao entra).
