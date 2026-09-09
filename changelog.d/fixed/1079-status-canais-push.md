- `GET /api/channels` parou de reportar os quatro canais push (WhatsApp,
  Google Chat, Teams, LINE) como `offline` num gateway saudavel. Canal push
  nao entra no `ChannelRegistry` por desenho — vira estado da rota
  `/webhooks/*` —, entao derivar status do registry dava `offline` eterno.
  O `KNOWN_CHANNELS` ganhou uma coluna `kind` (`Pull` | `Push`) e o status
  dos push passa a vir das listas que de fato subiram, e nao do arquivo de
  config: um canal configurado mas recusado no boot continua aparecendo
  como `offline`, que e a verdade. (#1079)
