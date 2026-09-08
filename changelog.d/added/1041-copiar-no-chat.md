- **Copiar mensagem, codigo, memoria e log com um toque (#1041).** Relato de
  campo da v0.4.0: nao havia como copiar uma mensagem inteira — o texto do
  assistente era selecionavel por long-press sem nenhuma dica, e a bolha do
  usuario era `Text` puro. Toda bolha ganha um botao de copiar ao lado do
  horario e a do usuario vira selecionavel; bloco de codigo cercado ganha o
  proprio botao, que copia so o codigo; tocar numa memoria abre o texto completo
  com botao Copiar; o log da Activity tambem copia. Um so helper
  (`copyToClipboard`) para o app inteiro, com testes que interceptam
  `Clipboard.setData`.
