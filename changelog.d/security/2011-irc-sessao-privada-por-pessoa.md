- **IRC: cada conversa privada tem a propria sessao (#2011).** No privado o
  alvo do `PRIVMSG` recebido e o nick do bot, e a sessao era montada dele:
  as conversas privadas de todas as pessoas caiam juntas em
  `irc-<nick do bot>`, e o modelo recebia as mensagens de uma pessoa no turno
  de outra (com a allowlist em modo `open`, de qualquer nick da rede). Agora o
  canal decide canal ou privado num ponto so, pelo `CHANTYPES` que o servidor
  anuncia, e a resposta e a sessao seguem a mesma decisao: no canal a sessao
  continua `irc-<canal>`; no privado e `irc-@<nick>`, uma por pessoa, com o
  nick normalizado pelo `CASEMAPPING` anunciado (sem anuncio, `ascii`). A
  sessao antiga `irc-<nick do bot>` fica como esta e nao recebe turno novo. De
  quebra, uma mensagem num canal `&local` passa a ser respondida no canal, e
  nao no privado de quem falou.
