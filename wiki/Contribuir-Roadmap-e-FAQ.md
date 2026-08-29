# Contribuir, Roadmap e FAQ

## Contribuir

Guia completo: [CONTRIBUTING.md](https://github.com/michelbr84/GarraRUST/blob/main/CONTRIBUTING.md) — pré-requisitos (Rust 1.94+, FFmpeg 6.x, Node 20+), setup, convenções (Conventional Commits) e processo de PR. Antes de abrir PR, rode `garra verify` (fmt + clippy + test + gitleaks). Issues para começar: [good first issue](https://github.com/michelbr84/GarraRUST/issues?q=is%3Aissue+is%3Aopen+label%3A%22good+first+issue%22). Decisões arquiteturais exigem [ADR](https://github.com/michelbr84/GarraRUST/blob/main/docs/adr/README.md) antes do código.

## Roadmap AAA (7 fases)

Fonte de verdade: [ROADMAP.md](https://github.com/michelbr84/GarraRUST/blob/main/ROADMAP.md) (com [TODO.md](https://github.com/michelbr84/GarraRUST/blob/main/TODO.md) como backlog curto).

1. Fundações de Core & Inferência
2. Performance, Memória de Longo Prazo & MCP Ecosystem
3. Group Workspace (multi-tenant família/equipe)
4. Experiência Multi-Plataforma AAA
5. Qualidade, Segurança, Compliance & Polishing
6. Lançamento, Observabilidade SRE & GA
7. Pós-GA & Evolução (contínuo)

## FAQ

**O instalador falhou com HTTP 429.** Rate-limit do `raw.githubusercontent.com`. Use o espelho: `curl -fsSL https://github.com/michelbr84/GarraRUST/releases/latest/download/install.sh | sh`.

**`garra update` devolve 404.** Instalações anteriores à v0.2.1 não têm o canal de update — reinstale com o one-liner do instalador.

**Existe binário para ARM64?** Sim, Linux ARM64 a partir da **v0.3.2**.

**Endpoints de auth respondem 503.** Comportamento fail-closed proposital: falta `GARRAIA_JWT_SECRET` no ambiente. Ver [docs/auth-config.md](https://github.com/michelbr84/GarraRUST/blob/main/docs/auth-config.md).

**`GarraIA_VAULT_PASSPHRASE` ou `GARRAIA_VAULT_PASSPHRASE`?** As duas grafias funcionam em todos os consumidores; a all-caps é a canônica e a mista está deprecated com warning (issue #824).

**O que significam os exit codes de `garra config check` / `garra verify`?** `config check`: 0 ok, 2 warnings (com `--strict`), 65 configuração inválida. `verify`: 0 ok, 2 falha em alguma etapa.
