# Memory System

> **Esta página foi substituída.** O conteúdo vivo e conferido está em
> **[`docs/src/memory.md`](src/memory.md)** — a página do book, em português.

## Por que este arquivo ainda existe

Porque o wiki do projeto aponta para ele como "Sistema de memória", e apagá-lo
quebraria esses links. Ele fica como redirecionamento, não como documentação.

## O que ele dizia, e por que não pode mais dizer

Até 2026-09-07 este arquivo descrevia um sistema que **nunca foi construído**:
um `facts.json` com array de fatos datados, as chaves de configuração
`auto_extract` e `max_facts`, e os comandos `garraia memory clear`,
`garraia memory export` e `garraia memory disable`.

Nada disso existe. A reescrita da página do book (#963) foi feita conferindo
cada afirmação contra o código; esta cópia ficou para trás e seguiu sendo
servida ao leitor que chegava pelo wiki.

O que existe de verdade, hoje:

```text
garra memory stats | list | add | search | reindex | backup | pin | ttl | delete | compact
```

`add` é recente (#958) — foi construído, não herdado daquela lista. Não há
`clear`, `export` nem `disable`: para apagar use `delete` (uma entrada) ou
`compact` (por idade); para levar embora, `backup`.

Os detalhes — o caminho de um turno, a configuração real, o que ainda não
existe — estão em [`docs/src/memory.md`](src/memory.md).
