- **`garraia start -d` deixa de apagar o `garraia.log` a cada start e de
  rasgar a cabeca dele (#1371).** O daemon abria o log com `File::create`,
  que trunca, e sem `O_APPEND`. Com isso cada start do daemon apagava o log
  da execucao anterior, justo o que se quer ler ao reiniciar depois de uma
  queda. E esse descritor vira stdout e stderr do daemon pelo `dup2`, entao
  tudo que escrevia cru nele (a mensagem de um panic, um `eprintln!`, um
  filho com o stderr herdado) caia no offset proprio do descritor, que
  comeca em 0, por cima das linhas que o `tracing` ja tinha gravado: o smoke
  de instalacao limpa da v0.4.4 achou "Secure MCP Filesystem Server running
  on stdio" na primeira linha, seguido de meia linha de tracing. O log agora
  abre em append: cada escrita vai para o fim do arquivo, atomicamente, e as
  execucoes anteriores ficam. O arquivo cresce sem rotacao, como ja crescia
  no `garraia start` em foreground, que sempre abriu o mesmo arquivo em
  append.
