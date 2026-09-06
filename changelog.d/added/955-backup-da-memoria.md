- **`garra memory backup` — retrato consistente da memoria, com retencao (#955).**
  A memoria e o ativo que mais doi perder e vivia num arquivo so, sem copia. O
  comando escreve um retrato em `<data_dir>/backups/`, com nome ordenavel por
  data (`memory-20260906T054500123Z.db`, UTC com milissegundo).

  **`VACUUM INTO`, nao `cp`.** O banco roda em WAL: copiar o arquivo com `cp`
  pega uma foto sem as transacoes que ainda estao no `-wal` ao lado, e o
  resultado e um backup que parece bom e esta incompleto — o pior tipo. O
  `VACUUM INTO` le sob uma transacao e escreve o estado commitado inteiro, sem
  checkpoint e sem parar o gateway, e de brinde compacta.

  **O indice vetorial vai junto** — verificado, nao presumido: uma sonda contra
  um banco com `vec_embeddings_*` real confirmou que as tabelas vec0, as
  sombras delas e o `vec_id_map` chegam integros, e a copia reabre com o mesmo
  relatorio de integridade. Era o risco de verdade; o WAL, que a issue
  levantou, o `VACUUM INTO` ja resolve sozinho.

  `--keep-days N` apaga backups **nossos** mais velhos que N dias, depois de o
  novo existir. Duas garantias: so apaga arquivo que casa com o padrao que o
  proprio comando cria (backup manual com outro nome fica), e a idade vem do
  **nome**, nao do `mtime` — copiar o diretorio para outra maquina renova todo
  `mtime` e apagaria tudo na primeira execucao seguinte. Sem `--keep-days`,
  nada e apagado.

  Restauracao em `docs/src/memory-backup.md`, e o proprio comando imprime os
  quatro passos ao terminar, ja com os caminhos da instalacao. O passo que
  costuma ser esquecido — apagar o `-wal` antigo — esta em destaque nos dois:
  um `-wal` ao lado de um banco restaurado reintroduz exatamente o que se
  acabou de descartar.
