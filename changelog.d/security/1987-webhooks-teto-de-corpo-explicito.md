- **O corpo dos webhooks push passa a ter teto declarado, conferido antes do parse (#1987).**
  WhatsApp, LINE, Teams e Google Chat recebem o corpo como `Bytes` e fazem o
  `serde_json::from_slice` a mao, porque a autenticacao (HMAC sobre os bytes crus,
  ou JWT) tem de vir antes. O limite de tamanho era o default do Axum para `Bytes`
  — 2 MiB que ninguem escreveu e nenhum teste segurava: um `DefaultBodyLimit` mais
  largo posto no router por outro motivo alargaria os quatro webhooks junto. Agora
  o teto e `garraia_channels::webhook_body::MAX_WEBHOOK_BODY_BYTES` (o mesmo valor,
  entao nada muda no que e aceito), aplicado duas vezes: na rota, onde impede o
  buffer de crescer, e no handler, antes de o `serde_json` ser chamado (413 acima
  do teto). A ordem nao muda: sem assinatura ou token valido a resposta continua
  sendo o 403/401 uniforme, qualquer que seja o tamanho. Fecha os quatro alertas
  Critical `rust/unsafe-deserialization` do CodeQL (#198-#201) sem dispensar
  nenhum: o validador e declarado como barreira num pack de modelos em
  `.github/codeql/extensions/`, e a regra segue alertando quem parsear corpo
  remoto sem passar por ele.
