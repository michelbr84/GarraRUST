- **`install.sh` deixa de tentar o wizard sem terminal em container e CI.**
  O teste era `[ -r /dev/tty ]`, que so le os bits de permissao; o `/dev/tty`
  e `crw-rw-rw-` em qualquer Linux, inclusive num container ou runner sem
  terminal de controle, onde abrir o dispositivo falha com ENXIO. O smoke de
  instalacao limpa da v0.4.4 imprimia `main: line 631: /dev/tty: No such
  device or address` e depois "Wizard exited non-zero" para um wizard que
  nem chegou a rodar. O `has_usable_tty` agora abre o `/dev/tty` de verdade
  (so leitura, dentro de um subshell, com `-c` exigindo dispositivo de
  caractere), e sem terminal o instalador segue o caminho nao interativo
  documentado sem linha de erro. Um `CI` nao vazio tambem conta como sem
  terminal, como o `Test-InteractiveSession` do `install.ps1` ja fazia
  (regra 16): um job de CI com pty nao tem ninguem para digitar. O
  `install.ps1` nao tinha a falha (`[Console]::IsInputRedirected` consulta
  `GetConsoleMode`, nao permissao); a suite dele ganhou os casos espelhados
  que rodam a sonda real em processos filhos.
