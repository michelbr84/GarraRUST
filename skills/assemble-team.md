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

## Seleção por risco

| Risco | Exemplos | Time |
|-------|----------|------|
| R0 | typo, link quebrado, changelog | DocWriter |
| R1 | bug localizado, warning de clippy | Analyst + Implementer + Tester |
| R2 | lógica interna, refactor, teste novo | + Reviewer |
| R3 | API pública, schema, migration, dependência, CI | + revisão reforçada |
| R4 | auth, JWT, crypto, RLS, secrets, SSRF, upload | + **Security obrigatório** |

**Em R4 o Security pede o controle e o Reviewer muta o controle.** Não basta o
`security-auditor` exigir a correção e o Implementer aplicá-la: o
`code-reviewer` remove o controle e prova que algum teste fica **vermelho**.

Sem esse segundo passo, "o fix foi aplicado" é uma *alegação*, não uma
evidência — que é exatamente o que o `team-coordinator` já é instruído a não
aceitar.

Caso real, medido: numa auditoria R4 o Security exigiu `env_clear()` no spawn
do processo filho e uma allowlist de ambiente. Os dois foram aplicados. O
Reviewer depois apagou o `env_clear()` e **93 de 93 testes seguiram verdes** —
o teste afirmava o conteúdo de duas constantes, nunca que o ambiente era de
fato limpo. O controle que a auditoria exigiu não restringia o comportamento
que dizia restringir. Ninguém tinha pedido o pino.
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
