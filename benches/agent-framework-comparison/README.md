# `benches/agent-framework-comparison`

Harness reprodutível para validar os claims comparativos do `README.md`
(GarraIA vs [OpenClaw](https://github.com/openclaw/openclaw) vs
[ZeroClaw](https://github.com/zeroclaw-labs/zeroclaw)).

> **Status (2026-05-04):** scaffolding inicial. Nenhum resultado commitado
> ainda. A primeira execução real entrará como
> `results/<YYYY-MM-DD>-<host>/` em PR separada.

## Objetivo

Substituir os placeholders `Em medição` da tabela "Por que GarraIA?" do
`README.md` por números **medidos**, **versionados** e **reprodutíveis**.
Toda execução grava ambiente (CPU, RAM, OS, kernel, versões dos competidores
pinadas via tag) ao lado dos logs raw, para que a leitura seja auditável
sem confiar na palavra do projeto.

## Hardware target

- DigitalOcean Droplet, 1 vCPU Intel Xeon ~2.5 GHz, 1 GB RAM, SSD NVMe
- Ubuntu 24.04 LTS (kernel 6.x)

Outro hardware é aceito desde que `environment.txt` capture os specs reais.
Resultados em hardware diferente não substituem os do droplet target — vão
pra subpastas separadas (ex.: `results/2026-05-15-droplet-do-1vcpu/` vs
`results/2026-05-15-laptop-arm64/`).

## Versões medidas

| Framework | Como é pinado |
|---|---|
| **GarraIA** | Checkout atual da branch sob teste (`HEAD`). Toolchain do `rust-toolchain.toml`, build `cargo build --release --bin garraia`. |
| **OpenClaw** | Env var `OPENCLAW_REF` (default: `latest`). Instalado em prefix temporário (`mktemp -d`); jamais via `npm install -g` no ambiente do usuário. |
| **ZeroClaw** | Env var `ZEROCLAW_REF` (default: `main`). Clone+build em diretório temporário com a tag fixada. |

## Métricas medidas (escopo atual)

| Métrica | Comando subjacente | Justificativa |
|---|---|---|
| **Tamanho do binário** | `ls -lh <binário>` ou `du -sh <pkg>` (Node) | Compara footprint de distribuição. |
| **Pico de RSS durante `--help`** | `/usr/bin/time -v <bin> --help \| grep "Maximum resident"` | Proxy honesto de footprint mínimo. **Não é "idle memory"** — só medimos uma invocação `--help`. Idle real exige rodar o servidor e coletar métricas durante N segundos; fica para escopo futuro. |
| **Início a frio (`--help`)** | `hyperfine --warmup 3 --runs 20 '<bin> --help'` | Tempo do executável até retornar; não inclui startup do servidor HTTP. Cold start completo (até `/health` retornar `200`) também fica pra escopo futuro. |

## Como rodar

Pré-requisitos (validados pelo próprio `run.sh` antes de qualquer medição):

```bash
cargo --version
git --version
hyperfine --version
npm --version
/usr/bin/time --version
```

Execução:

```bash
cd benches/agent-framework-comparison

./run.sh --all          # roda os 3 frameworks
./run.sh --garraia      # só GarraIA (checkout atual)
./run.sh --openclaw     # só OpenClaw (npm install em prefix temporário)
./run.sh --zeroclaw     # só ZeroClaw (clone+build em mktemp)
```

Variáveis de ambiente opcionais:

```bash
OPENCLAW_REF=v2.3.1 ZEROCLAW_REF=v0.4.0 ./run.sh --all
```

## Como contribuir resultados

1. Rode `./run.sh --all` no hardware desejado (idealmente o droplet target).
2. Inspecione `results/<DATE>-<host>/raw/` — os logs hyperfine + time + ls.
3. Crie `results/<DATE>-<host>/README.md` com tabela-resumo: 1 linha por
   métrica × 3 colunas (GarraIA / OpenClaw / ZeroClaw).
4. Abra PR com a pasta `results/<DATE>-<host>/` inteira commitada.
5. Atualize as 3 linhas correspondentes da tabela do `README.md` raiz
   substituindo `Em medição` pelos números medidos, na **mesma PR**.

**Convenção de datas** (CLAUDE.md §"Convenção de datas"):
- Diretório `results/<DATE>-<host>/` usa data narrativa em
  **America/New_York** (Florida).
- Timestamps dentro de `environment.txt` são **UTC** com sufixo `Z`.

## Não-objetivos

- **Não medimos** throughput agora (`req/s`) — exige mock de provedor LLM
  e processo servidor de pé. Próxima fase.
- **Não medimos** latência P50/P95/P99 do gateway agora — mesma razão.
- **Não rodamos** em CI agora. Workflow `.github/workflows/bench.yml` é
  follow-up.
- **Não instalamos** OpenClaw globalmente. O `run.sh` usa
  `npm_config_prefix=$(mktemp -d)/npm` e descarta no fim.
- **Não inventamos** resultados — `run.sh` só executa comandos reais e
  copia a saída pra `raw/`.

## Arquivos

```
benches/agent-framework-comparison/
├── README.md          # este arquivo
├── run.sh             # harness reprodutível (bash, set -euo pipefail)
├── .gitignore         # ignora results/*/raw/*.log opcionalmente
└── results/
    └── README.md      # formato esperado de results/<DATE>-<host>/
```
