- A documentacao descreve a precedencia real do bind do gateway: `--host`/
  `--port` explicitos > `HOST`/`PORT` de ambiente > defaults `127.0.0.1:3888`,
  e as chaves `gateway.host`/`gateway.port` do arquivo de config NAO alimentam
  o `garra start` (medido na v0.4.2). `docs/auth-config.md` ganhou a secao 5.1
  com a cadeia e as consequencias para `config check`; os trechos de
  `docs/installation.md` e `docs/deployment-runpod.md` que tratavam o valor do
  arquivo como governante do bind foram corrigidos (#1261).
