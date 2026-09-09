- canais: o IRC passa a ser registrado quando ha uma secao
  `[channels.<nome>]` com `channel_type = "irc"` e ao menos uma sala. A porta
  default passa a seguir o `use_tls` (6697 com, 6667 sem) em vez de 6667
  fixo, que apontaria TLS para a porta em claro. `garra config check` ganha
  avisos para servidor ausente, lista de salas vazia, e `use_tls` com a porta
  6667 — nenhum deles alcancado pela checagem generica, que procura token, e
  o IRC nao tem token. (#1050)
