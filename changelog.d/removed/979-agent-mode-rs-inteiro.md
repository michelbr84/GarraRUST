- **`garraia_agents::agent_mode` removido (496 linhas).** `AutoRouter`,
  `ToolPolicyEngine`, `LlmRouter`, `ModeProfileExt`, `ModeSelectionMethod` e
  `SessionModeMetadata` estavam todos re-exportados em `lib.rs` e nenhum tinha
  consumidor fora do proprio arquivo. A #979 pedia para conectar o
  `AutoRouter`/`ToolPolicyEngine` daqui ao runtime; era o par inferior. O
  roteador vivo — com pontuacao, folga minima e estagio de LLM — mudou de
  `garraia-gateway` para `garraia-agents`, que e onde o runtime alcanca (a CLI
  monta o proprio `AgentRuntime` e nao passa pelo gateway).
