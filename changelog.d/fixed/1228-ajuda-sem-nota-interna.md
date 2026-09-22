- **`garraia whatsapp --help` e `garraia memory add --help` param de exibir
  notas de implementacao (#1228).** O clap publica o doc comment inteiro no
  `--help`, e os dois comandos mostravam ao usuario a justificativa interna
  (o `#[command(name)]`, o nome do teste que o prende, o historico do
  `memory add`). As notas viraram comentario comum, a ajuda do `whatsapp`
  passa a citar `garraia init` em vez do alias, e um teste renderiza a ajuda
  longa de todos os comandos e falha se uma nota assim voltar.
