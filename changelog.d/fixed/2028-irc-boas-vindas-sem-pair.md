- **A mensagem de boas-vindas ao dono no IRC deixa de mandar usar `/pair` (#2028).**
  O IRC nao trata comandos, e o `/pair` dos outros canais so atende o dono
  daquele canal, nao um nick de IRC: a instrucao nao tinha como funcionar. A
  mensagem agora aponta o caminho que funciona, acrescentar `"irc:<nick>"` em
  `users` do `allowlist.json` com o gateway parado.
