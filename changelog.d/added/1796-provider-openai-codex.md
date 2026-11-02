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
