- **iMessage: o campo da sala chama-se `room_id` (#1461).** Ele sempre foi
  `message.cache_roomnames` do `chat.db` (`chat<digitos>`, o identificador que
  o servidor gera e que iguala `chat.chat_identifier`), nunca o
  `chat.display_name` que os participantes renomeiam — mas chamava-se
  `group_name`, e foi esse nome que fez uma revisao de seguranca ler
  "texto livre renomeavel" onde ha um id estavel. Rename puro, sem mudanca de
  comportamento; o doc do campo diz o que ele e e o que nao e.
