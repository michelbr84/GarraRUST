- **WhatsApp: nenhum descarte de mensagem e mais mudo (#1815).** Falha de
  decriptacao, mensagem offline vencida, enquete e outros conteudos sem texto
  viram um evento `dropped` da ponte; o gateway loga (`INFO`, so os quatro
  ultimos digitos) e conta como rejeicao (`decrypt_failed`, `offline_expired`,
  `unsupported_content`), visivel no Web Console e no `/api/diagnostics`.
  Audio e foto de terceiro em 1:1 tambem contam. Os eventos `log` e `error` da
  ponte (inclusive falha de envio da resposta) passam a ir para o log do
  gateway em vez de serem ignorados.
