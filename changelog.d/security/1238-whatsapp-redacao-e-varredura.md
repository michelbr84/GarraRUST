- **A redacao do stderr da ponte deixava metade de uma credencial passar
  (#1238).** Ela varria sequencias de `[A-Za-z0-9+=]`, com a barra de fora
  para nao redigir caminhos de arquivo. So que a barra faz parte do alfabeto
  base64 padrao e aparece a cada ~64 caracteres, entao ela quebrava a propria
  chave em pedacos curtos demais para serem cortados. Medido sobre 2000 chaves
  de 32 B: o maior pedaco em claro tinha 17 dos 44 caracteres em media, e em
  35% dos casos metade ou mais da chave sobrevivia; o teto de 240 caracteres
  por linha nao ajudava, porque 240 caracteres tambem sao 240 caracteres de
  credencial. A sequencia agora inclui `/`, `.`, `@`, `-` e `_`, e so e
  poupada quando tem `.` ou `@` — extensao de arquivo ou escopo de pacote
  npm, que nenhum alfabeto base64 produz. Com isso o vazamento cai para ruido
  (~1,3 caractere) tanto no base64 padrao quanto no url-safe, em quatro
  contextos diferentes, e os caminhos que a cauda existe para mostrar
  continuam legiveis. Isentar tambem `-` e `_`, que caminho real tem, seria a
  escolha obvia e e a errada: medido, `npm ERR! _auth=<chave>` volta a vazar
  os 44 de 44 caracteres em 100% dos casos, porque o `_` do nome do campo
  entra na mesma sequencia e isenta a chave junto. Isentar `/` "junto de"
  `-`/`_` tambem nao serve: a chave em base64 padrao traz a propria barra, e o
  vazamento inteiro volta em 48,8% dos casos. O preco aceito e um falso
  positivo conhecido — caminho longo sem ponto e sem `@` vira `<redigido>`.
- **A redacao passou a ser exercitada nos dois lugares em que ela e aplicada
  (#1238).** Ela tinha teste como funcao pura e nenhum nos dois call sites:
  neutralizar os dois — devolver a linha crua em vez da redigida — desfazia a
  correcao inteira com a suite toda verde. Agora ha um teste de ponta a ponta
  para cada um, com processo de verdade cuspindo material de credencial no
  stderr.
- **A varredura do proprio fonte ficava cega a partir de um `#[cfg(test)]`
  sobre item sem chave (#1238).** Ela ligava o modo "apaga" ao ver o atributo
  e so o desligava na primeira linha com `}`. Sobre `mod tests;`, `use …;`,
  `const …;` ou `static …;` — item que fecha em `;` e nunca abre bloco — o
  modo seguia apagando **producao** ate topar com uma chave de fechamento
  qualquer, la adiante. As tres varreduras do modulo leem esse mesmo texto,
  entao ficavam cegas juntas e a comparacao entre elas continuava batendo:
  verde com bug. `mod.rs` ja tem um `#[cfg(test)] mod source_scan;` na
  arvore — o dano era zero so porque ele esta na ultima linha do arquivo, e
  move-lo para o topo cegaria o arquivo inteiro sem nenhum teste piscar. A
  primeira correcao cobriu so o atributo em linha propria; com atributo e item
  na MESMA linha (`#[cfg(test)] use std::fmt;`) o furo continuava aberto, e
  aninhado dentro de um `mod` ele escapava tambem da guarda que olhava so a
  coluna 0. Agora o cortador trata o resto da linha do atributo, e a guarda
  alcanca item de producao em qualquer profundidade — por indentacao, e nao
  contando chaves, para nao errar junto com o que ela vigia.
- **Um comentario no FIM da linha do `#[cfg(test)]` ainda cegava a varredura
  (#1238).** A correcao anterior tratava comentario no COMECO da linha e nao
  no fim: em `#[cfg(test)] use std::fmt; // so no teste` o texto termina em
  `teste` e nao em `;`, entao o item ficava pendente, a linha seguinte era
  julgada como se fosse ele, e a primeira que abrisse chave ligava o modo
  "apaga" sobre producao. Medido com as funcoes reais, as duas varreduras
  voltavam zero achado sobre um `expose()` plantado. A guarda que deveria ser
  a segunda opiniao ficava MUDA quando o item apagado comecava com
  `pub(crate)`, comum nesta crate: a lista de prefixos tinha `pub ` e nao
  `pub(`. Agora o comentario de fim de linha e cortado respeitando literais —
  contar aspas, a solucao obvia, quebra tanto `"http://x"` quanto
  `const S: &str = "a // b";` — e a lista de prefixos ganhou `pub(`, `mod`,
  `use` e `unsafe`, de modo que qualquer falha residual apareca alto. A guarda
  passou a examinar 293 itens de producao contra 258. O buraco era latente: a
  arvore nao tinha nenhum caso.
- **A regra invertida do `expose()` deixou de poder sumir em silencio
  (#1238).** Numa arvore sem violacao, "zero achados" nao distingue regra viva
  de regra ausente, e arrancar a allowlist inteira deixava o teste verde. O
  caso negativo virou texto plantado dentro do proprio teste — incluindo o
  `let s = blob.expose(); let t = s;` que motiva a inversao existir. E
  `SessionBlob::expose()` virou `pub(crate)`: o compilador passa a impedir de
  graca o que a allowlist textual impedia com esforco.
