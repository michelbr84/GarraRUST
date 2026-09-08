# Branch protection ruleset — `main`

## Fonte de verdade

**`docs/security/protect-main-ruleset.json` é a fonte versionada de verdade**
para a configuração da branch protection ruleset id `15901595` (`Protect main`)
no repositório [`michelbr84/GarraRUST`](https://github.com/michelbr84/GarraRUST).

A configuração efetiva no GitHub deve sempre coincidir com este JSON. Qualquer
divergência entre o JSON commitado e a ruleset live é considerada **drift** e
precisa ser reconciliada antes que mudanças adicionais sejam aplicadas (ver
seção [Drift detection](#drift-detection)).

## Regras enforçadas

A ruleset aplica os seguintes controles à branch `main`:

| Regra | Efeito |
|---|---|
| `deletion` | A branch `main` não pode ser deletada. |
| `non_fast_forward` | Force push em `main` é bloqueado. |
| `pull_request` | Toda mudança em `main` precisa passar por PR (review opcional, sem dismissal automático). |
| `required_status_checks` | Os 5 jobs CI abaixo precisam ficar verdes antes do merge. |
| `merge_queue` | Merge em `main` só acontece pela fila; cada entrada é testada em cima da anterior. |

Required status checks (todos obrigatórios), e em qual workflow cada um vive:

| Check | Workflow |
|---|---|
| `Format Check` | `.github/workflows/ci.yml` |
| `Clippy Linting` | `.github/workflows/ci.yml` |
| `Test (ubuntu-latest)` | `.github/workflows/ci.yml` |
| `Test (windows-latest)` | `.github/workflows/ci.yml` |
| `Security Gate (BOLA & Tenant Isolation)` | `.github/workflows/security-gate-bola.yml` |

O `Security Gate` foi adicionado à ruleset live em 2026-09-05 e não estava no
JSON versionado — drift reconciliado aqui, no sentido "o GitHub estava certo".

### Merge queue (desde 2026-09-08)

A fila substitui o `strict_required_status_checks_policy`, que passou a
`false` no mesmo commit. Os dois davam a mesma garantia — "o código testado
é o código que vai para `main`" — mas por caminhos opostos: o `strict`
**rejeitava** toda PR que não estivesse na cabeça de `main`, o que serializava
os merges (cada merge invalidava as demais PRs abertas, obrigando um
`update-branch` + rodada nova de CI por PR, ~25 min cada). A fila **constrói**
o merge: o GitHub cria um ref temporário `refs/heads/gh-readonly-queue/main/…`
com a PR já rebaseada sobre a entrada anterior, roda os 4 checks ali, e só
então mergeia. Duas PRs independentes entram juntas em vez de esperar uma
pela outra, e nada entra em `main` sem ter sido testado no estado final.

Parâmetros escolhidos:

| Parâmetro | Valor | Por quê |
|---|---|---|
| `merge_method` | `SQUASH` | é o método usado em todos os merges do repo; o título do squash vem do título da PR (por isso Conventional Commits no título) |
| `grouping_strategy` | `ALLGREEN` | uma entrada vermelha não derruba as demais do lote |
| `check_response_timeout_minutes` | `60` | o CI completo leva ~25 min; 60 dá folga para fila de runners sem travar a fila |
| `max_entries_to_build` / `max_entries_to_merge` | `5` | teto de runners paralelos gastos pela fila |
| `min_entries_to_merge` | `1` | uma PR sozinha não espera por companhia |
| `min_entries_to_merge_wait_minutes` | `5` | janela curta para agrupar PRs que chegam juntas |

**Pré-requisito no CI: todo workflow que hospeda um check obrigatório precisa
do trigger `merge_group:`.** Sem ele a fila espera para sempre por um check
que nunca é reportado, porque a fila roda num ref temporário
(`refs/heads/gh-readonly-queue/main/…`) que não dispara `pull_request` nem
`push`. Hoje são dois workflows, `ci.yml` e `security-gate-bola.yml`, e os
dois ganharam o trigger no mesmo commit desta mudança.

**Ao adicionar um check obrigatório novo, faça os três passos juntos:** o
`merge_group:` no workflow dele, a entrada no JSON versionado, e o `gh api PUT`.
Pular o primeiro trava a fila; pular o segundo cria drift — foi o que aconteceu
com o `Security Gate` entre 2026-09-05 e hoje, e só apareceu quando um merge
foi recusado com "5 of 5 required status checks are expected" enquanto o doc
dizia que eram 4.

`bypass_actors` está vazio — **ninguém** tem bypass automático. Mudanças em
`main` sempre passam pelo fluxo PR + CI verde.

## Como aplicar / atualizar

Quando esta ruleset precisar mudar (adicionar required check, alterar políticas
de revisão, etc.):

1. Editar `docs/security/protect-main-ruleset.json` neste repo.
2. Abrir PR com a alteração + justificativa no body.
3. Após o PR ser merged em `main`, aplicar no GitHub:

   ```bash
   gh api -X PUT \
     repos/michelbr84/GarraRUST/rulesets/15901595 \
     --input docs/security/protect-main-ruleset.json
   ```

   O token usado precisa ter escopos `repo` e `admin:org` (ou ser um PAT
   classic com permissões equivalentes).

4. Verificar que a aplicação foi bem-sucedida com a [Drift detection](#drift-detection)
   abaixo.

Nunca aplicar `gh api PUT` diretamente sem antes commitar o JSON correspondente
em `main` — isso introduziria drift no sentido oposto (live ≠ JSON versionado).

## Drift detection

Para verificar se a configuração live ainda coincide com o JSON versionado:

```bash
gh api repos/michelbr84/GarraRUST/rulesets/15901595 \
  | jq 'del(.id, .source_type, .source, .created_at, .updated_at, ._links, .current_user_can_bypass, .node_id)' \
  > /tmp/ruleset-live.json

jq -S . docs/security/protect-main-ruleset.json > /tmp/ruleset-versioned.json
jq -S . /tmp/ruleset-live.json                  > /tmp/ruleset-live-sorted.json

diff -u /tmp/ruleset-versioned.json /tmp/ruleset-live-sorted.json
```

Se o `diff` retornar saída, há drift. Resolução possível:

- **Drift legítimo no GitHub** (alguém editou via UI): atualizar o JSON neste
  repo via PR para refletir a mudança, ou reverter no GitHub via `gh api PUT`.
- **JSON adiantado**: aplicar `gh api PUT` para sincronizar a live config.

Não deixar drift por mais de uma sessão — quanto mais tempo a divergência
persiste, maior o risco de mudanças contraditórias se acumularem.

## Regra absoluta — nunca contornar via `--admin`

Quando um merge legítimo for bloqueado pela ruleset (CI vermelho, branch
desatualizada, etc.), **a resposta correta é corrigir o root cause, nunca usar
bypass administrativo.**

Comandos proibidos em fluxo normal:

- `gh pr merge --admin <PR>` — força merge ignorando required checks.
- `gh api PUT … --field bypass_actors='[…]'` — adiciona bypass actor temporário.
- Qualquer edição manual da ruleset via UI para "destravar" um PR específico.

Padrão correto para os bloqueios mais comuns:

| Sintoma | Root cause provável | Resposta correta |
|---|---|---|
| `head branch is not up to date` | Branch divergiu de `main` após push | Com a fila ligada isso deixa de bloquear (a fila rebaseia sozinha). Se aparecer mesmo assim: `gh pr update-branch <PR>` + esperar nova rodada CI |
| PR sai da fila com check vermelho | Bug genuíno, ou base quebrada | Ler o run do ref `gh-readonly-queue/main/…`, corrigir e re-enfileirar. Nunca desabilitar a fila para passar |
| 1+ status check vermelho | Bug genuíno na branch | Fix the bug, push, esperar verde |
| Auto-merge não disponível | Repo desabilitou auto-merge | `gh pr merge --squash <PR>` manualmente após CI verde |
| Required check faltando | CI não rodou aquele job | Investigar o workflow, não bypassar |

`--admin` está reservado apenas para emergências reais (ex.: precisa revertir
um deploy hotfix e a CI está down por causa de outage GitHub). Cada uso deve
ser registrado com justificativa explícita no PR body ou em uma issue de
incidente.

Esta regra está documentada também no memory persistente do harness em
`feedback_ruleset_drift_no_admin_bypass.md` e foi validada empiricamente em
2026-05-03 durante o merge do PR [#113](https://github.com/michelbr84/GarraRUST/pull/113):
o merge ficou bloqueado por "head branch is not up to date" e foi resolvido
via `gh pr update-branch` + nova rodada CI verde, **sem** usar `--admin`.

## Referências

- Ruleset live: [`michelbr84/GarraRUST` rulesets/15901595](https://github.com/michelbr84/GarraRUST/rules/15901595)
- Regra 3 de `CLAUDE.md`: "NUNCA force push para `main`".
- Memory persistente: `feedback_ruleset_drift_no_admin_bypass.md`.
- Precedente operacional: PR [#113](https://github.com/michelbr84/GarraRUST/pull/113)
  (2026-05-03) — caso real de bloqueio + resolução correta sem `--admin`.
