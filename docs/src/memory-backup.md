# Backup e restauração da memória

A memória semântica é o ativo que mais dói perder: fatos, preferências e o
histórico que alimenta o recall. Ela vive num arquivo só —
`~/.config/garraia/data/memory.db` — que guarda as entradas **e** o índice
vetorial.

```bash
garra memory backup
```

Escreve um retrato consistente em `~/.config/garraia/data/backups/`, com
nome ordenável por data: `memory-20260906T054500123Z.db` (UTC, com
milissegundo — dois backups seguidos não podem colidir).

## Por que não `cp`

O banco roda em modo WAL. Copiar o arquivo com `cp` pega uma foto **sem** as
transações que ainda estão no `-wal` ao lado, e o resultado é um backup que
parece bom e está incompleto — o pior tipo.

O comando usa o `VACUUM INTO` do SQLite, que lê sob uma transação e escreve o
estado commitado inteiro. Não precisa de checkpoint, não precisa parar o
gateway, e de brinde compacta: páginas livres não vão junto.

O índice vetorial vai junto — as tabelas `vec_embeddings_*`, as sombras delas
e o `vec_id_map`. A cópia reabre com o mesmo relatório de integridade que a
origem, o que é verificado por teste.

## Retenção

```bash
garra memory backup --keep-days 14
```

Apaga backups **nossos** com mais de 14 dias, depois de o novo existir. Duas
garantias que valem saber:

- só apaga arquivo que casa com o padrão `memory-<data>Z.db` que o próprio
  comando cria. Um backup manual seu, com outro nome, fica;
- a idade vem do **nome**, não do `mtime`. Copiar o diretório de backups para
  outra máquina renova todo `mtime` e apagaria tudo na primeira execução
  seguinte.

Sem `--keep-days`, nada é apagado.

Use um diretório **dedicado**. O `--dir` aceita qualquer caminho, e a retenção
varre o que estiver lá: um arquivo alheio que por acaso se chame
`memory-<data>Z.db` seria apagado junto. O padrão (`<data_dir>/backups`) já é
dedicado.

## Restaurar

> **O gateway tem de estar parado antes do passo 3.** O SQLite mantém o
> `-wal` aberto enquanto houver conexão viva; apagá-lo com o gateway rodando
> **corrompe o banco**. Se o `garra stop` não confirmar, confira antes de
> seguir.

```bash
garra stop
cp ~/.config/garraia/data/backups/memory-20260906T054500123Z.db \
   ~/.config/garraia/data/memory.db
rm -f ~/.config/garraia/data/memory.db-wal ~/.config/garraia/data/memory.db-shm
garra start
```

**O terceiro passo é o que costuma ser esquecido.** Um `-wal` antigo ao lado
de um banco restaurado reintroduz exatamente o que você acabou de descartar.
O nome do arquivo acima é um exemplo — use o que o `garra memory backup`
imprimiu (ele mostra os quatro passos ao terminar, já com os caminhos da sua
instalação), ou `ls` no diretório de backups.

Depois de subir, confira:

```bash
garra memory stats
```

Se `Linhas no mapa` estiver abaixo de `com vetor`, rode `garra memory reindex`
— ele devolve ao índice os vetores que já estão na coluna, sem custo de
provider.

## Agendar

Não há agendador embutido para o backup. No Linux, um timer do systemd ou uma
linha de cron resolvem:

```cron
0 4 * * * /usr/local/bin/garra memory backup --keep-days 14
```

O que **é** agendado pelo gateway é a retenção da *memória* (não a dos
backups): `memory.retention` apaga entradas antigas e vencidas, e nasce
desligada. Faz sentido ligar o backup antes dela.
