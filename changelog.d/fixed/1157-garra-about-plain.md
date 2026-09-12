- Corrige `garra about`, que escrevia ANSI incondicional: `about > arquivo`,
  pipe, `NO_COLOR` e `TERM=dumb` recebiam sequencias de escape cruas. A tela
  agora e renderizada por `about_text(style)` a partir do mesmo dono de
  decisao do #942 (`ui::Capabilities::detect()`) — plain recebe ASCII puro,
  sem uma unica sequencia de escape, afirmado em teste.
