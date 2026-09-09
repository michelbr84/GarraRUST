- canais: o Google Chat passa a ter rota. `POST /webhooks/google-chat`
  autentica cada requisicao pelo JWT RS256 que o Google manda no
  `Authorization`, conferindo assinatura, emissor e **audiencia** contra o
  JWK Set publico do `chat@system.gserviceaccount.com`. A audiencia e
  obrigatoria e o canal e recusado no boot sem ela: todo webhook do Chat,
  de toda app, e assinado pela mesma chave do Google, entao sem conferir o
  `aud` um token legitimo emitido para a app de outra pessoa passaria. As
  chaves publicas ficam num cache que se renova sozinho, com piso entre
  buscas para o `kid` do token — que quem manda a requisicao controla — nao
  virar gatilho de trafego de saida. `garra config check` ganha checagem
  propria dos dois campos. (#1050)
