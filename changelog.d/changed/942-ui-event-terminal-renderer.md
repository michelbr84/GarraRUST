- **A saida do terminal passa por um renderer, nao mais por `println!` espalhado
  (#942).** Ate aqui tres donos que nao se conheciam montavam a tela do
  `garra chat`: `println!` direto, o `stream_turn` e o `tracing` — e a separacao
  entre log e interface, que o #933 conquistou, era convencao, nao estrutura.
  Agora ha `UiEvent` (o que aconteceu) e `TerminalRenderer` (o que aparece),
  conforme a ADR 0017. A ordem obrigatoria de escrita — apagar a animacao,
  escrever o rotulo `Garra` uma unica vez, so entao o texto do modelo — saiu de
  uma macro dentro do `stream_turn` e virou responsabilidade do renderer. O
  `spinner` e o `conversation` viraram `ui::spinner` e `ui::conversation`, sem
  alteracao propria. Comportamento visivel identico, com uma excecao de
  proposito: num terminal interativo sem UTF-8 (`LANG=C`), a conversa e a
  animacao agora caem para ASCII **juntas**. Antes cada uma decidia sozinha e o
  usuario via `❯` em UTF-8 ao lado de uma animacao ASCII — o desencontro que a
  ADR 0017 manda acabar. `GARRAIA_NO_SPINNER` passou a desligar so a animacao,
  mantendo o resto da interface rica de pe. A largura do terminal tambem virou
  fonte unica: pergunta ao terminal primeiro, `COLUMNS` como segunda opiniao.
