- **Testes de guarda para os 2 mutantes sobreviventes de `purge_pool.rs` no piloto de mutacao (garraia-auth).**
  O `PurgePool` recusava role nao-purge e redigia credenciais no `Debug`, mas nenhum
  teste cobria esses dois ramos: o mutante `!= -> ==` no guard de role e o
  `Debug for PurgePool -> {}` sobreviviam no piloto `cargo-mutants` (mutants.yml).
  O novo `tests/purge_pool_guard.rs` (feature `test-support`) mata os dois.
