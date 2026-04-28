# Coverage Baseline — abril 2026 (GAR-435)

## Visão geral

PR-2 da sessão sprightly-raven (2026-04-28) adicionou o job `coverage` em `.github/workflows/ci.yml`, fechando a sub-issue **GAR-435** (Q5 do EPIC GAR-430 — Quality Gates Phase 3.6).

## O que o job faz

- Roda em `ubuntu-latest` em todo `pull_request` e `push: branches: [main, develop, master]`.
- Instala `cargo-llvm-cov ^0.6` via `cargo install --locked` (mesmo padrão de supply-chain de `cargo-audit` / `cargo-deny`).
- Executa `cargo llvm-cov --workspace --exclude <…> --lcov --output-path lcov.info`.
- Faz upload de dois artifacts (retenção 14 dias):
  - `coverage-lcov-${{ github.run_id }}` — `lcov.info` para consumo por ferramentas externas (lcov, genhtml, IDE plugins).
  - `coverage-summary-${{ github.run_id }}` — `coverage-summary.txt` com a tabela textual.
- Posta a tabela textual como PR comment via `gh pr comment` usando o `GITHUB_TOKEN` default (sem Codecov/Coveralls — ajuste #2 do plan foamy-origami).

## Política de exclusões

| Crate | Razão | Quando reincluir |
|---|---|---|
| `garraia-desktop` | Tauri sidecar + GTK ausentes em runners GHA. Excluído também por clippy/test/build/msrv. | Nunca (Desktop tem sua própria pipeline). |
| `garraia-auth` | Integration tests requerem Postgres testcontainer + secrets (`GARRAIA_JWT_SECRET`, `GARRAIA_REFRESH_HMAC_SECRET`, `GARRAIA_LOGIN_DATABASE_URL`, `GARRAIA_SIGNUP_DATABASE_URL`). Este job intencionalmente não provisiona esse contexto. | Quando o job ganhar Postgres service container + envs masqueradas, em sub-issue dedicada. |
| `garraia-workspace` | Mesma razão de `garraia-auth`. | Mesma sub-issue de reinclusão. |

A exclusão é cosmética sob a perspectiva do gate: o objetivo do PR-2 é instrumentar cobertura para os ~17 crates que rodam sem testcontainers. Os 3 excluídos têm cobertura via `Test (ubuntu-latest)` e `E2E Tests` jobs, que rodam contra Postgres real.

## Soft-gate (sem threshold)

Per plan sprightly-raven §11.3:

- **Sem threshold de cobertura** — % por crate ou agregado **não** bloqueia PRs.
- O job **roda em todo PR** e emite artifact + comentário.
- **Sem `continue-on-error: true` permanente** — se o job quebrar (ex.: install do `cargo-llvm-cov` falhar, CI runtime estourar timeout, instrumentação LLVM falhar a link), o job falha duro, abrindo issue de fix imediato. Isso preserva o contrato fechado pelo GAR-453 de eliminar todas as silent skips.

Fork PRs (PRs vindos de forks externos do repo público) não recebem `pull-requests: write` por política do GitHub. O step `Post coverage summary as PR comment` é gated por `github.event.pull_request.head.repo.full_name == github.repository` para falhar gracefully nesse caso (artifact ainda é gerado e baixável; só o comment é pulado).

## Baseline empírico

Os valores de cobertura iniciais são publicados como artifact + PR comment a cada run. Esta seção **intencionalmente não fixa números** porque:

1. Cobertura flutua a cada PR.
2. Manter números aqui geraria drift entre o doc e a realidade.
3. O GitHub Actions UI já dá o histórico via artifacts.

Para inspecionar a cobertura de qualquer PR ou commit em `main`:

1. Abrir [Actions → CI](https://github.com/michelbr84/GarraRUST/actions/workflows/ci.yml).
2. Clicar no run desejado.
3. Baixar o artifact `coverage-summary-<run_id>` (texto) ou `coverage-lcov-<run_id>` (lcov para `genhtml` / IDE plugins).

Para baseline qualitativo no momento do PR-2 (ver primeiro CI run em `main`):

- 17 crates instrumentados (workspace exceto os 3 excluídos acima).
- Métricas reportadas por crate: lines, regions, functions, branches.

## Trilha de evolução (follow-ups de GAR-435)

Depois que este PR mergear e a cobertura virar parte do CI normal:

1. **Q1 — reincluir `garraia-auth` e `garraia-workspace`.** Requer adicionar Postgres service container + envs masqueradas no job `coverage` (igual ao job `e2e`). Sub-issue do EPIC GAR-430 (TBD).
2. **Q2 — diff coverage.** Postar no PR comment apenas o delta vs `main` em vez da tabela completa, para feedback mais útil. Pode usar `cargo llvm-cov` com `--summary-only --no-cfg-coverage` + `diff-cover` ou similar.
3. **Q3 — threshold gate (opcional, decisão futura).** Se o time decidir, transformar o soft-gate em blocking-gate com threshold mínimo (ex.: 60%) por crate. **Não é objetivo de PR-2.**

## Referências

- Linear [GAR-435](https://linear.app/chatgpt25/issue/GAR-435).
- Linear [GAR-430](https://linear.app/chatgpt25/issue/GAR-430) (parent EPIC).
- Linear [GAR-453](https://linear.app/chatgpt25/issue/GAR-453) (continue-on-error elimination contract).
- Plan: `C:/Users/miche/.claude/plans/voc-est-no-reposit-rio-sprightly-raven.md`.
