- **`POST /v1/chat/completions` sem `model` deixa de mandar `"gpt-4"` ao backend (#1541).**
  O handler fazia `body.model.unwrap_or_else(|| "gpt-4")` e entregava isso ao
  runtime como se o cliente tivesse pedido, entao um llama-server ou Ollama
  local respondia 404 `model 'gpt-4' not found` — a API OpenAI-compat era
  inutilizavel justamente para o cliente que omite `model` porque quer o
  default do servidor. Agora o corpo sem `model` chega ao provider como o
  sentinel vazio que ele ja sabe resolver com o proprio `configured_model`
  (mesma decisao que o A2A tomou), e o campo `model` do response/SSE ecoa o
  modelo que de fato atendeu. Os outros tres defeitos da #1541 (prefixo de
  chave de perfil, model do fallback e erro engolido no SSE) seguem abertos.
