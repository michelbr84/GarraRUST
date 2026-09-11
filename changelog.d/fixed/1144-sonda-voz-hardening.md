- **#1144: o hardening da sonda de voz que o #1098 ja prometia.** A revisao
  do PR #1115 (que entrou por auto-merge armado antes do veredito) achou
  tres lacunas no `GET /api/diagnostics`; as tres fecham aqui. (1) O motivo
  de um veto SSRF sai de um match proprio, campo a campo, e nao do `Display`
  de `SsrfRejection`: `voice.tts`/`voice.stt` sao auth-free sem a chave do
  gateway, e a garantia de que nenhuma variante ecoe a URL crua — com
  `userinfo` embutida — passa a ser estrutural, pinnada por teste. (2) O
  `next_step` de um HTTP 5xx agora e "inspecione os logs do processo", nao o
  comando de subida: o servidor esta de pe, e a `docs/voice.md` e o changelog
  do #1098 ja prometiam esse split. (3) A sonda e single-flight: o lock do
  cache e seguro atravessando a sonda inteira, entao N requests simultaneos
  numa janela de cache frio disparam um par de dials, nao N pares — o
  amplificador que o TTL existe para impedir deixava de valer pela porta dos
  fundos.
