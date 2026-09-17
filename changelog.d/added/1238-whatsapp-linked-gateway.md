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
  configuravel para menos. (1) **Fail-closed de verdade**: o canal tem
  **allowlist propria**, e nao a global da instalacao — entra-se com codigo do
  `/pair` ou pela lista `allow` da config, e portao vazio significa ninguem. A
  allowlist global nao serve aqui porque *todo* canal irmao a preenche sozinho
  por auto-claim do primeiro remetente, o que faria de um estranho que mandou
  "oi" para o numero Cloud um remetente admitido no numero pessoal do operador.
  Como consequencia, `allow` deixou de ser gravado no `allowlist.json` global:
  a config e a fonte de verdade, o que sai do `allow` sai do portao (antes nao
  havia revogacao por config), e um numero liberado aqui nao vira identidade
  valida em Telegram, Discord, Slack, Signal, Matrix ou no `/start`. Pareamento
  vale para a execucao corrente; acesso duravel se declara no `allow`.
  (2) **Ferramentas somente-leitura por padrao**: sessao sem modo escolhido
  resolve para o perfil `search`, e nao para "sem politica", ate o operador
  subir o nivel com `/mode`. Enquanto a #1264 estiver aberta, esse piso **nao
  cobre ferramenta de servidor MCP** (o `ToolGate` deixa passar qualquer
  `servidor__tool` pelo whitelist), entao o canal se recusa a subir se houver
  uma registrada no boot e recusa cada turno cuja entrada encontre uma
  registrada. Nao e invariante continua: entre um turno e o seguinte o
  inventario pode mudar (o monitor de saude do MCP re-sincroniza a cada 30 s),
  e o que o controle entrega e a reducao da janela de minutos para o intervalo
  entre dois turnos. O detector pergunta o MESMO que a escapatoria do
  `ToolGate` pergunta — origem MCP **ou** `__` no nome —, e nao so a origem,
  para que uma ferramenta nativa de nome com `__` nao fure o whitelist sem
  acender nada. (3) **Guard de injecao
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
  E o desligamento do gateway **cancela o supervisor**: o `cancelar()` existia,
  estava correto e nao tinha chamador de producao, entao depois do Ctrl+C o
  processo imprimia "shut down gracefully" com a ponte Node viva e turnos de
  agente rodando. A rota "o `Sender` cai junto com o `AppState`" nao existe
  aqui, por um ciclo de `Arc` (o sink do canal detem o estado que detem o
  cancelamento), e por isso o cancelamento e explicito.
