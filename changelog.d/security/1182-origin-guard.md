- **Pagina web qualquer nao dispara mais POST/PATCH/DELETE contra o gateway local (#1182).**
  `/api/*` e auth-free por desenho — "quem alcanca a porta e o dono" — mas o navegador
  do dono alcanca a porta rodando codigo de terceiros: bastava ele visitar uma pagina
  para ela mandar `PATCH /api/settings`, `POST /api/mode/select`,
  `POST /api/mcp/marketplace/install`, `POST /v1/chat/completions` (gastando a chave de
  LLM dele), `DELETE /api/memory` ou `POST /admin/api/setup` (que cria o primeiro admin
  numa instalacao nova e nao tinha CSRF proprio) contra `127.0.0.1:3888`. O #1093 tinha
  fechado so `/api/learning/*`; agora a guarda e generica
  (`garraia_gateway::origin_guard`) e cobre toda a superficie mutante, com a mesma
  comparacao estrita de `Origin` contra o transporte e o `Host`, mais uma ancora
  anti-DNS-rebinding que aceita IP literal, `localhost`/`*.localhost` ou nome declarado
  em `gateway.allowed_origins`. Em HTTP/2 o `Host` vem da `:authority`, entao o console
  servido com TLS nativo continua passando.
- **CORS default deixa de ser allow-all (#1182).** `gateway.allowed_origins` vazio (o
  default de instalacao) anunciava `Access-Control-Allow-Origin: *`, entao a pagina do
  atacante nao so disparava a escrita como lia a resposta — `/api/settings/effective`
  inteiro, por exemplo. Agora vazio significa nenhuma origem cross-origin. O Web Console
  e servido pelo proprio gateway e e same-origin (nao usa CORS); cliente nao-navegador
  (app mobile, `curl`, Claude Code) ignora CORS. Uma entrada `*` na lista e ignorada com
  aviso em vez de derrubar o gateway no boot.
- **`/ws` e `/ws/parrot` passam a checar o `Origin` do handshake (#1182).** WebSocket nao
  passa por CORS, entao `new WebSocket("ws://127.0.0.1:3888/ws/parrot")` de qualquer
  pagina abria um turno completo do agente — com as tools, com a chave de LLM do dono,
  escrevendo na sessao persistente do Garra Desktop e lendo a resposta de volta; a rota
  nem tinha gate de `api_key` (ele cobre so `/api/*`). Handshake sem `Origin` (app, CLI,
  `curl`) segue como antes. A webview Tauri do desktop passa pela sua origem exata
  (`tauri://localhost`; `http://tauri.localhost` no Windows — `origin_guard::ORIGENS_TAURI`,
  derivada do fonte do Tauri 2.11 e nao medida em runtime nesta entrega; se o passaro
  parar de conectar, o log diz `ws: cross-origin upgrade refused`). Residual registrado
  em `docs/security/threat-model.md` secao 5.10: leitura `GET` sob DNS rebinding nao e
  fechada por regra de `Origin` (o navegador nao manda `Origin` em GET same-origin);
  a mitigacao e `gateway.api_key`.
