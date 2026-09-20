- **Ferramenta de servidor MCP ignorava a whitelist do modo do agente** (#1264).
  O `ToolGate::permite` isentava qualquer nome que contivesse o separador `__`
  — a whitelist era consultada, e a isencao nao: modo somente-leitura
  (`search`/`architect`/`debug`/`review`/`edit`) deixava passar ferramenta de
  escrita de um servidor MCP conectado. Agora a permissao e **declarada**:
  `whitelist_mode` vale para MCP, e a sintaxe `servidor/*` na `allowed`
  libera o servidor inteiro (`meu-servidor/*` cobre `meu-servidor__<qualquer
  nome>`); nome completo libera so aquela ferramenta; `denied` continua
  vencendo tudo, prefixo incluso. `whitelist_mode = true` com `allowed` vazia
  continua **permitindo tudo** (opcao (b), compatibilidade) — mas o runtime
  emite aviso por turno nesse caso, e o aviso de ferramenta MCP escondida
  diz o nome e a sintaxe que libera. Os testes exercitam o portao que o
  runtime monta de verdade (`ModeProfile::from_custom` + `ToolGate::
  para_o_turno`) e o turno completo do runtime; a mutacao da isencao de volta
  derruba 3 testes.
