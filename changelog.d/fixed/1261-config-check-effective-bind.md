- **`config check` deixa de opinar sobre um bind que nunca sobe (#1261).** O achado de
  exposicao lia so `gateway.host` do arquivo, mas `garra start` sobrescreve host e porta
  com `--host`/`--port` do clap, que por sua vez leem as envs `HOST`/`PORT`. Resultado:
  um arquivo em `127.0.0.1` com `HOST=0.0.0.0` passava calado — falsa garantia para quem
  consultou o diagnostico antes de expor a porta — e um `0.0.0.0` no arquivo que a env
  cobria virava aviso sobre valor morto. Agora o check avalia o bind efetivo que ele
  consegue ver (env vence arquivo), nomeia a origem de cada metade e admite no proprio
  texto o que nao ve: a flag de um `garra start` futuro, que roda noutro processo. Sem
  mudanca de comportamento de boot — so do que o relatorio afirma.
