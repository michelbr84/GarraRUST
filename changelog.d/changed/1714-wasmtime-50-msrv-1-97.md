- **wasmtime 49.0.4 -> 50.0.0 e MSRV 1.96 -> 1.97 (#1714).** A arvore do wasmtime
  50 (cranelift 0.137.0, pulley/wiggle 50.0.0) declara `rustc 1.97.0`, entao o
  `rust-version` do workspace e o job `MSRV check` sobem juntos, no mesmo padrao
  dos cinco bumps anteriores; sem migracao de API em `garraia-plugins`. Quem
  compila do fonte precisa de Rust 1.97 ou mais novo.
