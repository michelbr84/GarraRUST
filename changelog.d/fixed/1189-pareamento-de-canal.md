- **`/pair` volta a funcionar — em todos os 11 canais, nao so no Telegram
  (#1189).** O comando gerava o codigo de 6 digitos em `state.pairing`, mas
  cada bootstrap de canal montava um `PairingManager` proprio, e era nessa
  instancia local que o handler de mensagens chamava `claim()`. A tabela
  local estava sempre vazia: o `claim()` devolvia `None`, o usuario caia em
  `telegram: unauthorized user ...` e a mensagem era descartada, mesmo com o
  codigo enviado dentro da janela de 5 minutos. O mesmo copy-paste duplicava
  a `Allowlist`: duas instancias liam o MESMO `allowlist.json` e ambas
  gravavam nele em `add()`/`claim_owner()`, entao uma sobrescrevia o owner ou
  o usuario recem-pareado da outra — perda de escrita silenciosa em disco.
  Agora os dois gates saem de um unico lugar, `channel_gates(state)`, que
  entrega as instancias do `AppState` compartilhadas por `/pair`, pelo
  handler de mensagens e pelo handler de voz. Telegram, Slack, Discord,
  WhatsApp, Signal, Matrix, IRC, LINE, Teams, Google Chat e iMessage estavam
  afetados pela duplicacao da allowlist; o `/pair` sem efeito era o do
  Telegram (o unico que passa pelo `commands.rs`) — no Discord, cujo `/pair`
  local gerava e resgatava na mesma instancia, o pareamento ja funcionava, e
  os outros nove canais nunca puderam emitir `/pair`. Mudanca de
  comportamento a registrar: com um `PairingManager` so por processo, um
  codigo gerado pelo `/pair` passa a ser resgatavel em **qualquer** canal
  habilitado (coerente com a allowlist global, que sempre foi um
  `allowlist.json` so); a protecao contra forca bruta no `claim()` e o aviso
  de queima ao dono estao no #1191.
