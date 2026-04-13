# plans/

Histórico de planos de execução do GarraIA. Cada plano está atrelado a uma issue GAR-* no Linear e é aprovado antes da execução.

## Convenção de nome

`NNNN-gar-XXX-slug-descritivo.md`

- `NNNN` — sequencial monotônico (`0001`, `0002`, ...) — ordem cronológica de criação.
- `gar-XXX` — issue Linear principal que o plano entrega.
- `slug-descritivo` — identificador humano curto em kebab-case.

## Regras

- **Aprovação obrigatória:** nenhum plano vira código sem "Plano aprovado" explícito do owner.
- **Imutável após merge:** um plano é o registro histórico de como a decisão foi tomada. Se o escopo mudar, crie um novo plano (`NNNN+1`) que o supersede.
- **Escopo claro:** `§1 Goal`, `§3 Scope/Non-scope`, `§4 Acceptance criteria` são obrigatórios.
- **Rollback plan:** todo plano precisa de `§8 Rollback plan` — se é reversível, como; se não é, por quê.
- **Open questions:** dúvidas que bloqueiam execução ficam no `§12 Open questions` e precisam ser respondidas antes do start.

## Index

| # | Plano | Issue | Status |
|---|---|---|---|
| 0001 | [OpenTelemetry + Prometheus baseline](0001-gar-384-opentelemetry-baseline.md) | [GAR-384](https://linear.app/chatgpt25/issue/GAR-384) | ✅ Merged 2026-04-13 (`84c4753`) |
| 0002 | [ADR 0003 — Database para Group Workspace](0002-gar-373-adr-postgres-decision.md) | [GAR-373](https://linear.app/chatgpt25/issue/GAR-373) | 📋 Awaiting approval |

## Arquivos não-versionados

Drafts ad-hoc dentro de `plans/` que **não** sigam o padrão `NNNN-*.md` ficam gitignored por design — ver `.gitignore`. Útil para rascunhos pessoais antes de formalizar um plano numerado.
