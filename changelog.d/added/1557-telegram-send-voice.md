- **Tool `telegram_send_voice`: a Garra fala por iniciativa propria (#1557).**
  Sintetiza o texto (ate 1000 caracteres) com o TTS configurado e entrega como
  voice message no Telegram. O destino e a mesma decisao do `telegram_send`
  (chat da sessao, ou `chat_id` explicito so se estiver em
  `proactive_chat_ids`), compartilhada em `resolver_alvo_telegram`. Mapeada para
  `MessageSend` na tabela de capacidades, com teto proprio de 2 vozes por minuto
  por sessao. Falha de TTS vira erro explicito, sem fallback silencioso para
  texto; a tool some da lista quando falta TTS ou o canal esta offline. O
  adaptador Telegram aceita `MessageContent::Audio` apenas com caminho local.
