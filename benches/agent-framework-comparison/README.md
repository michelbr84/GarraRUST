# `benches/agent-framework-comparison`

Harness reprodutível para validar os claims comparativos do `README.md`
(GarraIA vs [OpenClaw](https://github.com/openclaw/openclaw) vs
[ZeroClaw](https://github.com/zeroclaw-labs/zeroclaw)).

> **Status (2026-08-28):** primeira execução real versionada em
> [`results/2026-08-28-vm/`](results/2026-08-28-vm/) (container x86_64;
> a run no droplet target segue pendente). Método formalizado em
> cenários declarativos (`scenarios/`).

## Método: cenários declarativos

Adotamos o contrato de validação por cenários do
[claude-stack-lab](https://github.com/michelbr84/claude-stack-lab):
cada dimensão comparada é um **cenário versionado** com seções fixas —
*objetivo → alvos pinados → comando esperado → resultado esperado →
evidência mínima → status*. Regra herdada intacta: **número sem artefato
bruto commitado não conta como claim válido** no `README.md` raiz.

| Cenário | Dimensão | Tipo |
|---|---|---|
| [001-binary-size](scenarios/001-binary-size.md) | Footprint instalado | medição |
| [002-peak-rss](scenarios/002-peak-rss.md) | Pico de RSS (`--help`) | medição |
| [003-cold-start](scenarios/003-cold-start.md) | Cold start (`--help`, hyperfine) | medição |
| [004-credentials-at-rest](scenarios/004-credentials-at-rest.md) | Criptografia de credenciais em repouso | claims-table (inspeção de código) |
| [005-attack-surface](scenarios/005-attack-surface.md) | Bind/auth defaults + dependências | claims-table (inspeção de código) |

Os cenários 004/005 verificam claims **mecanicamente** (grep contra
checkouts pinados por commit) e gravam `{target, name, ok, detail}` em
`summary.json` + saída bruta por claim em `raw/`. Claims dos concorrentes
incluem os pontos onde eles são **superiores** (ex.: ZeroClaw cifra
secrets por default) — o comparativo só tem valor se sobreviver a
fact-check hostil.

## Hardware target

- DigitalOcean Droplet, 1 vCPU Intel Xeon ~2.5 GHz, 1 GB RAM, SSD NVMe
- Ubuntu 24.04 LTS (kernel 6.x)

Outro hardware é aceito desde que `environment.txt` capture os specs
reais; resultados vão para subpastas separadas por `<data>-<host>` e não
substituem os do droplet target.

## Versões medidas

| Framework | Como é pinado |
|---|---|
| **GarraIA** | Checkout atual (`HEAD`), `cargo build --release -p garraia` (o binário chama-se `garra`). |
| **OpenClaw** | Env var `OPENCLAW_REF` (default: `latest`), npm em prefix temporário isolado. Exige Node dentro dos `engines` do pacote. |
| **ZeroClaw** | Env var `ZEROCLAW_REF` (default: branch default do upstream, hoje `master`), clone+build em mktemp. |
| Cenários 004/005 | Checkouts locais via `OPENCLAW_CHECKOUT` / `ZEROCLAW_CHECKOUT`; commits registrados em `environment.txt`. Ausente ⇒ claims `skipped`, nunca inventados. |

## Como rodar

Pré-requisitos (validados pelo `run.sh`): `cargo`, `git`, `hyperfine`,
`npm`, `/usr/bin/time` (GNU time), `python3` (cenários 004/005).

```bash
cd benches/agent-framework-comparison

./run.sh --all           # cenários 001-003 nos 3 frameworks
./run.sh --garraia       # só GarraIA
./run.sh --openclaw      # só OpenClaw (npm em prefix temporário)
./run.sh --zeroclaw      # só ZeroClaw (clone+build em mktemp)

# Auditoria de segurança (004+005) contra checkouts pinados:
OPENCLAW_CHECKOUT=/path/openclaw ZEROCLAW_CHECKOUT=/path/zeroclaw \
  ./run.sh --scenarios   # ou --scenario-004 / --scenario-005
```

## Como contribuir resultados

1. Rode os cenários no hardware desejado (idealmente o droplet target).
2. Inspecione `results/<DATE>-<host>/` — raw logs + `summary.json`.
3. Escreva `results/<DATE>-<host>/README.md` com a tabela-resumo
   (ver [`results/2026-08-28-vm/README.md`](results/2026-08-28-vm/README.md)
   como modelo).
4. Abra PR com a pasta inteira commitada **e**, no mesmo PR, atualize as
   linhas correspondentes da tabela do `README.md` raiz.

**Convenção de datas** (CLAUDE.md §"Convenção de datas"): diretório
`results/<DATE>-<host>/` usa data narrativa **America/New_York**;
timestamps internos são **UTC** com sufixo `Z`.

## Não-objetivos

- **Não medimos** throughput (`req/s`) nem latência P50/P95 do gateway —
  exigem mock de provedor LLM e servidor de pé; próxima fase.
- **Não medimos** "idle memory" de servidor — `--help` é o piso do CLI e
  os cenários dizem isso explicitamente.
- **Não rodamos** em CI (follow-up).
- **Não instalamos** nada globalmente; prefixes temporários sempre.
- **Não inventamos** resultados — só comandos reais com saída copiada
  para `raw/`; concorrente indisponível vira `skipped`.

## Arquivos

```text
benches/agent-framework-comparison/
├── README.md            # este arquivo
├── run.sh               # harness (bash, set -euo pipefail)
├── scenarios/           # 001..005 — declarações do método
└── results/
    ├── README.md        # formato esperado
    └── 2026-08-28-vm/   # primeira run versionada (container x86_64)
```
