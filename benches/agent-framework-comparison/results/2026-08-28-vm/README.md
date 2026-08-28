# Run 2026-08-28 — host `vm` (x86_64 Linux container)

> **Não é o hardware target** (droplet DO 1 vCPU / 1 GB). Números aqui são
> a primeira execução real versionada do harness; a run no droplet target
> entra em diretório próprio quando executada. Specs completos, versões e
> commits pinados em [`environment.txt`](environment.txt).

## Frameworks e pinagem

| Framework | Versão medida (performance) |
|---|---|
| GarraIA | checkout `ea06286` (branch do PR do comparativo) — `cargo build --release -p garraia` (LTO, stripped) |
| OpenClaw | `openclaw@2026.7.1-2` (npm `latest` na run; ver `raw/openclaw-install.log`) em prefix isolado — executado com Node v24.15.0 isolado (host tinha v22.22.2 < engines) |
| ZeroClaw | clone fresco `--depth 1` do branch default `master` = `d355e3b` (ver `raw/zeroclaw-clone.log`) — build release default (lean bundle) em 10m17s |

A **auditoria de segurança** (004/005) usa checkouts de inspeção
separados, pinados por commit: OpenClaw `343252a`, ZeroClaw `d5617f1`
(registrados em `environment.txt`).

## Cenários 001–003 (performance)

| Métrica | GarraIA | OpenClaw | ZeroClaw |
|---|---|---|---|
| Footprint instalado ([001](../../scenarios/001-binary-size.md)) | 47 MiB (binário único) | 370 MiB (`node_modules`, `du -sh`) + runtime Node ≥22.22.3 | 40 MiB (binário único, build default) |
| Pico de RSS em `--help` ([002](../../scenarios/002-peak-rss.md)) | 8 756 KiB (8,6 MiB) | 50 388 KiB (49,2 MiB) | 15 704 KiB (15,3 MiB) |
| Cold start `--help`, média de 20 runs ([003](../../scenarios/003-cold-start.md)) | 4,1 ms | 46,2 ms | 8,5 ms |

Raw: [`raw/`](raw/) — hyperfine JSON + logs `time -v` + binsize por target.
O `npm install -g` do OpenClaw resolveu **300 pacotes** nesta run
(`raw/openclaw-install.log`).

## Cenários 004–005 (auditoria de segurança)

- [`004-credentials-at-rest/summary.json`](004-credentials-at-rest/summary.json) — **pass**, 7 claims, 0 skipped
- [`005-attack-surface/summary.json`](005-attack-surface/summary.json) — **pass**, 13 claims, 0 skipped (Cargo.lock: GarraIA 1061 crates vs ZeroClaw 1265; OpenClaw 66 deps diretas de produção; isolamento de plugins nos 3 frameworks)

Evidência bruta por claim em `00{4,5}-*/raw/`.

## Notas de honestidade

1. `--help` mede o piso do CLI, não "idle memory" de servidor — ver
   escopo declarado nos cenários 002/003.
2. O binário default do ZeroClaw é ~7 MiB **menor** que o do GarraIA.
3. OpenClaw recusou rodar no Node v22.22.2 do host (engines ≥22.22.3) —
   medido com Node v24.15.0 dedicado; o gate de versão é comportamento
   deles, registrado como fato, não como demérito.
