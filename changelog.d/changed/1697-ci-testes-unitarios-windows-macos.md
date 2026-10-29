- **CI executa os testes unitarios (`--lib`) tambem no Windows e no macOS (#1697).**
  Os jobs `Test (windows-latest)` e `Test (macos-latest)` so compilavam a suite
  (`--no-run`), porque os runners nao tem Docker; mas o testcontainers so entra em
  alvos de integracao (`tests/`), nunca num alvo lib, entao as suites unitarias do
  workspace inteiro passam a rodar nos tres sistemas. No Windows o job tambem executa
  `cargo test -p garraia-agents --features mcp --lib` e o clippy correspondente com
  `-D warnings`, em vez de so `cargo check`. Fecha a lacuna que deixou 19 falhas
  Windows-only invisiveis por semanas.
