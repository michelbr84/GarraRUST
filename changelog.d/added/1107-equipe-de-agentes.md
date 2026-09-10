- A equipe de agentes do `.claude/agents/` passa a ter um modelo por funcao em vez
  de um unico modelo para todos: `team-coordinator` (tencent/hy4-preview) coordena e
  decide sem executar trabalho operacional, `repo-analyst` e `test-engineer`
  (deepseek/deepseek-v4-flash-0731) investigam e testam, `implementer`
  (z-ai/glm-5.3-flash) implementa, `code-reviewer` e `security-auditor`
  (openai/gpt-5.6-luna) julgam com independencia em relacao a quem escreveu, e
  `doc-writer` fecha o ciclo com documentacao e higiene do repositorio.
- `assemble-team` e a nova skill `repo-autopilot` selecionam o time por risco em vez
  de convocar os sete agentes sempre: R0 e so documentacao, R1 e bug pequeno, R2 e
  logica interna com revisao, R3 e API/DB/dependencia com revisao reforcada, R4 e
  auth/seguranca/cripto com auditor obrigatorio, R5 e release/secrets/destrutivo e
  escala para aprovacao humana.
- Todo agente devolve o mesmo contrato de status (`PASS`, `FAIL`, `BLOCKED`,
  `NEEDS_CHANGES`, `NEEDS_HUMAN`) com risco e recomendacao, para o coordenador
  arbitrar por sinal em vez de interpretar texto longo. `MERGE_READY` passa a exigir
  causa raiz encontrada, mudanca minima, teste de regressao fmt/check/clippy/test,
  revisor independente, auditor de seguranca quando aplicavel e docs atualizadas —
  CI verde sozinho nao e mais suficiente.
