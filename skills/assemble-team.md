---
name: assemble-team
description: Monta e coordena uma equipe de agentes especializados para tarefas complexas. Dois modos - novo projeto (scaffold) ou projeto existente (análise + execução).
---

# Assemble Team

Monte uma equipe coordenada de agentes para executar uma tarefa complexa no GarraRUST.

## Modos de operação

### Modo 1: Novo módulo / feature grande
Equipe padrão: Architect → Implementer(s) → Tester → Reviewer → DocWriter

1. **Architect** analisa a tarefa e produz design doc (crates afetados, interfaces, tipos)
2. **Implementer(s)** executam em paralelo por crate (worktree isolation)
3. **Tester** escreve e roda testes após implementação
4. **Reviewer** revisa todo o diff (usa agent code-reviewer)
5. **DocWriter** atualiza documentação se API pública mudou

### Modo 2: Projeto existente / análise + correção
Equipe padrão: Analyst → Implementer(s) + Reviewer

1. **Analyst** examina o codebase, identifica issues, prioriza
2. **Implementer(s)** corrigem em paralelo (worktree isolation)
3. **Reviewer** valida todas as correções

## Regras de execução

- Máximo **7 teammates** por equipe
- **Reviewer é obrigatório** em qualquer equipe que edite código
- Agentes que editam arquivos devem usar **worktree isolation**
- Dependências respeitadas: schema → handlers → testes → docs
- Se um agente falhar, parar e reportar antes de continuar

## Comunicação entre agentes

- Task list compartilhada via TodoWrite
- SendMessage para comunicação direta entre agentes
- Arquivos compartilhados em `.claude/team-output/` (temporário)

## Output esperado

Relatório final com:
1. Composição da equipe (quem fez o quê)
2. Arquivos criados/modificados
3. Testes adicionados e status
4. Veredicto do Reviewer
5. Recomendações pendentes

Usage: /assemble-team <descrição da tarefa> [--mode new|existing]
