- **CI ganha o job nao obrigatorio `Unit tests (windows-latest, macos-latest)`, que EXECUTA os testes unitarios (`--lib`) fora do Linux (#1697).**
  Os jobs `Test (windows-latest)`/`Test (macos-latest)` so compilavam a suite (`--no-run`)
  porque os runners nao tem Docker; mas o testcontainers so entra em alvos de integracao
  (`tests/`), nunca num alvo lib, entao as suites unitarias do workspace inteiro passam a
  rodar nos tres sistemas. No Windows o job novo tambem executa
  `cargo test -p garraia-agents --features mcp --lib` e o clippy correspondente com
  `-D warnings`. O job nasce fora dos checks obrigatorios, por decisao do dono: vermelho
  ali e sinal em toda PR, nao bloqueio; a promocao a obrigatorio fica para depois de uma
  sequencia de runs verdes em `main`. Fecha a lacuna que deixou 19 falhas Windows-only e
  3 macOS-only invisiveis por semanas.
