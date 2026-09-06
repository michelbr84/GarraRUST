- **Saida de ferramenta nao injeta mais comando no terminal (#995).** O resumo
  do #937 redigia segredo e trocava quebra de linha por espaco, mas deixava
  passar `ESC` e os demais controles — e saida de ferramenta nao e conteudo
  confiavel: e o que o agente leu de um arquivo, baixou de uma pagina ou o que
  um comando escreveu. Um `README` de repositorio clonado conseguia limpar a
  tela de quem roda o `garra chat` com `\x1b[2J`, trocar o titulo da janela com
  OSC, ou reposicionar o cursor para sobrescrever linhas ja impressas, forjando
  texto que parece ter vindo do proprio Garra. O caso mais pontudo era
  `\x1b[?25l`: o projeto tem invariante explicita de que essa sequencia nao e
  emitida em lugar nenhum, para que nenhum caminho de saida deixe o terminal
  sem cursor — e saida de ferramenta a violava. Agora todo controle C0, C1 e
  DEL sai, no `garraia-agents`, junto da redacao de segredo e antes do
  truncamento (truncar antes deixaria meia sequencia passar, pela mesma razao
  que ja valia para segredo). A seguranca vem da regra por caractere, nao do
  reconhecimento de sequencia: sem `ESC`, `[2J` e texto inerte. O
  reconhecimento de CSI e OSC que existe serve so para legibilidade, porque
  saida colorida e comum e legitima e trocar so o `ESC` por marcador deixaria
  ruido na tela do caso normal. Vale para o resumo do input tambem — o
  `command` do bash tambem vem de fora.
