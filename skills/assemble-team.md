---
name: assemble-team
description: Monta e coordena a equipe de agentes do GarraRUST para uma tarefa complexa. Seleciona o time por risco (R0-R5) em vez de convocar todos, e roda o pipeline análise → implementação → teste → revisão → segurança → documentação.
---

# Assemble Team

Monte uma equipe coordenada de agentes para uma tarefa no GarraRUST.

**Primeiro classifique o risco. O risco escolhe o time.** Convocar os 7 agentes sempre é o modo mais caro e não é o melhor.

## Roster

| Papel | Agent | Modelo | Função |
|-------|-------|--------|--------|
| Coordinator | `team-coordinator` | anthropic/claude-fable-5.1 | planeja, delega, arbitra, decide |
| Analyst | `repo-analyst` | anthropic/claude-opus-5 | diagnóstico e plano |
| Implementer | `implementer` | anthropic/claude-opus-5 | escreve o código |
| Tester | `test-engineer` | anthropic/claude-opus-5 | prova que funciona |
| Reviewer | `code-reviewer` | anthropic/claude-opus-5 | julgamento independente |
| Security | `security-auditor` | anthropic/claude-opus-5 | superfície sensível |
| DocWriter | `doc-writer` | anthropic/claude-opus-5 | docs e higiene |

Roster em Claude desde 2026-09-14 (decisão do dono). Quem escreve não julga: a independência entre Implementer e Reviewer vem do contexto separado (agente, prompt e worktree distintos; o Reviewer lê o diff, nunca o relatório do Implementer), não de modelos distintos.

## Seleção por risco

| Risco | Exemplos | Time |
|-------|----------|------|
| R0 | typo, link quebrado, changelog | DocWriter |
| R1 | bug localizado, warning de clippy | Analyst + Implementer + Tester |
| R2 | lógica interna, refactor, teste novo | + Reviewer |
| R3 | API pública, schema, migration, dependência, CI | + revisão reforçada |
| R4 | auth, JWT, crypto, RLS, secrets, SSRF, upload | + **Security obrigatório** |
| R5 | release, secrets de CI, destrutivo, `install.sh`/`install.ps1` | **pare e escale ao humano** |

## Pipeline

```text
Analyst  →  diagnóstico + plano + risco
Implementer  →  worktree isolada + diff
Tester  →  fmt/check/clippy/test + teste de regressão
Reviewer  →  lê o diff real, não o relatório do Implementer
Security  →  somente R4
DocWriter  →  somente se API/setup/CHANGELOG mudou
Coordinator  →  sintetiza e decide
```

Ordem de dependência: **schema → handlers → testes → docs**.

## Regras de execução

- Máximo **7 teammates**. Prefira 3-5
- Todo agente que edita arquivos usa **worktree isolada** (`fix/<slug>`, `feat/<slug>`)
- Implementers em paralelo só em worktrees diferentes e sem arquivo em comum
- Qualquer `FAIL` de Tester, Reviewer ou Security bloqueia o avanço
- Divergência Tester vs Reviewer: Reviewer prevalece em qualidade de código, Tester em comportamento observável — mas FAIL de um bloqueia
- Se um agente falhar, pare e reporte antes de continuar

## Contrato de status

Todo agente devolve:

```yaml
status: PASS | FAIL | BLOCKED | NEEDS_CHANGES | NEEDS_HUMAN
summary: uma frase
findings: []
risk: R0..R5
recommendation: MERGE_READY | NEEDS_CHANGES | NEEDS_HUMAN
```

## Gate MERGE_READY

Exige todos: issue compreendida · causa raiz encontrada · mudança mínima · teste de regressão · `cargo fmt` · `cargo check` · `cargo clippy` · `cargo test` · Reviewer aprovado · Security aprovado se R4 · docs atualizadas se necessário · CI verde · nenhuma discussão pendente · nenhuma alteração não relacionada · nenhum TODO/debug/código morto.

**CI verde sozinho não é PR pronta.**

## Nunca

- force push em `main`; merge em `main` sem autorização explícita (o padrão é abrir PR e parar)
- deletar branch não confirmada como merged; fechar issue sem evidência
- editar `CHANGELOG.md` direto; remover `continue-on-error` de CI para ficar verde
- renomear assets de release; editar `install.sh` sem `install.ps1`

## Output esperado

1. Risco e composição do time (quem, com qual modelo, fez o quê)
2. Arquivos criados/modificados
3. Testes adicionados e status
4. Veredito do Reviewer e do Security (se R4)
5. Veredito final: MERGE_READY | NEEDS_CHANGES | NEEDS_HUMAN
6. Pendências

Usage: /assemble-team <descrição da tarefa>
