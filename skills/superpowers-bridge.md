---
name: superpowers-bridge
description: Mapeia skills locais do GarraRUST para o workflow Superpowers, definindo prioridades e delegação entre os dois sistemas de skills.
triggers:
  - superpowers
  - workflow
  - planning workflow
  - dev workflow
dependencies: []
---

# Superpowers Bridge — GarraRUST

Este skill define como as skills locais do GarraRUST se integram com o framework [Superpowers](https://github.com/obra/superpowers).

---

## Princípio

- **Superpowers** é o workflow primário para desenvolvimento de features novas (brainstorming → spec → plan → TDD → code review → merge)
- **Skills locais** (`skills/`) são usadas para operações específicas do GarraRUST que o Superpowers não cobre

---

## Mapeamento de Skills

| Skill Local | Equivalente Superpowers | Quem usar |
|-------------|------------------------|-----------|
| `/tdd-loop` | `test-driven-development` | **Superpowers** — mais rigoroso (RED-GREEN-REFACTOR com deleção de código sem teste) |
| `/code-review` | `requesting-code-review` | **Superpowers** — review em 2 estágios (spec compliance + code quality) |
| `/review-pr` | `requesting-code-review` + `receiving-code-review` | **Superpowers** — complementar com security-auditor local |
| `/git-assist` | `using-git-worktrees` + `finishing-a-development-branch` | **Superpowers** — worktrees para branches paralelas |
| `/fix-issue` | `systematic-debugging` + `verification-before-completion` | **Superpowers** — 4-phase root cause process |
| `/refactor-module` | `writing-plans` + `executing-plans` | **Superpowers** — plans com tasks de 2-5 min |
| `/assemble-team` | `dispatching-parallel-agents` + `subagent-driven-development` | **Superpowers** — subagents com 2-stage review |
| `/pre-commit` | N/A (local only) | **Local** — validação de segredos, debug, lint específica do GarraRUST |
| `/generate-docs` | N/A (local only) | **Local** — geração de docs PT-BR/EN específica do projeto |
| `/shell-explain` | N/A (local only) | **Local** — explicação de comandos shell |
| `/summarize` | N/A (local only) | **Local** — resumo de contexto |
| `/translate` | N/A (local only) | **Local** — tradução PT-BR ↔ EN |
| `/web-lookup` | N/A (local only) | **Local** — pesquisa web |

---

## Quando usar Superpowers

1. **Feature nova** → Superpowers brainstorming → spec → plan → implement
2. **Bug complexo** → Superpowers systematic-debugging → verify → commit
3. **Refactoring** → Superpowers writing-plans → executing-plans → review
4. **Code review** → Superpowers requesting-code-review (2-stage)
5. **Git branch workflow** → Superpowers git-worktrees → finishing-branch

## Quando usar Skills locais

1. **Pre-commit check** → `/pre-commit` (segredos, debug, lint)
2. **Gerar docs** → `/generate-docs` (PT-BR/EN)
3. **Tradução** → `/translate`
4. **Explicar shell** → `/shell-explain`
5. **Resumir contexto** → `/summarize`

---

## Runner de Testes (Superpowers ↔ GarraRUST)

Quando Superpowers invocar TDD, usar estes comandos:

```bash
# Rust — rodar um teste específico
cargo test -p <crate> <test_name> 2>&1 | tail -5

# Rust — rodar todos os testes de um crate
cargo test -p <crate>

# Rust — rodar todos os testes do workspace
cargo test --workspace

# Flutter — rodar testes
cd apps/garraia-mobile && flutter test

# Lint
cargo clippy --workspace

# Format check
cargo fmt --check
```
