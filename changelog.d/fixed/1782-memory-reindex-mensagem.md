`garra memory stats` nao promete mais que o reindex conserta entradas com
vetor e sem modelo registrado: a fila do reindex inclui so entradas sem
vetor (`embedding IS NULL`), e o aviso agora diz isso e aponta o bloco
"Por modelo" do proprio `garra memory stats` como o diagnostico do
mismatch. Teste em `garraia-db` fixa a semantica atual da fila.
