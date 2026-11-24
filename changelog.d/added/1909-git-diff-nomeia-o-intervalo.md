- **`git_diff` passa a dizer o intervalo exato que o `diff` comparou (#1909).**
  O `status` ja nomeava a revisao do checkout, mas o `diff` devolvia as mudancas
  sem dizer entre o que e o que: `main..feature` e nome movel, e o mesmo pedido
  feito duas vezes pode falar de commits diferentes. A resposta agora abre com
  `Intervalo: <de> (<SHA completo>) .. <ate> (<SHA completo>)`; sem commits no
  pedido, diz que a comparacao e indice contra arvore de trabalho e em qual
  `HEAD`. As pontas sao resolvidas antes e o diff roda sobre os SHAs, entao a
  linha nomeia exatamente o que foi comparado. Muda um comportamento: com uma
  ponta que nao e commit do repositorio (nome errado, vazia, `a..b` dentro de
  uma ponta), o diff nao roda e a recusa diz qual ponta faltou — antes o texto
  de erro do git voltava cru, e uma ponta vazia devolvia diff contra o `HEAD`.
  So leitura (`git rev-parse --verify`, com o mesmo endurecimento do resto da
  tool); nenhuma raiz, jail ou modo mudou.
