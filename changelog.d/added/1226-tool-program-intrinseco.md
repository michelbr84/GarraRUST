- **`tool_program` intrinseco no `AgentRuntime`, com gate por passo (#1226 S-B).**
  O modelo pode encadear ate 16 chamadas de ferramenta num unico turno, sem
  voltar para o LLM entre passos — mas nao e um caminho paralelo: cada passo
  resolve pelo mesmo `find_tool` e passa pelo mesmo `dispatch_tool_call` do
  loop normal (recursivo, ainda um unico ponto de consulta ao `ToolGate` no
  fonte), entao um programa nunca alcanca ferramenta que o modo negaria fora
  dele, nem pula o orcamento por passo, a deteccao de loop ou os eventos de
  tool. Passo negado pelo gate ou que pede confirmacao humana encerra o
  programa ali, com os passos ja executados no relatorio. `tool_program`
  chamando `tool_program` e recusado (sem aninhamento). `"$var"` encadeia a
  saida de um passo para o proximo, mas so quando ela e um numero inteiro —
  substituicao vira `Number`, nunca texto bruto, ao contrario do prototipo
  `ToolRegistry::execute_program` (`garraia-tools`, `#[deprecated]` desde a
  #1226 S-E) que reusava qualquer string na integra. Teto agregado de 120s,
  alem do timeout por passo, contra um perfil com `GARRA_TOOL_TIMEOUT_SECS`
  generoso.
