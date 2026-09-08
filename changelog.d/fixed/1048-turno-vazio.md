- **Turno de streaming vazio deixa de virar bolha em branco no canal (#1048).**
  Quando o stream do provider terminava sem nenhum `TextDelta` e sem nenhuma
  ferramenta, o turno devolvia string vazia e o Telegram — como qualquer outro
  canal que passe pelo caminho de streaming — publicava uma mensagem em branco.
  O retry e o fallback de provider nao pegavam o caso: `stream_complete_with_
  fallback` devolve o stream **antes** de qualquer evento existir, e
  `is_retryable_error` so olha texto de erro, e aqui nao ha erro nenhum a
  olhar. A deteccao passou para o consumidor do stream: volta vazia refaz a
  rodada pelo caminho nao-streaming, que tem retry e fallback de verdade. O
  redo e limitado a um por turno, porque nenhuma guarda do loop conta volta
  vazia e sem o limite um provider mudo giraria sem parar. Turno inteiramente
  vazio, nos dois caminhos, termina em erro explicito, que o canal mostra, em
  vez de fingir que respondeu — mas so quando **nada** foi entregue: se o
  modelo ja mandou texto nas voltas anteriores, o turno encerra com o que ha,
  porque descartar resposta ja publicada seria pior que o bug original.
- **O ramo nao-streaming de dentro do turno de streaming parou de publicar um
  marcador interno como se fosse resposta (#1048).** Ali o vazio nao dava bolha
  em branco: dava `[no textual response provided by the model]`, em ingles, que
  o usuario le como resposta do modelo. `extract_text` continua devolvendo o
  marcador para quem precisa de uma `String` sempre, mas a decisao passou a
  usar a irma `extract_text_opt`, que preserva o "veio vazio". O `text_len` do
  log do batch tambem media o marcador, e passou a medir a resposta. O escopo
  e esse: o caminho nao-streaming **autonomo** (`process_message_with_agent_
  config`, que serve o app mobile, o `/ws`, o `/v1/chat/completions` nao-
  streaming e o chat REST) continua publicando o marcador, e sai num trabalho
  proprio.
