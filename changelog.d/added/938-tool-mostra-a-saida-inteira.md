- **`/tool` mostra a saida inteira de uma chamada de ferramenta (#938).** O
  #937 reduziu cada chamada a uma linha, o que deixou a conversa legivel — mas
  resumo bom e resumo que esconde coisa, e esconder sem dar como reaver e so
  perder: quando o `cargo test` diz "Failed" e o resumo mostra a primeira
  linha, a causa costuma estar na linha 300. Agora cada linha de ferramenta
  termina com um numero (`#7`) e `/tool 7` mostra a saida completa; `/tool`
  sozinho lista o que esta guardado. O numero e curto de proposito porque
  aparece em toda linha, e ele existe porque sem ele o `/tool <n>` do `/help`
  seria instrucao sem como ser seguida.
- **Indices nao sao reusados.** Numerar por posicao faria `/tool 3` significar
  coisas diferentes conforme outras chamadas acontecessem — o usuario le "3" na
  tela, roda mais um comando e recebe outra saida sem nada indicando a troca.
  Sao monotonicos, e pedir uma entrada ja descartada diz que ela expirou em vez
  de mostrar a errada. "Expirou" e "nunca existiu" sao mensagens diferentes: a
  primeira e acionavel (rode de novo), a segunda quer dizer que o numero esta
  errado.
- **Dois limites, nao um.** O registro guarda as 20 ultimas chamadas E no
  maximo 512 KiB. So o teto de entradas nao seguraria memoria (vinte saidas de
  64 KiB sao 1,2 MiB grudados no processo); so o teto de bytes tornaria o
  `/tool` imprevisivel, porque uma saida gorda expulsaria dez pequenas e o
  indice recem-visto sumiria. Cada saida e capada em 64 KiB **na origem**, no
  `garraia-agents`, preservando comeco e fim com marcador do que sumiu — so o
  fim perderia o que estava sendo compilado, so o comeco perderia a causa da
  falha.
- **A saida completa passa pelas mesmas garantias do resumo**: redacao de
  segredo e remocao de controle de terminal (#995), na origem. Era criterio de
  aceite explicito da issue e daria para errar, porque o resumo ja era seguro e
  dava para achar que a saida crua tambem era. A diferenca e so de forma —
  quebra de linha e tabulacao sobrevivem, porque sao a estrutura do texto que o
  usuario pediu para ler; o `\r` nao sobrevive, porque sozinho ele devolve o
  cursor ao inicio da linha e e a primitiva de sobrescrever texto ja impresso —
  mas na saida completa ele vira **quebra de linha** em vez de sumir, senao uma
  barra de progresso (`10%\r20%\r100%`) viraria `10%20%100%`, um amontoado que
  o leitor nao distingue de uma saida que era assim mesmo. Idem backspace,
  tabulacao vertical e form feed, que viram marcador visivel.
