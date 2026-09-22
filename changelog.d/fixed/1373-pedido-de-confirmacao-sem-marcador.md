- **O pedido de confirmacao chega ao usuario sem o marcador interno (#1373).**
  Quando um comando de risco pausava o turno, o texto da ferramenta subia
  inteiro como resposta, e o usuario do WhatsApp (e de todo canal e do web
  chat) recebia `[CONFIRM_REQUIRED:6b2e7f7e9f135cbc] O comando a seguir requer
  confirmacao...`. O marcador sai no unico ponto em que o pedido vira texto do
  humano, o despacho de ferramenta do runtime, que alimenta as quatro copias do
  loop e o `tool_program` - e sai tambem da linha da ferramenta nos sinks de
  eventos (o resumo e a saida do `tool_finished` que a `garraia chat` desenha e
  que o `/ws` envia), que o mesmo despacho mandava antes da pausa com o
  conteudo cru. A frase que fica continua dizendo o comando e "Responda
  **sim** para executar". A aprovacao nao muda: a impressao digital segue no
  `ToolResult` do historico e no registro de pedidos pendentes, e digitar o
  marcador nunca aprovou nada.
