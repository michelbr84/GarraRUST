- **ADR 0017: camada de apresentacao do CLI (`UiEvent` + `TerminalRenderer`).**
  Formaliza a Fase 2 do epico #944 antes do codigo, como manda a regra absoluta
  8. Registra onde a camada mora (`garraia-cli`, nunca `garraia-agents`), o que
  o renderer possui e o que nao possui — o relogio continua vindo do
  `select!`, e `tracing` nao vira interface —, e quatro invariantes: o renderer
  e chamado de dentro do `select!` e nunca de uma task propria (e o que protege
  a drenagem do canal limitado que ja derrubou o `garra chat` uma vez), todo
  caminho de desenho aceita `impl io::Write` para ser afirmavel em teste, o
  cursor nunca e escondido, e nao-TTY nao emite escape algum. Rejeita
  explicitamente um TUI de tela cheia: ele quebraria scrollback e pipe, que sao
  o modo normal de usar uma CLI de conversa em fluxo.
