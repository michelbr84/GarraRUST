- **O canal pull `whatsapp_linked` no gateway (#1238, ADR 0023 slice S4d).**
  E a peca que faltava para a mensagem escaneada pelo QR chegar ao agente: ate
  aqui `garra whatsapp link` pareava e nada acontecia. O gateway supervisiona a
  ponte Node/Baileys em `mode: "serve"`, entrega cada `message` ao runtime e
  responde pelo comando `send`. Canal PULL sem registry e sem webhook — quem
  vive e um processo filho com loop de reconexao proprio —, entao o status sai
  do supervisor e nao do `ChannelRegistry`, pelo mesmo motivo que os canais
  push da #1079 precisaram de fonte propria.
  O modelo de ameaca deste canal e outro: a mensagem vem de qualquer pessoa que
  conheca o numero pessoal do operador. Tres decisoes saem disso, nenhuma
  configuravel para menos. (1) **Fail-closed de verdade**: allowlist vazia
  significa ninguem, e o modo aberto da allowlist — que existe para ergonomia
  local — nao vale aqui; nao ha auto-claim de dono como no canal Cloud, onde o
  primeiro remetente vira dono do bot. Entra-se com codigo do `/pair` ou pela
  lista `allow` da config. (2) **Ferramentas somente-leitura por padrao**:
  sessao sem modo escolhido resolve para o perfil `search`, e nao para "sem
  politica", ate o operador subir o nivel com `/mode`. (3) **Guard de injecao
  indireta no texto recebido**, aplicado localmente porque a #1243 (que propoe
  generaliza-lo para alem do `web_fetch`) nao mergeou.
  Grupo so responde com opt-in explicito, e mensagem propria (`from_me`) nunca
  gera turno — sem esse filtro o canal responderia as proprias respostas para
  sempre, ja que a conta vinculada e a do operador.
  `garra whatsapp status`, `GET /api/channels` e o check `whatsapp.linked` do
  `GET /api/diagnostics` classificam pela MESMA funcao
  (`whatsapp_linked::health::classify`), com proximo passo acionavel em cada
  modo de falha — "rode `garra whatsapp link`" sem sessao, "rode `npm ci` em
  <dir>" sem dependencias na ponte. Duas fontes divergentes sobre o mesmo canal
  e o defeito que a #1079 ja custou uma vez.
