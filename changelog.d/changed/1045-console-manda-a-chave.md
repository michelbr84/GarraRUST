- console web: as chamadas para `/api/*` passam a levar `Authorization:
  Bearer` com a `gateway.api_key` guardada, a mesma que o console ja mandava
  como `?token=` no WebSocket. Sem chave configurada nada muda. Prepara o
  gate do REST, que entra em seguida. (#1045)
