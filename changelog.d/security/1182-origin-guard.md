- **Pagina web qualquer nao dispara mais POST/PATCH/DELETE contra o gateway local (#1182).**
  `/api/*` e auth-free por desenho — "quem alcanca a porta e o dono" — mas o navegador
  do dono alcanca a porta rodando codigo de terceiros: bastava ele visitar uma pagina
  para ela mandar `PATCH /api/settings`, `POST /api/mode/select`,
  `POST /api/mcp/marketplace/install`, `POST /v1/chat/completions` (gastando a chave de
  LLM dele) ou `DELETE /api/memory` contra `127.0.0.1:3888`. O #1093 tinha fechado so
  `/api/learning/*`; agora a guarda e generica (`garraia_gateway::origin_guard`) e cobre
  toda a superficie mutante, com a mesma comparacao estrita de `Origin` contra o
  transporte e o `Host`, mais uma ancora anti-DNS-rebinding que aceita IP literal,
  `localhost` ou nome declarado em `gateway.allowed_origins`.
- **CORS default deixa de ser allow-all (#1182).** `gateway.allowed_origins` vazio (o
  default de instalacao) anunciava `Access-Control-Allow-Origin: *`, entao a pagina do
  atacante nao so disparava a escrita como lia a resposta — `/api/settings/effective`
  inteiro, por exemplo. Agora vazio significa nenhuma origem cross-origin. O Web Console
  e servido pelo proprio gateway e e same-origin (nao usa CORS); cliente nao-navegador
  (app mobile, `curl`, Claude Code) ignora CORS.
- **`/ws` passa a checar o `Origin` do handshake (#1182).** WebSocket nao passa por CORS,
  entao `new WebSocket("ws://127.0.0.1:3888/ws")` de qualquer pagina abria uma sessao de
  chat completa. Handshake sem `Origin` (app, CLI, `curl`) segue como antes. O
  `/ws/parrot` do Garra Desktop **nao** foi alterado: o `Origin` real da webview Tauri
  nao pode ser verificado neste ambiente e uma allowlist adivinhada derrubaria o desktop
  em producao — fica como follow-up, registrado em `docs/security/threat-model.md`
  secao 5.10.
