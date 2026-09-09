- O webhook do WhatsApp passa a verificar a assinatura `X-Hub-Signature-256`
  (HMAC-SHA256 do app secret sobre os bytes crus do corpo). Ate aqui
  `POST /webhooks/whatsapp` aceitava qualquer requisicao, num canal que ja
  estava ligado em producao: quem descobrisse a URL podia mandar mensagem como
  qualquer numero, gastar token do provider a cada POST, forjar um `from` da
  allowlist e, numa instalacao nova, reivindicar o papel de owner. Um canal sem
  `app_secret` agora e recusado no boot em vez de subir em modo degradado
  (#1070).
- Com mais de um canal WhatsApp configurado, quem atende passa a ser o canal
  que **assinou**, e nao o que o `metadata.phone_number_id` do corpo aponta.
  O roteamento por corpo deixava quem conhecesse o app secret de um canal
  assinar um corpo apontando para outro e receber a resposta enviada com o
  `access_token` do outro — escalada entre canais a partir da credencial de
  menor valor. Um corpo assinado que reivindica o numero de outro canal
  configurado agora e descartado (#1070).
- O `hub.verify_token` do handshake `GET` do WhatsApp passou a comparacao em
  tempo constante sobre digests SHA-256 dos dois lados, o que tambem para de
  vazar o comprimento do token configurado pelo tempo de resposta (#1070).
