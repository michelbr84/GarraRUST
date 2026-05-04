# results/

Cada execução de `../run.sh` cria uma pasta nova `<DATE>-<host>/` com:

```
results/<DATE>-<host>/
├── README.md             # sumário humano (1 linha por métrica × 3 frameworks)
├── environment.txt       # CPU, RAM, OS, kernel, versões pinadas (UTC + Florida)
└── raw/
    ├── garraia-binsize.log
    ├── garraia-hyperfine.json
    ├── garraia-hyperfine.log
    ├── garraia-time.log
    ├── openclaw-binsize.log
    ├── openclaw-hyperfine.json
    ├── openclaw-hyperfine.log
    ├── openclaw-install.log    # ignorado pelo .gitignore por padrão
    ├── openclaw-time.log
    ├── zeroclaw-binsize.log
    ├── zeroclaw-build.log      # ignorado pelo .gitignore por padrão
    ├── zeroclaw-clone.log      # ignorado pelo .gitignore por padrão
    ├── zeroclaw-hyperfine.json
    ├── zeroclaw-hyperfine.log
    └── zeroclaw-time.log
```

`<DATE>` é a data narrativa em **America/New_York** (Florida), formato
`YYYY-MM-DD`. Timestamps **dentro** dos arquivos seguem UTC com sufixo
`Z`. Convenção definida em `CLAUDE.md` §"Convenção de datas".

`<host>` é a saída de `hostname -s` (com fallback `unknown`).

## Status

> **Sem resultados commitados ainda.** A primeira run real entrará como
> uma subpasta dedicada, em PR separada. Até lá, a tabela "Por que GarraIA?"
> do `README.md` raiz mostra `Em medição` em todas as três métricas.

## Como adicionar resultados

1. Rode `../run.sh --all` no hardware desejado.
2. Inspecione `<DATE>-<host>/raw/` (gerado automaticamente).
3. Crie `<DATE>-<host>/README.md` com tabela-resumo:

   ```md
   # Resultados — <DATE> em <host>

   | Métrica                       | GarraIA | OpenClaw | ZeroClaw |
   |-------------------------------|---------|----------|----------|
   | Tamanho do binário            | …       | …        | …        |
   | Pico de RSS (`--help`)        | …       | …        | …        |
   | Início a frio (`--help`) p50  | …       | …        | …        |

   Veja `environment.txt` e `raw/*.json` para detalhes.
   ```

4. Abra PR commitando a subpasta inteira + atualizando o `README.md` raiz
   substituindo `Em medição` pelos números medidos.
