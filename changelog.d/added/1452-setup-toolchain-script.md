- **`scripts/setup-toolchain.sh` fixa a toolchain Rust localmente (#1452).**
  O workspace exige rustc 1.95+, mas nada instalava/ativava essa versao
  automaticamente para quem clonava o repo — o sintoma era `cargo
  check`/`cargo test` falhando com "is not supported" ate rodar `rustup
  update` manualmente. Um `rust-toolchain.toml` commitado foi tentado (PR
  #1454) e quebrou os jobs de CI que fazem cross-compile com targets extras
  (Android, Windows ARM64): o arquivo tem precedencia sobre a toolchain que
  `dtolnay/rust-toolchain` acabou de instalar com esses targets. O script
  evita isso: usa `rustup override set`, que grava a preferencia em
  `~/.rustup/settings.toml` (no HOME de quem roda), nunca em um arquivo do
  repo, entao o CI nao ve nem herda a mudanca.
