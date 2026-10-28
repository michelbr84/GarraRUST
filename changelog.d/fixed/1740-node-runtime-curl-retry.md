- **O download do runtime Node privado retenta o erro 92 do HTTP/2 (#1740).** O
  `fetch()` de `scripts/desktop/node-runtime/build-node-runtime.sh` usava
  `curl --retry 3`, que so repete erros que o curl julga transitorios; o
  `curl: (92) HTTP/2 stream 1 was not closed cleanly: INTERNAL_ERROR` do CDN do
  nodejs.org derrubou o `Build macOS .dmg` em duas runs seguidas sem retentar.
  Agora o download usa `--http1.1 --retry 5 --retry-all-errors`; GPG e SHA-256
  seguem validando o que chegou.
