- **O `mode` pedido na criacao da sessao volta a valer (#1102).** `POST
  /api/sessions` respondia 201 ecando o modo, mas a linha no banco nascia sem
  `agent_mode`: o handler grava o modo e **depois** emite o token de sessao, e
  `ChatSessionManager::create_token` chama `upsert_session(..., Value::Null)`
  so para garantir a linha antes da FK. Como `json_patch(T, P)` devolve `P`
  quando `P` nao e um objeto, aquele "no-op" reescrevia o metadado inteiro com
  `null` por cima do modo recem-gravado — e o modo escolhido e o que liga a
  `ToolPolicy` desde #988, entao a sessao rodava sem a politica pedida. O
  mesmo `upsert` explica por que `/api/mode/select` sempre funcionou: aquele
  caminho nao emite token. Agora um patch que nao e objeto e ignorado (e uma
  linha nova criada assim nasce com `{}`); `null` explicito **dentro** de um
  objeto continua apagando a chave, como o `clear_agent_mode` precisa.
