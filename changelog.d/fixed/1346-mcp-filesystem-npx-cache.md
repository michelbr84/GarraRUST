- **O MCP `filesystem` deixa de ficar preso num cache do npx corrompido e para de
  inundar o log (#1346).** A provisao de instalacao nova agora fixa
  `@modelcontextprotocol/server-filesystem@2026.8.31` (testada com handshake em
  node 20 e 22; o mesmo valor vale para o template do admin e o marketplace) em
  vez de baixar o build mais novo do registry a cada cache frio (so o pacote de
  topo: as dependencias dele seguem os ranges semver publicados); um `mcp.json`
  existente nunca e reescrito, e o novo check `mcp.filesystem_pinned` do
  `/api/diagnostics` avisa quem ficou sem versao (dist-tag como `@latest` e
  range como `@^1` contam como sem versao) com os `args` exatos para colar.
  O stderr do processo filho passa a ser capturado (so em `debug`), e um
  `ERR_MODULE_NOT_FOUND` dentro de `<cache npm>/_npx/<16 hex>/` apaga so aquela
  entrada e tenta de novo uma vez: comando `npx`, do stderr so o hash da
  entrada (o diretorio e remontado a partir do cache que o proprio gateway deu
  ao filho, o que tambem cobre perfil com espaco no nome), caminho canonico,
  nunca symlink, `package.json` do pacote configurado, uma vez por servidor por
  processo. Esgotados os
  `max_restarts`, o erro sai uma unica vez em vez de a cada 30 s. O
  `/api/mcp/health` passa a listar servidores que falharam no boot (antes
  respondia `no_mcp_configured`), com `status`, `cause` e `last_error` (que
  nunca leva caminho; em servidor HTTP reflete a ultima tentativa e some quando
  ele reconecta), e o check `mcp.servers` do diagnostico traz o proximo passo
  por causa, nomeando o diretorio so quando o gateway o validou.
