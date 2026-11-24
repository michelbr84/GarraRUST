- **`git_diff` passa a dizer o intervalo exato que o `diff` comparou (#1909).**
  O `status` ja nomeava a revisao do checkout, mas o `diff` devolvia as mudancas
  sem dizer entre o que e o que: `main..feature` e nome movel, e o mesmo pedido
  feito duas vezes pode falar de commits diferentes. A resposta agora abre com
  `Intervalo: <de> (<SHA completo>) .. <ate> (<SHA completo>)`; sem commits no
  pedido, diz que a comparacao e indice contra arvore de trabalho e em qual
  `HEAD`. Ponta que o git nao resolve sai como `revisao nao resolvida`, nunca
  some. So leitura (`git rev-parse --verify`, com o mesmo endurecimento do
  resto da tool); nenhuma raiz, jail ou modo mudou.
