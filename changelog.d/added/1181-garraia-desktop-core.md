- **Crate `garraia-desktop-core`, o nucleo sem Tauri do Desktop Control Center
  (#1181, milestone M0).** `garraia-desktop` esta excluida de todos os gates
  obrigatorios de CI — o `build.rs` do Tauri exige GTK/webkit que os runners
  nao tem — e por isso nao tem cobertura automatizada nenhuma. A crate nova e
  o outro lado dessa fronteira: leva a logica que nao precisa de janela para um
  lugar que o CI compila, linta e testa. Tres modulos: `state` (ligado/
  desligado como estado puro, sem relogio nem I/O, separando o que o usuario
  pediu do que de fato acontece), `detect` (deteccao de agentes que e leitura e
  nunca execucao — um nome na PATH nao prova identidade, entao sem corroboracao
  a deteccao fica `Ambiguous`, com um teste varrendo o proprio fonte para
  garantir que nenhum caminho de execucao apareca depois) e `supervise`
  (launch/restart/kill extraidos do `gateway.rs` da casca Tauri, agora sem
  `unwrap` em lock, sem `sleep` por dentro e com o filho morrendo junto com o
  supervisor via `Drop`). Nenhuma crate a consome ainda: a migracao da casca e
  o `garraia desktop` da CLI sao dos milestones seguintes.
