- **Aprovacao de comando so vale para o pedido que pausou o turno (#1339).**
  A deteccao do "ok" do humano (GAR-187) aceitava o marcador
  `[CONFIRM_REQUIRED:...]` de qualquer resultado de tool recente, e dentro de
  uma mesma mensagem lia os resultados do mais antigo para o mais novo.
  Uma tool comum que devolvesse a copia de um marcador verdadeiro (pagina
  lida pelo `web_fetch`, arquivo, resultado MCP) podia vencer o pedido de
  verdade, e o "ok" cobria o comando errado. Marcador forjado nunca
  autorizou nada (a impressao digital e HMAC com chave por processo), mas a
  copia de um verdadeiro autorizaria. Agora toda saida que nao e pedido de
  confirmacao tem o prefixo do marcador neutralizado no ponto unico de
  despacho, antes de entrar no historico, e dentro de uma mensagem o
  resultado mais novo ganha. Tres testes, provados por mutacao.
