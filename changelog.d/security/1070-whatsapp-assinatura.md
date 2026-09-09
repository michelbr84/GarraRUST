- O webhook do WhatsApp passa a verificar a assinatura `X-Hub-Signature-256`
  (HMAC-SHA256 do app secret sobre os bytes crus do corpo). Ate aqui
  `POST /webhooks/whatsapp` aceitava qualquer requisicao, num canal que ja
  estava ligado em producao: quem descobrisse a URL podia mandar mensagem como
  qualquer numero, gastar token do provider a cada POST, forjar um `from` da
  allowlist e, numa instalacao nova, reivindicar o papel de owner. Um canal sem
  `app_secret` agora e recusado no boot em vez de subir em modo degradado, e o
  `hub.verify_token` do handshake passou a comparacao em tempo constante
  (#1070).
