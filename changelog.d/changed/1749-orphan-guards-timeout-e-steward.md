- **Job `orphan-processes` do CI ganha `timeout-minutes` explicito (45) e o `steward` reconhece o leg macOS cancelado sem passos (#1749).**
  Sem o teto valia o default de 360 min: um leg travado depois de comecar so caia
  pela plataforma, sem motivo no log, e um hang no macOS custava minutos 10x ate la.
  O valor e 2x o pior caso medido em 100 runs (22,4 min no leg Windows com cache
  frio); estourado, o leg falha com motivo claro. O teto nao cobre a fila: um leg
  `macos-latest` que nunca recebe runner continua terminando `cancelled` sem passos
  nem log, e a skill `steward` passa a classificar essa assinatura como ambiental
  quando os legs irmaos passam, o mesmo job esta verde em `main` e nao houve push
  novo cancelando a run, com o remedio (re-executar so o leg depois que a run
  termina; o job nao e check obrigatorio). Manter ou mover o leg macOS para o
  workflow agendado segue como decisao do dono na #1749.
