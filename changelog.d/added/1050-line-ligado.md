- canais: o LINE passa a ter rota. `POST /webhooks/line` verifica a assinatura
  `X-Line-Signature` sobre os **bytes crus** do corpo antes de qualquer parse,
  e e a assinatura — nao um campo do corpo, que quem manda controla — que
  decide de qual canal LINE configurado e o webhook. Todo modo de falha
  responde o mesmo 403 com o mesmo corpo, para o erro nao virar um oraculo.
  O `LineChannel` tinha `impl Channel` e a verificacao de assinatura (#1051)
  desde antes, e nenhum call-site. `garra config check` ganha checagem propria
  dos dois segredos: a generica procura `bot_token`/`access_token`/`app_token`
  e nao alcancava `channel_access_token` nem `channel_secret`. (#1050)
