- **WhatsApp pessoal: o `link` passa a perguntar quem pode falar com o GarraIA, e autorizar ou revogar vale sem reiniciar (#1345).**
  Depois do QR, o `garraia whatsapp link` deixava `allow` e `owners` vazios e
  dizia "pronto para receber mensagens": o portao do gateway (fail-closed, o
  que continua certo) descartava toda mensagem em silencio. Agora o `link`
  pede o numero autorizado com o codigo do pais (normalizado como o gateway
  compara; so os 4 ultimos digitos na tela; aviso quando e o proprio celular
  vinculado, cujas mensagens sao ignoradas) e so diz "pronto" com alguem
  autorizado. O novo `garraia whatsapp allow <numero> [--owner] [--yes]`
  autoriza sem terminal, preservando o resto da config e sem ligar o canal
  (exit 65 para numero invalido, 64 para `--owner` fora de `isolated-pod` ou
  num pipe sem `--yes`); dono so e oferecido em `isolated-pod`, com default
  nao. O gateway rele `allow`/`owners`/`enabled` da config viva a cada
  mensagem: entrar, sair e perder o piso de dono valem na mensagem seguinte,
  e `enabled: false` recusa todo mundo. `status`, `/api/diagnostics`
  (`whatsapp.linked` vira `warning`) e o log de boot avisam quando ninguem
  esta autorizado. Estranhos continuam recusados em silencio e nao ha
  auto-claim.
