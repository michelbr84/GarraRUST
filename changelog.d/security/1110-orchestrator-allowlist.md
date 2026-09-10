- **A allowlist do modo `orchestrator` passa a valer (#1110).** O modo
  declarava `allowed` com seis ferramentas (`bash`, `file_read`, `file_write`,
  `repo_search`, `web_search`, `web_fetch`), mas `whitelist_mode: false`
  fazia `ToolGate::permite` liberar qualquer ferramenta e a lista nunca era
  lida — no modo de maior orcamento do sistema (`max_tool_loops: 100`). A
  correcao e so fazer a lista valer: o proprio prompt do modo ja anuncia
  exatamente essas seis, entao nenhuma capacidade que o modo prometia foi
  retirada. Um teste trava o comportamento nas duas direcoes.
