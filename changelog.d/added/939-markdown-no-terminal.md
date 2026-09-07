- **A resposta do modelo passa a sair formatada no terminal (#939).** Titulo,
  negrito, enfase, codigo inline, bloco cercado, lista com marcador ou numero,
  citacao, link e regra horizontal saem renderizados em vez de com a sintaxe
  crua na tela.
- **Sem bufferizar por linha, que era a solucao obvia e errada.** Renderizar a
  linha inteira quando ela fecha da resultado perfeito e zero cintilacao — e
  para a prosa: o modelo costuma emitir um paragrafo inteiro como uma unica
  linha, entao o usuario ficaria olhando para o nada ate o ponto final.
  Streaming que nao aparece nao e streaming. O renderizador segura so o que
  ainda pode mudar de significado: poucos caracteres no inicio da linha para
  decidir o tipo de bloco, e depois a cauda a partir do ultimo delimitador
  inline ainda sem par. O atraso maximo e de uma palavra.
- **Um construto partido entre dois deltas continua sendo um construto.** E a
  mesma classe de problema do filtro ANSI (#996) e tem a mesma forma de
  solucao — estado que atravessa as chamadas. O teste que importa renderiza
  cada exemplo em **todo** tamanho de pedaco possivel, e nao num corte
  escolhido a dedo: o corte que o autor imagina e justamente o que nao quebra.
- **A prosa quebra na largura do terminal, e a continuacao recebe o recuo do
  bloco.** E esse alinhamento que justifica quebrar: o terminal ja quebra
  sozinho no limite direito, mas sempre na coluna zero, e a segunda linha de um
  item de lista passava a parecer um item novo. Quem mede e
  `console::measure_text_width`, que ignora escape e conta largura visual —
  contar bytes erraria com acento e com ideograma.
- **Bloco cercado nao ganha estilo inline nem quebra.** O criterio de aceite
  pede que codigo continue facil de copiar: um `*` no meio de um programa e um
  `*`, e um `\n` que o modelo nao escreveu vira um `\n` que o usuario cola.
  Linha longa de codigo passa a decisao ao terminal. Pela mesma razao, um bloco
  unico maior que a linha inteira (URL longa, base64) sai sem quebra inventada.
- **Sem cor, nada disto acontece — nem um escape.** Saida redirecionada,
  `NO_COLOR` ou `TERM=dumb` devolvem o texto exatamente como veio, byte a byte,
  que e o que mantem `garra chat > arquivo` util para automacao. O `garra ask`
  fica de fora por contrato proprio ja escrito no codigo: ele nunca imprime
  ANSI no stdout.
- **Tres limites que a auditoria pediu, e um pânico que ela achou.** Acento
  como primeiro caractere de uma linha dentro de bloco cercado derrubava o CLI
  — o ramo que decide se a linha e a cerca de fechamento fatiava o primeiro
  **byte**, e num `é` esse byte nao e fronteira de caractere. Alem disso: o que
  fica retido esperando um delimitador fechar tem teto (um `**` sem par numa
  resposta sem quebra de linha segurava a tela e o buffer crescia junto), a
  linha e compactada enquanto sai (uma resposta de uma linha so ficava inteira
  em memoria), e o estilo de titulo ou citacao e fechado no fim do turno mesmo
  sem o `\n` final — antes um turno truncado deixava o proximo prompt em
  negrito.
