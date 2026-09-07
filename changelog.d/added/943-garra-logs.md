- **`garra logs` e `garra logs --follow` (#943).** Le o arquivo canonico
  (`garraia.log`, no diretorio de config) direto do disco: **nao fala com o
  gateway** e nao pede que ele esteja rodando — que e justamente quando o log
  importa. `-n/--lines` escolhe quantas linhas do fim (100 por padrao) e
  `--path` imprime so o caminho, para encadear com outro comando.
- **A redacao continua sendo da escrita, e o comando nao a repete.** O
  `RedactingMakeWriter` envolve o appender, entao o que esta no disco ja esta
  redigido. Redigir de novo na leitura mascararia um vazamento em vez de
  conserta-lo: quem abrisse o arquivo no `less` veria o segredo do mesmo jeito,
  e nos acharíamos que estamos protegidos. Ha teste afirmando que o comando
  devolve o byte que leu.
- **`Ctrl+C` no `--follow` sai limpo, com codigo 130** — a convencao do shell,
  a mesma que o `garra chat` ocioso ja seguia, e a mesma do `tail -f` e do
  `journalctl -f`.
- **Log ausente nao e erro.** E o estado normal de quem nunca rodou nada, e a
  mensagem diz onde o arquivo estaria e o que o cria.
- **`/logs` no chat diz onde o log esta, e nao o despeja na conversa.**
  Misturar centenas de linhas de log com a conversa e o oposto do que a Fase 2
  deste epico foi fazer, e seguir o arquivo em tempo real disputaria a mesma
  tela com o streaming da resposta.
- **Sem `--level`, com o motivo escrito.** Uma entrada de log ocupa varias
  linhas quando carrega backtrace, e filtrar linha a linha partiria a entrada
  ao meio — sobraria a primeira linha do erro sem o rastro que a explica. Quem
  quer menos ruido tem o `RUST_LOG`, que decide na escrita.
