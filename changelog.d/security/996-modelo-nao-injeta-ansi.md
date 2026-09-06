- **O texto do modelo tambem para de injetar comando no terminal (#996).** O
  #995 fechou a superficie da ferramenta; esta e a irma. O `write_delta`
  escrevia o delta do modelo direto no terminal, entao um modelo induzido a
  emitir `\x1b[2J` limpava a tela de quem estava conversando, e `\x1b[?25l`
  deixava o terminal sem cursor — a mesma invariante do CLAUDE.md violada por
  outro caminho. Vale tambem para aviso, erro e dica, que carregam texto de
  fora (corpo de erro de provedor, por exemplo).
- **O filtro tem estado, e e isso que o distingue do #995.** Aquele recebe a
  saida da ferramenta inteira e decide olhando o texto todo. Este nao: o texto
  do modelo e streaming, e `\x1b` pode chegar num delta e `[2J` no seguinte —
  cada metade inofensiva isolada, e um filtro sem estado deixaria as duas
  passarem, com o terminal executando a concatenacao. Ha teste que varre
  **todos** os cortes possiveis de uma carga hostil, nao so um escolhido a
  dedo. O `finish` do turno limpa o pendente, senao um `ESC` no fim de uma
  resposta engoliria o primeiro caractere da proxima.
- **Cor do modelo fica bloqueada, e nao por conservadorismo.** A issue deixava
  em aberto se o modelo devia poder emitir cor de proposito, como algumas CLIs
  permitem. A resposta sai de um principio que o projeto ja tem escrito:
  respeitar `NO_COLOR`, non-TTY e saida redirecionada. Cor vinda do modelo
  passa por cima disso — ela nao sabe se o usuario pediu `NO_COLOR`, se a saida
  vai para um pipe, ou se o terminal e legado. Quem decide cor e o
  `TerminalRenderer`, olhando `Capabilities`; o modelo escreve texto.
- **Teto para sequencia sem terminador.** Um `ESC ]` solto engoliria a resposta
  inteira em silencio, porque OSC so termina em `BEL` ou `ESC \`. Com teto de
  128 caracteres, o pior caso e perder um trecho curto.
