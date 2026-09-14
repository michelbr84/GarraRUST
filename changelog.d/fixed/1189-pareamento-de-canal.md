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
  todos afetados pelo mesmo defeito.
