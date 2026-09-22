- **As crates mortas `garraia-tools` e `garraia-runtime` saem do workspace
  (#1226).** Nenhum caminho alcancavel as usava: o unico consumidor era codigo
  do gateway que nunca foi roteado (`runtime_handler.rs` e o campo
  `RuntimeSettings` do `AppState`, sem chamador). Elas carregavam um segundo
  executor com limites diferentes do `ExecutionBudget` real, o
  `ToolRegistry::execute_program` ja marcado `deprecated` e uma segunda
  `RepoSearchTool`/`ListDirTool` sem jail, um risco latente se alguem a
  ligasse. Nenhum binario, rota, chave de config ou asset de release muda.
  O workspace passa de 24 para 22 crates.
