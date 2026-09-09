- canais: o Microsoft Teams passa a ter rota. `POST /webhooks/teams` autentica
  cada requisicao pelo JWT RS256 do Bot Framework, conferindo assinatura,
  emissor e audiencia (o `app_id` do bot) contra o JWK Set publico. O `app_id`
  e obrigatorio e o canal e recusado no boot sem ele: todos os tokens do Bot
  Framework sao assinados pelas mesmas chaves, entao sem `aud` um token
  emitido para o bot de outra pessoa passaria. (#1050)
