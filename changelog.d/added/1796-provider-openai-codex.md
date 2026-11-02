- **Provider `openai-codex`: o plano do ChatGPT pelo Sign in with ChatGPT (#1796).**
  Login oficial da OpenAI para apps open-source (OAuth + PKCE S256, registro
  dinamico, escopo `chatgpt.tokens.use.direct`, ID token validado em RS256
  contra o JWKS) e inferencia no endpoint publico `POST /v1/responses` com
  `store: false`, `stream: true` e `reasoning.effort` (padrao `high`). Modelo
  preferencial `gpt-6.1-sol`. Credenciais em `credentials/openai-codex` (0600,
  renovacao serializada entre processos, revogacao no logout). Novo comando
  `garra chatgpt` (login, logout, status, modelos, usar, configurar, testar) e
  cartao "Continuar com ChatGPT" na pagina Providers da porta 3888, com teste
  de inferencia real. Guia em `docs/src/guides/connect-chatgpt.md`.
- **ChatGPT: niveis de raciocinio e velocidade.** O cartao e o
  `garra chatgpt configurar` oferecem Minimal, Low, Medium, High, Extra High,
  Max e Ultra, e as velocidades Thinking (padrao), Fast e Ultrafast. Cada
  opcao so fica ativa quando o catalogo da conta a oferece para o modelo e a
  Responses API a aceita; Ultra existe so no Codex e aparece desativado com o
  motivo. Fast envia `service_tier: priority` e o teste mostra o tier que a
  OpenAI de fato serviu.
- **ChatGPT: limite de uso do plano passa para o fallback.** Quando a OpenAI
  responde `subscription_sharing_usage_limit_exceeded`, o runtime nao repete o
  pedido e passa direto para o primeiro provider de `agent.fallback_providers`
  (por exemplo o OpenRouter), no chat em streaming e no modo batch. Sem conta
  conectada o comportamento e o mesmo.
