- **O turno que caiu no fallback nao-streaming nao era anotado.** O `/stats`
  (#984) e o `/status` novo respondiam "nenhum turno ainda" **depois de um
  turno inteiro**, sempre que o provider nao fazia streaming — Ollama antigo,
  llama.cpp sem SSE, ou qualquer provider num momento em que o streaming
  falha. Dos tres `return Ok` do `stream_turn_with_sink`, so um anotava.
  Achado rodando o binario: `Turnos 1` e `Ultimo turno: nenhum ainda` na mesma
  tela.
- **E o ramo do fallback agora anota melhor que o de streaming.** La ha uma
  `LlmResponse` de verdade, entao o modelo vem **confirmado pelo provider** e a
  contagem de tokens existe — o caminho de streaming so conhece o modelo
  *pedido*. O turno que pede confirmacao de ferramenta tambem passou a contar:
  um `/stats` em branco depois de uma pergunta diria que nada aconteceu.
- **A descricao de ferramenta MCP passa por redacao de segredo.** Ela e texto
  livre escrito pelo servidor, e o `/tools` a mostra. O painel ja tirava
  escape, mas escape nao e o unico problema: uma descricao mal escrita pode
  trazer uma URL com token, e ela iria para a tela em texto plano. E a mesma
  redacao que a **saida** da ferramenta ja recebia.
