- canais: o Signal passa a ser registrado de verdade quando ha uma secao
  `[channels.<nome>]` com `channel_type = "signal"`. O `SignalChannel` tinha
  `impl Channel`, polling do daemon signal-cli e o guard `vet_signal_cli_url`
  desde antes, e nenhum call-site. Como o daemon e local e sobe por fora,
  falha de conexao no boot segue o retry com backoff do Telegram em vez de
  ser terminal. `garra config check` ganha aviso proprio: o Signal nao tem
  token, entao a checagem generica nao o alcancava e um canal pela metade
  saia sem um unico achado. (#1050)
