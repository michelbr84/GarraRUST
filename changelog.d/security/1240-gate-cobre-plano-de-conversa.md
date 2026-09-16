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
