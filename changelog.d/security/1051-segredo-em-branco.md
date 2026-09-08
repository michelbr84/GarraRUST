- **A verificacao de assinatura do LINE deixa de aceitar segredo so com
  espacos (#1051).** O construtor `LineChannel::new` recusa `channel_secret`
  em branco desde o PR #1057, mas `line_signature::verify_signature` media
  "vazio" com `is_empty()` em vez de `trim().is_empty()`. As duas discordavam,
  e a mais fraca era justamente a exportada: `verify_signature` e API publica
  reexportada, e `ChannelConfig.settings` e um mapa nao-tipado, entao um
  `channel_secret = "   "` no TOML passava pela config e a funcao validava
  contra essa chave — que qualquer um reproduz. O impacto hoje segue nulo (o
  canal LINE nao chega ao gateway, #1050), mas quem escrever o handler do
  webhook chamaria a funcao direto, e nesse dia o teatro seria real.
