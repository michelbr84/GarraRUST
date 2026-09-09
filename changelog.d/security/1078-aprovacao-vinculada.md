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
