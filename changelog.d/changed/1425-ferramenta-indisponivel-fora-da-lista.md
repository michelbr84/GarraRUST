- **Ferramenta registrada mas nao operacional fica fora da lista chamavel
  do modelo (#1425, opcao B; #1418).** `Tool::disponibilidade()` diz se a
  ferramenta esta utilizavel AGORA; o runtime consulta isso ao montar a
  lista de cada turno (junto com o portao de nome e classe) e antes de
  despachar: se o modelo pedir pelo nome uma ferramenta indisponivel, ela
  nao roda e a explicacao volta como resultado de ferramenta, com codigo
  (`not_configured`, `channel_offline`, ...) e motivo, distinta de "negada
  pela politica". `telegram_send` e a primeira a usar: so e chamavel com o
  Telegram configurado na config viva e conectado; desligar o canal na
  config tira a tool na mensagem seguinte, sem restart. `NoRoots` das file
  tools ganha mensagem propria e acionavel — "nenhuma raiz esta configurada
  para esta sessao ... selecione um projeto com `/project <nome>` ou
  configure `agent.file_roots`" — em vez da recusa generica de caminho
  fora das raizes (que continua identica para "fora" e "nao resolveu",
  sem oraculo de existencia).
