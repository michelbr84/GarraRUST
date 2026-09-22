- **WhatsApp pessoal: o `link` passa a perguntar quem pode falar com o GarraIA, e autorizar ou revogar vale sem reiniciar (#1345).**
  Depois do QR, o `garraia whatsapp link` deixava `allow` e `owners` vazios e
  dizia "pronto para receber mensagens": o portao do gateway (fail-closed, o
  que continua certo) descartava toda mensagem em silencio. Agora o `link`
  pede o numero autorizado com o codigo do pais (normalizado como o gateway
  compara; so os 4 ultimos digitos na tela; aviso quando e o proprio celular
  vinculado, cujas mensagens sao ignoradas) e so diz "pronto" com alguem
  autorizado. O novo `garraia whatsapp allow <numero> [--owner] [--yes]`
  autoriza sem terminal, sem mudar outro valor da config (o arquivo e
  reescrito: comentarios nao ficam) e sem ligar o canal (exit 65 para numero
  invalido, 64 para `--owner` fora de `isolated-pod` ou num pipe sem
  `--yes`); dono so e oferecido em `isolated-pod`, com default nao. O numero
  exige `+` e codigo do pais (6 a 15 digitos, a faixa da ponte), e
  `<id>@lid` tambem e aceito. O gateway rele `allow`/`owners`/`enabled` da
  config viva a cada mensagem quando vigia o `config.yml`: entrar, sair e
  perder o piso de dono valem na mensagem seguinte, e `enabled: false`
  recusa todo mundo, codigo de pareamento incluso. Quem pareou por codigo e
  estava no `allow` perde os dois ao sair da lista; quem so pareou segue ate
  o restart, e um `config.yml` que nao parseia mantem a lista anterior.
  Celular brasileiro com e sem o nono digito casa como o mesmo numero. A
  ponte passa a entregar o numero de remetente `@lid` quando o servidor o
  manda (`remoteJidAlt`/`participantAlt` do Baileys 7); sem ele o portao
  compara o LID, e `status` e `/api/diagnostics` contam essas recusas.
  `status`, `/api/diagnostics` (`whatsapp.linked` vira `warning`, tambem com
  a ponte conectada e o canal desligado na config viva) e o log de boot
  avisam quando ninguem esta autorizado. Estranhos continuam recusados em
  silencio e nao ha auto-claim.
