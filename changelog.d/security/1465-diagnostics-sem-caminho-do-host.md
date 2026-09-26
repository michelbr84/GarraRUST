- **`/api/diagnostics` nao publica mais caminho absoluto do host para raiz
  do MCP `filesystem` fora do `data_dir` (#1465).** As linhas
  `execution.profile` (em `isolated-pod`) e `mcp.filesystem_root` caiam no
  fallback que imprimia o caminho como esta — no caso legado mais comum, o
  `$HOME` do host com o nome de usuario — numa rota auth-free. Agora uma raiz
  fora do `data_dir` sai so como contagem (`1 fora do data_dir`), e a raiz
  ofensora e apontada pela posicao na lista do servidor (`raiz #2 do servidor
  filesystem`), nunca pelo caminho; e a mesma regra que `files.workspace` ja
  aplicava a raiz declarada. O caminho continua no `mcp.json`/`config.yml` e
  no log de boot.
