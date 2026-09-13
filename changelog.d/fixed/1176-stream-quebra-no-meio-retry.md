- **Stream que quebra no meio agora refaz o turno em vez de matar ele (#1176).**
  `stream read error: error decoding response body` (upstream de provider free
  cortando o corpo, rede movel) matava o turno de primeira: sem retry, sem
  fallback, so a mensagem de erro no canal. Quando nada tinha ido ao sink
  ainda, o turno agora refaz uma vez pelo caminho batch (mesmo redo do
  #1048), que tem retry/fallback de verdade. Com texto ja entregue ao
  usuario, o erro original continua chegando — reenviar duplicaria o que ja
  apareceu no canal.
