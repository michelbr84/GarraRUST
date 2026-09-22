- **`garraia status | head` deixa de terminar em panico (#1371).** O
  runtime do Rust ignora SIGPIPE, entao um `println!` num pipe cujo leitor
  ja fechou recebia `EPIPE` e o processo morria com "failed printing to
  stdout: Broken pipe" e exit 101 (ou, no `garraia logs`, com "Error: Broken
  pipe" e exit 1). Nos comandos que so leem e imprimem, o SIGPIPE volta ao
  padrao do Unix antes da primeira escrita: o comando sai em silencio no
  primeiro `write` sem leitor, como `ls | head`, e o shell ve 141. Entram
  `status`, `about`, `logs` (inclusive `--follow`), `doctor`, `runs list`,
  `config check`, `memory stats|list|search`, `mcp list`, `channel list|status`,
  `skill list`, `glob`, `whatsapp status`, `ask` e
  `desktop --status|--no-launch`. O gateway (`start`, `restart`, o daemon),
  o `mcp-server` e o REPL do `chat` ficam com o sinal ignorado, porque la um
  leitor que some tem de virar erro tratado e nunca a morte do processo, e o
  mesmo vale para os comandos que mudam estado. A decisao e um `match`
  exaustivo ate o subcomando aninhado, sem curinga: um subcomando novo, de
  primeiro nivel ou aninhado (um `memory` ou `config` novo, por exemplo), nao
  compila sem alguem decidir de que lado ele fica, e um teste varre o corpo
  da funcao para que ninguem troque isso por `matches!` ou `_ =>`. No
  Windows nada muda.
