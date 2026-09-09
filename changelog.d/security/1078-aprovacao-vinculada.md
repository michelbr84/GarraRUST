- A aprovacao humana de um comando de risco passou a valer para O COMANDO
  aprovado, e nao para o turno inteiro. Antes o `ok` do usuario ligava um
  booleano que qualquer tool call daquele turno consumia: o modelo pedia
  confirmacao para um `ls -la`, recebia o `ok`, e executava outra coisa em
  seguida. Cada pedido agora carrega a impressao digital de
  `(ferramenta, assunto)` e a ferramenta so honra a aprovacao que bate com o
  que ela esta prestes a fazer (#1078).
- O marcador de pedido de confirmacao deixou de ser aceito quando vem do
  TEXTO do modelo. Um modelo com saida nao sanitizada escrevia o marcador na
  propria narracao, plantava um pedido que nunca existiu e colhia o `ok`
  inocente do usuario na mensagem seguinte. So resultado de ferramenta cria
  pedido, porque e a ferramenta que o emite (#1078).
- A impressao digital do pedido de confirmacao passou a ser um HMAC com chave
  aleatoria por processo, e nao um hash de entradas publicas. Restringir o
  marcador a resultado de ferramenta fecha o texto do modelo, mas nao fecha o
  resultado de uma ferramenta que devolve conteudo de terceiro: uma pagina
  buscada pelo `web_fetch`, um arquivo lido pelo `file_read`, a resposta de um
  servidor MCP. Com hash simples o atacante pre-computava o marcador de um
  comando escolhido por ele, servia junto de uma injecao de prompt, e colhia o
  "ok" do usuario. Sem a chave, conteudo de terceiro nao cunha marcador que
  bata com comando nenhum (#1078).
