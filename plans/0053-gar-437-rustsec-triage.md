# 0053 — GAR-437 RUSTSEC triage (Q7 sob Epic GAR-430)

> **Status:** Em execução 2026-04-25.
> **Aprovação:** plan completo aprovado em `~/.claude/plans/estamos-retomando-o-projeto-serialized-karp.md` (sessão `serialized-karp`, 2026-04-25). Espelho conciso aqui.
> **Deadline:** 2026-05-20.
> **Modo:** conservador, 1 PR por lote, ≤ 500 LOC, 7 sub-issues GAR-447..GAR-453 + GAR-454 (Q7b wasmtime sob GAR-430).

## §1 Goal

Fechar o último `continue-on-error: true` do `.github/workflows/ci.yml` (job `security`, linha 475) convertendo `cargo audit` em gate bloqueante. Resolver **8 advisories** cobertas pelo escopo GAR-437 antes da deadline 2026-05-20.

## §2 Contexto

GAR-443 (PR #67, `f3b92da`) acaba de fechar a Lote 4 Playwright. Sobrou 1 CoE — o `cargo audit` informativo. `.cargo/audit.toml` tem 13 advisories ignoradas, todas expirando 2026-05-20. Sem ação, CI volta a emitir warnings de ignore expirado.

Wasmtime (×2 advisories) sai de GAR-437 e vira **GAR-454 (Q7b)** com plan próprio `plans/0050.4-wasmtime-upgrade.md` (a criar). glib + rand permanecem com ignore explícito refrescado.

## §3 Scope / Non-scope

### In scope
| ID | Crate | PR | Sub-issue |
|---|---|---|---|
| RUSTSEC-2026-{0049,0098,0099,0104} | rustls-webpki **production hot path 0.103.x** | PR-1 | GAR-447 (parcial; residual em GAR-455 Q7c) |
| RUSTSEC-2024-0421 | idna | PR-2 | GAR-448 |
| RUSTSEC-2025-0111 | tokio-tar (dev-only) | PR-3 | GAR-449 |
| RUSTSEC-2023-0071 | rsa (via sqlx-mysql leak) | PR-4 | GAR-450 |
| RUSTSEC-2026-0105 | core2 (yanked) | PR-5 | GAR-451 |
| RUSTSEC-2026-0002 | lru (drop ignore — não está no tree) | PR-6 | GAR-452 |
| (cleanup audit.toml + deny.toml) | — | PR-6 | GAR-452 |
| (CI blocking flip) | — | PR-7 | GAR-453 |

> **Adendo PR-1 (2026-04-25):** investigação técnica revelou que `cargo-audit` não permite ignore por (advisory ID, versão). O patch `0.103.9 → 0.103.13` fecha o **production hot path** (rustls 0.23 chain), mas as cadeias residuais `rustls-webpki 0.102.8` (via `serenity 0.12.5 → tokio-tungstenite 0.21 → rustls 0.22`) e `rustls-webpki 0.101.7` (via `aws-smithy-http-client 1.1.12`, feature `storage-s3`) continuam disparando os mesmos 4 IDs porque **não há fix upstream** (serenity 0.12.5 e aws-smithy-http-client 1.1.12 são os latest stable). PR-1 entrega **mitigação parcial honesta**, mantém os 4 IDs em `ignore` com comments explícitos, e cria **GAR-455 (Q7c)** para rastrear o upstream blocker até 2026-07-31. **Não há override agressivo, fork de serenity nem bump semver-incompatible de tokio-tungstenite.**

### Out of scope
- **wasmtime ×2** → GAR-454 (Q7b), plan 0050.4 a escrever.
- **rand semver bump** → refresh exp 2026-07-31 (ou Q7c se priorizado).
- **glib (desktop only)** → refresh exp 2026-07-31.
- **MSRV bump** → GAR-441.
- **Coverage / mutation / refactor de hotspots** → Q5/Q6/Q9..Q11.
- **Playwright / Echo provider** → GAR-443 / GAR-444 (já Done).

## §4 Acceptance criteria

1. `.github/workflows/ci.yml`: zero `continue-on-error: true` (net 1→0).
2. `cargo audit --no-fetch` exit 0 com apenas wasmtime ×2 + glib + rand (+ rsa se PR-4 cair em Caso B) como ignores.
3. Cada ignore remanescente em `audit.toml` aponta para issue Linear concreto.
4. `audit.toml` ↔ `deny.toml` em sync para wasmtime IDs.
5. CI run mais recente em main após PR-7: todas as jobs verdes, **`security` job blocking**.
6. GAR-437 movido para Done com comment de evidência.

## §5 PR sequence (critical path: PR-1 → PR-6 → PR-7)

| # | Slug | Sub-issue | Risco | Branch | Files |
|---|---|---|---|---|---|
| PR-1 | rustls-webpki 0.103.x hot path patch (parcial; residual → GAR-455) | GAR-447 | Baixo (lockfile-only) | `gar-437a-rustls-webpki` | `Cargo.lock`, `.cargo/audit.toml` (comments) |
| PR-2 | idna via validator bump | GAR-448 | Médio (validation macros) | `gar-437b-idna-validator` | workspace + 4 crate Cargo.toml |
| PR-3 | tokio-tar via testcontainers | GAR-449 | Baixo (dev-dep) | `gar-437c-tokio-tar` | dev-deps de 5 crates |
| PR-4 | sqlx-mysql cleanup (rsa) | GAR-450 | Baixo-Médio | `gar-437d-sqlx-no-mysql` | workspace `Cargo.toml` |
| PR-5 | core2 via bitstream-io | GAR-451 | Baixo | `gar-437e-core2` | `garraia-media/Cargo.toml` |
| PR-6 | audit.toml cleanup | GAR-452 | Baixo (no code) | `gar-437f-audit-cleanup` | `.cargo/audit.toml`, `deny.toml` |
| PR-7 | CI security blocking | GAR-453 | Baixo-Médio | `gar-437g-ci-blocking` | `.github/workflows/ci.yml` |

PR-2/3/4/5 paralelizáveis. PR-6 espera PR-1..PR-5 + GAR-454 (Q7b) existir. PR-7 espera PR-1..PR-6 + validação empírica do `cargo audit` no `ci.yml`.

## §6 Quality gates por PR

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --exclude garraia-desktop --all-targets -- -D warnings`
- `cargo test` nos crates tocados
- `cargo deny check bans licenses sources`
- `gitleaks` + `dependency-review` (no PR)
- `cargo audit --no-fetch` (informativo até PR-7; bloqueante depois)
- Review @code-reviewer obrigatório
- Review @security-auditor obrigatório em PR-1, PR-4, PR-7

## §7 Riscos principais

- **PR-1 TLS surface:** lockfile-only patch é o caminho menor risco.
- **PR-2 validator macros:** testes reais necessários, não só clippy. Fallback validator 0.19 aceito.
- **PR-4 sqlx-mysql:** se for dep real downstream, fallback é refresh de ignore (não obrigatório-sucesso).
- **PR-5 core2:** AVIF feature-gate é safe escape se rav1e não aceitar bitstream-io 5.x.
- **PR-7 CI flip:** validar empiricamente o setup do advisory DB antes do merge.

## §8 Rollback plan

Cada PR é `git revert <sha>` simples (Cargo.toml/Cargo.lock/audit.toml/ci.yml apenas — sem migrations Postgres, sem schema changes). Pior caso: revert PR-7 mantém `security` informativo + refresh expirations + replanejar.

## §9 Verificação end-to-end (após PR-7)

1. `git checkout main && git pull`
2. `cargo install cargo-audit --locked`
3. `cargo audit --no-fetch --deny unsound` → exit 0
4. `grep -rn "continue-on-error: true" .github/workflows/` → vazio
5. `cargo deny check advisories bans licenses sources` → exit 0
6. CI run mais recente em main: `security` job verde sem CoE
7. Linear: GAR-437 → Done

## §10 Evidência

- **Linear:** sub-issues GAR-447..GAR-453 + GAR-454 com link para PR + output de `cargo audit` + workflow run.
- **PR body:** seção "Evidence" com diff de `Cargo.lock` + `cargo tree -i` antes/depois + `cargo audit` antes/depois.
- **`.garra-estado.md`:** session header com lote + advisory IDs + link PR (via hook `stop.sh`).

## §11 Restrições rígidas

- ❌ Sem CoE novo em workflows.
- ❌ Sem teste skip/mock sem issue Linear.
- ❌ Sem PR > 500 LOC.
- ❌ Sem MSRV/coverage/mutation/refactor neste lote.
- ❌ Wasmtime fora deste lote (Q7b).
- ❌ Secrets nunca commitados; nunca lidos `.env` salvo necessidade explícita.

## §12 Open questions

Todas resolvidas pré-execução pelo usuário (2026-04-25):
- Q1 numeração `0053` ✅
- Q2 Q7b criado **antes** da PR-6 ✅ (GAR-454)
- Q3 sub-issues a..g criadas ✅ (GAR-447..GAR-453)
- Q4 PR-4 não obrigatório-sucesso ✅
- Q5 PR-2 fallback validator 0.19 aceito ✅
- Q6 PR-7 valida empiricamente `cargo audit --no-fetch` no `ci.yml` antes de remover CoE ✅

## §13 Plan completo

Espelho conciso. Plano completo com seções 1-13 detalhadas em `~/.claude/plans/estamos-retomando-o-projeto-serialized-karp.md`.

---

## §14 PR-2 evidence (GAR-448) — sessão `mutable-kite`, 2026-04-25

**Plano detalhado:** `~/.claude/plans/estamos-retomando-o-projeto-mutable-kite.md` (aprovado pelo usuário com 6 ajustes).

### Mudanças aplicadas
- 4× `crates/{garraia-auth, garraia-config, garraia-telemetry, garraia-workspace}/Cargo.toml`: `validator = { version = "0.18", features = ["derive"] }` → `"0.20"`.
- `Cargo.lock`: `validator 0.18.1 → 0.20.0`, `validator_derive 0.18.2 → 0.20.0`, `idna 0.5.0` REMOVIDO. Transitivos novos: `proc-macro-error2 2.0.1` + `proc-macro-error-attr2 2.0.0` (deps de `validator_derive 0.20`). 5 files changed, 12 insertions, 21 deletions (`git diff --stat origin/main`).
- `.cargo/audit.toml`: bloco `RUSTSEC-2024-0421` removido (era 13 IDs em ignore; agora 12). `deny.toml` não tinha entrada paralela — não tocado.
- `plans/README.md`: linha de tabela 0053 (já dirty na sessão `serialized-karp`, escopo legítimo).
- `plans/0053-gar-437-rustsec-triage.md`: este apêndice §14 (untracked → tracked).

### Gates locais (worktree `gar-437-idna` em `gar-437b-idna-validator @ eb086ec` base)

| Gate | Resultado |
|---|---|
| `cargo update -p validator -p validator_derive` | ✓ removed `idna 0.5.0`, updated 2 packages |
| `cargo build -p garraia-auth -p garraia-config -p garraia-telemetry -p garraia-workspace` | ✓ 45.06s |
| `cargo clippy --workspace --exclude garraia-desktop --all-targets -- -D warnings` | ✓ 1m43s, zero warnings |
| `cargo test --lib -p garraia-auth -p garraia-config -p garraia-telemetry -p garraia-workspace` | ✓ all green (config 46/46, telemetry 16/16, workspace 5/5, auth lib all pass) |
| `cargo test --lib --workspace --exclude garraia-desktop` | ✓ all green |
| `cargo tree -i idna` | ✓ apenas `idna 1.1.0` (única versão) |
| `cargo tree -d \| grep -E 'validator\|idna\|url\|proc-macro-error'` (top-level dups) | ✓ ZERO target-name dups |
| `cargo deny check bans licenses sources` | ✓ bans ok, licenses ok, sources ok |
| `cargo audit` (com fetch) | 37 IDs ativos (nenhum introduzido pelo PR-2) |
| `cargo audit --no-fetch` | 37 IDs ativos (idem) |
| Diff IDs ativos PR-1 base (eb086ec) vs PR-2 worktree | **vazio** — PR-2 introduz ZERO novas advisories |
| `grep RUSTSEC-2024-0421` em audit ativo | ABSENT ✓ (alvo fechado) |

### Notas operacionais sobre `cargo audit`

`cargo audit` reporta 37 IDs ativos em ambos PR-1 base e PR-2 worktree porque o advisory DB local é mais recente que o snapshot que GAR-437 capturou (a maioria são `wasmtime` 28.x — Q7b/GAR-454, `gtk-rs` unmaintained — desktop-only, `quinn-proto` — não fix), e o job `security` do CI tem `continue-on-error: true` (linha 475 de `ci.yml`) — exatamente o último CoE que PR-7/GAR-453 vai remover. PR-2 **não** introduz nenhuma das 37; o conjunto é idêntico antes e depois do bump (validado por `diff` empírico). O alvo `RUSTSEC-2024-0421` desaparece do conjunto pré-existente que ainda fica em ignore (12, era 13).

### Aceite GAR-448 (cumulativo)

1. ✓ `cargo tree -i idna` mostra apenas `idna 1.x` (1.1.0).
2. ✓ `RUSTSEC-2024-0421` removida de `.cargo/audit.toml` (deny.toml não tinha entrada paralela).
3. ✓ `cargo test` (lib, workspace ex-desktop) verde nos 4 crates target + restante.
4. (CI) Verde sem novos `continue-on-error`, sem `--admin`, sem skip — pendente do CI run.
5. ✓ `cargo audit` não regride; 13 IDs em ignore → 12.
6. (Pendente) `@code-reviewer` + `@security-auditor` APPROVE.

### Rollback

`git revert <merge-sha>` (lockfile + 4 Cargo.toml + audit.toml + plans). Sem migrations, sem schema changes. Local-only fallback: `cargo update -p validator --precise 0.18.1`.
