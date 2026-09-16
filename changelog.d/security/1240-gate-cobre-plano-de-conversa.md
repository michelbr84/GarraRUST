- **O gate de `gateway.api_key` passa a cobrir o plano de conversa e o A2A (#1240).**
  `POST /v1/chat/completions`, `POST /v1/messages`, `POST /v1/messages/count_tokens`
  e todo o `/a2a/*` estavam montados no mesmo router cru que `/api/*`, mas fora do
  gate, que testava so o prefixo `/api/`. A justificativa original — "`/v1/*` tem
  JWT proprio" — vale para o `rest_v1` do workspace e para o `/v1/auth/*`, e nao
  para as rotas compat OpenAI/Anthropic, que nao resolvem identidade nenhuma. Era
  fail-open de um controle que o operador tinha ligado: quem configurava a chave
  acreditava ter fechado a porta, e a superficie que executa as tools do GarraIA na
  maquina dele seguia respondendo a qualquer `curl`. O recorte agora e um conjunto
  explicito: `/api/` menos a allowlist de descoberta, mais as tres rotas de
  conversa por igualdade exata, mais `/a2a/` por prefixo. As duas rotas compat
  Anthropic aceitam a chave tambem em `x-api-key`, porque o Claude Code e o SDK da
  Anthropic nunca mandam `Authorization: Bearer`; nunca por query string.
  `/v1/models`, `/.well-known/agent.json`, `/health` e `/ping` seguem abertas.
  **Sem `gateway.api_key` configurada nada muda** — este PR nao fecha nada por
  default.

  **O socket do papagaio (`/ws/parrot`) entrou junto**, achado pela auditoria R4
  do PR. Ele executa um turno completo do agente — com as tools e a chave de LLM
  do dono — sobre a sessao persistente do Garra Desktop, e a unica guarda que
  tinha era o anti-CSRF da #1182, que passa de proposito quando nao ha header
  `Origin`, porque cliente nao-navegador (app, CLI, `curl`) nao manda um. Com a
  chave configurada e o gateway em `0.0.0.0` — o caso do app na LAN, que e o
  motivo de a chave existir — um `websocat ws://host:3888/ws/parrot` conectava
  sem credencial e dirigia o agente na maquina do dono. O irmao `/ws`, montado na
  linha de cima do `router.rs`, ja checava a chave desde a #1045. A checagem ficou
  **dentro do handler**, e nao na lista de caminhos do middleware: o middleware so
  le header, e a webview Tauri abre o overlay com `new WebSocket(...)`, que nao
  manda header — gatear a rota la fecharia o desktop em vez de autentica-lo. Como
  no `/ws`, a chave vai por `?token=` / `?api_key=` ou por bearer, com a mesma
  comparacao de tempo constante e o mesmo 401.
