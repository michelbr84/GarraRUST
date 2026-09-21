- **O REPL do `garra chat` ganhou editor de linha: setas, historico e edicao
  (#1297).** Ate aqui o loop lia com `read_line` em modo canonico, e quem
  editava a linha era o driver do terminal — seta para cima imprimia `^[[A`,
  nao havia historico nem edicao no meio da linha. Agora o prompt e um
  `rustyline` (MIT): setas navegam o historico, Home/End/Ctrl+A/Ctrl+E/Ctrl+W
  editam, Ctrl+R busca. O historico e persistente em `garraia_dir()/history`
  (nunca `~/.garra`), criado com `0600` no Unix porque o que se digita num
  chat pode ser sensivel; linha comecando com espaco fica de fora, como no
  shell. Falha ao carregar ou gravar e fail-soft — vira aviso e a sessao
  segue sem historico.
  O editor so entra quando stdin, stdout **e** stderr sao terminal. `echo
  pergunta | garra chat`, CI e os testes de integracao continuam no
  `read_line` byte a byte, identico ao de antes; `NO_COLOR` e `TERM=dumb`
  tiram a cor, nao as setas.
  O dono do SIGINT continua sendo um so, o vigia do `run_chat` — o rustyline
  entra sem a feature `signal-hook`. Em raw mode o Ctrl+C no prompt ocioso
  chega como leitura interrompida em vez de sinal, e o REPL encerra com o
  mesmo 130 de sempre; durante o turno o terminal ja esta em modo canonico e o
  Ctrl+C segue cancelando o turno. Ctrl+D encerra como `/exit`. Um `kill -INT`
  externo recebido com o editor em raw mode devolve os atributos do terminal
  antes de sair, para o shell nao ficar sem eco.
