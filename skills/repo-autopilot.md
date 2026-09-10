---
name: repo-autopilot
description: Varredura autônoma do repositório GarraRUST — triagem de issues e PRs abertas, detecção de duplicadas e stale, abertura de PRs de correção e relatório de saúde. Roda sob o team-coordinator com paradas humanas em R5. Use para limpar e organizar o repositório.
---

# Repo Autopilot

Varredura autônoma do repositório. O `team-coordinator` dirige; os especialistas executam.

> **Padrão é propor, não destruir.** O autopilot abre PRs e fecha issues só com evidência. Merge em `main`, deleção de branch e release exigem humano.

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

Mantenha o `team-coordinator` (hy4) fora do trabalho operacional. Volume de tokens vai para DeepSeek (investigar/testar) e GLM (implementar); Luna julga; Hy4 decide. Se uma etapa puder ser feita por um modelo barato, ela deve ser.

Usage: /repo-autopilot [--issues #n,#m] [--max-parallel N] [--dry-run]
