- **Turno de streaming vazio deixa de virar bolha em branco no canal (#1048).**
  Quando o stream do provider terminava sem nenhum `TextDelta` e sem nenhuma
  ferramenta, o turno devolvia string vazia e o Telegram — como qualquer outro
  canal — publicava uma mensagem em branco. O retry e o fallback de provider
  nao pegavam o caso: `stream_complete_with_fallback` devolve o stream **antes**
  de qualquer evento existir, e `is_retryable_error` so olha texto de erro, e
  aqui nao ha erro nenhum a olhar. A deteccao passou para o consumidor do
  stream: turno vazio refaz a rodada pelo caminho nao-streaming, que tem retry
  e fallback de verdade. O redo e limitado a um por turno, porque nenhuma
  guarda do loop conta turno vazio e sem o limite um provider mudo giraria sem
  parar.
- **O caminho nao-streaming parava de publicar um marcador interno como se
  fosse resposta (#1048).** Ali o vazio nao dava bolha em branco: dava
  `[no textual response provided by the model]`, em ingles, que o usuario le
  como resposta do modelo. `extract_text` continua devolvendo o marcador para
  quem precisa de uma `String` sempre, mas a decisao passou a usar a irma
  `extract_text_opt`, que preserva o "veio vazio". Turno inteiramente vazio
  agora termina em erro explicito, que o canal mostra. O `text_len` do log do
  batch tambem media o marcador, e passou a medir a resposta.
