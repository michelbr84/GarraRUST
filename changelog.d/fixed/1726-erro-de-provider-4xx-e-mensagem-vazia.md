- **Modelo invalido deixa de virar HTTP 500 com o corpo bruto do provider (#1726).**
  `POST /api/sessions/{id}/messages` com `model` inexistente e `POST
  /v1/chat/completions` com um `model` fora de `/v1/models` respondiam 500 com
  `body={"error":{...},"user_id":"user_..."}` -- o erro e do cliente, e o
  `user_id` e a conta OpenRouter do operador, que vazava ate o `garra ask`. Agora
  os providers OpenAI-compativel e Anthropic reduzem o corpo de erro a frase
  `error.message` (mais `code`), o gateway responde 400 para 400/404/413/422 do
  provider, 429 para 429, 502 para 401/403/5xx e falha de rede, e `/v1` devolve
  o objeto de erro no formato da OpenAI. `content` vazio ou so espaco em
  `/api/sessions/{id}/messages` passa a ser 400 sem chamar o modelo.
