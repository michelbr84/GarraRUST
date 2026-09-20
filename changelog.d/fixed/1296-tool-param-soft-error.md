- **Tools nativas devolviam `Err(Error::Agent(...))` em parâmetro obrigatório
  ausente.** Quatorze pontos de validação em dez tools (`file_read`,
  `file_write`, `bash`, `web_fetch`, `web_search`, `git_diff`, `device_read`,
  `device_execute`, `schedule_heartbeat`, `schedule_recurring`) tratavam
  chamada malformada do modelo como erro fatal de turno: o dispatch do
  runtime converte o `Err` em texto com o prefixo enganoso `agent error:` e
  sem orientação de schema, e caminhos sem amortecimento (orquestrador)
  falham o step inteiro. Agora toda ausência/tipo errado de parâmetro volta
  como observação soft (`Ok` + `is_error`) com mensagem que nomeia o
  parâmetro, o schema da chamada e pede o reenvio — o modelo se autocorrige
  no mesmo turno, no padrão que `list_dir` e `channel_send` já seguiam.
  `Err` fica reservado a falha ambiental real (IO, permissão, jail,
  recusa de política).
