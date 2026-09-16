- **`POST /api/mcp/marketplace/install` deixa de aceitar chamador anonimo.** O
  handler nascia com a assinatura `(State, Json<InstallMcpRequest>)` — sem
  extractor nenhum de autenticacao ou autorizacao — e a rota era montada direto
  no grupo aberto do `build_router`. Qualquer chamador que alcancasse a porta
  registrava um servidor MCP do catalogo no registry do gateway, e escolhia
  pelo CORPO do pedido tanto os `extra_args` anexados ao comando quanto o `env`
  do processo que o gateway ia executar. Sao dois problemas empilhados: a rota
  sem gate, e a rota aceitando ambiente e argumentos arbitrarios para um
  processo filho.
  O que torna o alcance maior do que "so quem chega na porta": as rotas `/api/*`
  sao auth-free por desenho, e o navegador do dono alcanca a porta rodando
  codigo de terceiro. A guarda anti-CSRF generica da #1182 ja cobria este
  caminho, mas ela fecha o que o navegador pode ser forcado a fazer, nao o
  pedido direto — e o gate de `gateway.api_key` e opt-in.
  A correcao reusa exatamente o padrao das rotas irmas em vez de inventar
  mecanismo novo: a rota sai do grupo aberto e passa a viver num sub-router com
  `require_admin_auth` + `require_csrf` + `security_headers`, espelhando o
  `plugins_handler::build_plugin_routes` que cobre `/api/plugins/*`, e o handler
  confere `Permission::ManagePlugins` — a mesma permissao que o enum ja descreve
  como "Plugin / MCP server management" e que `admin_create_mcp` exige para
  registrar um servidor MCP pela admin API. Nenhuma permissao nova foi criada:
  `POST /api/plugins/install` e `POST /admin/api/mcp` sao a mesma capacidade por
  outra porta. `Role::Admin` e `Role::Operator` passam; `Role::Viewer` leva 403.
  `extra_args` e `env` continuam existindo — sao recurso legitimo para instalar
  com configuracao propria, e o catalogo depende do `env` para credencial
  (`GITHUB_PERSONAL_ACCESS_TOKEN`, `POSTGRES_CONNECTION_STRING`, ...). O que
  entrou junto foi uma denylist estreita: o corpo nao pode mais definir `PATH`,
  `LD_PRELOAD`, `LD_LIBRARY_PATH`, `LD_AUDIT`, `DYLD_INSERT_LIBRARIES`,
  `DYLD_LIBRARY_PATH` nem `NODE_OPTIONS` (comparacao case-insensitive), que sao
  as variaveis que decidem QUAL codigo o filho executa — o comando vem do
  catalogo (`npx -y @modelcontextprotocol/server-...`), e deixar o corpo trocar
  a resolucao do binario ou pre-carregar uma biblioteca faz o `id` vetado nao
  garantir mais nada. Secrets do gateway ficaram deliberadamente FORA da
  denylist: defini-las no filho nao vaza a do gateway — o vazamento era a
  heranca do ambiente, fechada na #1236 com `env_clear()` — e barra-las
  quebraria o caso legitimo do catalogo.
  As tres rotas `GET` do marketplace (catalogo, health, config-schema) seguem
  abertas de proposito: sao leitura e nao mudam estado.
  O teste de regressao cobre a matriz de autorizacao (sem cookie → 401, cookie
  invalido → 401, sessao sem CSRF → 403, `Viewer` → 403, `Operator`/`Admin` →
  201, env bloqueada → 400) e, alem do sub-router, exercita o router de
  producao: em axum a primeira rota registrada para um caminho vence, entao sem
  essa afirmacao alguem re-adicionando a rota ao grupo aberto faria o merge do
  sub-router protegido perder em silencio. Issue #1245.
