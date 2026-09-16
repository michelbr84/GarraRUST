---
name: repo-autopilot
description: Varredura autônoma do repositório GarraRUST — triagem de issues e PRs abertas, detecção de duplicadas e stale, abertura de PRs de correção e relatório de saúde. Roda sob o team-coordinator com paradas humanas em R5. Use para limpar e organizar o repositório.
---

# Repo Autopilot

Varredura autônoma do repositório. O `team-coordinator` dirige; os especialistas executam.

> **Padrão é propor, não destruir.** O autopilot abre PRs e fecha issues só com evidência. Merge em `main`, deleção de branch e release exigem humano.


## Pré-requisitos e modo degradado

Este pipeline **depende de uma ferramenta de spawn de subagente**. Sem ela, a
sessão só consegue falar com agentes que já existem (`SendMessage`), e a
independência de julgamento que o roster inteiro pressupõe deixa de existir:
ela vem de **contexto separado** — agente, prompt e worktree distintos, com o
Reviewer lendo o diff e nunca o relatório do Implementer.

Verifique antes de prometer um time. Se não houver spawn:

- **R0–R1** seguem normalmente: são trabalho que uma sessão só faz e assina.
- **R2–R3** seguem com ressalva escrita no PR dizendo que a revisão foi feita
  pela mesma sessão que implementou.
- **R4+ não é mergeável por construção.** Não implemente. Diagnostique,
  documente com file:line, abra a issue e **escale ao humano**. Auto-revisão em
  superfície sensível é proibida pelo `CLAUDE.md`, e "eu reli com cuidado" não
  substitui contexto separado.

Descobrir isso no meio da rodada custa a rodada inteira — foi o atrito nº 1 do
dogfood registrado na #1228.

### Outros pré-requisitos que já morderam

- **`cargo` sem `CARGO_TARGET_DIR` exportado cria uma árvore de build inteira
  dentro do worktree.** A variável **não persiste entre chamadas de shell**:
  reexporte em *cada* comando. Numa rodada real isso produziu um `target/` de
  4,5 GB dentro de um worktree e levou o disco a 100%, travando todos os
  agentes ao mesmo tempo.
- **Um `target/` compartilhado entre worktrees serve artefato velho.** Já
  produziu duas leituras falsas: um "vermelho" que não existia e um erro de
  compilação logo depois de o clippy passar na mesma lib. Em medição que vai
  virar decisão, force rebuild.
- **`cargo test` não roda sem egresso** por causa do build script do
  `utoipa-swagger-ui`; use `SWAGGER_UI_DOWNLOAD_URL=file://...`. Ver a skill
  `steward`.

## Fases

### 1. Levantamento
```bash
git fetch --all --prune
gh issue list --state open --limit 100 --json number,title,labels,createdAt,updatedAt
gh pr list --state open --limit 100 --json number,title,headRefName,isDraft,mergeable,updatedAt
gh run list --limit 20 --json conclusion,name,headBranch
```

### 2. Diagnóstico — `repo-analyst`
Para cada issue e PR aberta, o Analyst produz: problema real, reproduzível hoje?, arquivos, crates, duplicadas, relacionados, risco R0-R5, prioridade P0-P3, plano mínimo.

Também detecta:
- issues já resolvidas (com commit/PR de evidência)
- issues duplicadas (mesma causa raiz → agrupar)
- PRs stale ou com conflito
- CI vermelho e por quê

### 3. Grafo de execução — `team-coordinator`
Classifique risco, monte o grafo de dependências, decida o que paraleliza. **Não paralelize arquivo em comum.**

### 4. Execução por item
Worktree isolada por issue. `implementer` → `test-engineer` → `code-reviewer` → (`security-auditor` se R4) → (`doc-writer` se API/setup/CHANGELOG).

### 5. Abertura de PR
Uma PR por item, com:
- o que mudou e por quê
- evidência de teste (comando + resultado)
- risco declarado
- fragmento em `changelog.d/`
- `Closes #n`

### 6. Relatório final

```text
Garra Repository Health

Issues
Before: N
After:  N

Pull Requests
Before: N
After:  N

CI
✓ Linux  ✓ Windows  ✓ macOS  ✓ Rust  ✓ Flutter

Security
Critical: 0   High: 0   Medium: 0

Warnings
cargo clippy: 0

Tests
N passed, 0 failed

Repository Health: NN/100
```

## Paradas humanas — R5

**Pare e escale** ao encontrar:

- release, tag ou bump de versão
- secrets de CI, chaves, tokens
- deleção de branch, force push, rewrite de histórico
- migração irreversível ou `DROP`/`TRUNCATE`
- mudança em `install.sh` / `install.ps1` (afeta toda instalação existente)
- renomeação de asset de release
- conflito de merge que exija julgamento de intenção
- issue cujo diagnóstico seja ambíguo ou irreproduzível

## Nunca

- merge autônomo em `main` sem autorização explícita nesta sessão
- delete branch não confirmada como merged
- feche issue sem commit/PR de evidência
- edite `CHANGELOG.md` direto
- desative gate de CI, nem remova `continue-on-error` para ficar verde
- force push

## Custo

Mantenha o `team-coordinator` (Fable 5.1) fora do trabalho operacional: ele decide, os demais (Opus 5) investigam, implementam, testam, julgam e documentam. Se uma etapa puder ser feita com menos contexto (um agente `Explore`, um grep), ela deve ser.

Usage: /repo-autopilot [--issues #n,#m] [--max-parallel N] [--dry-run]
