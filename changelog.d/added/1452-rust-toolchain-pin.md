- **`rust-toolchain.toml` fixa a toolchain em 1.95 para todo clone novo (#1452).**
  O workspace exige rustc >= 1.95 (job "MSRV check (1.95)" do CI), mas nada no
  repo fixava essa versao para desenvolvimento local ou CI de terceiros — um
  `rustup` sem override ficava na stable que ja estivesse instalada, mesmo
  abaixo do piso, e cada `cargo check`/`cargo test` falhava so depois, com um
  erro generico de dependencia em vez de dizer qual a versao certa. Com o
  arquivo na raiz, `rustup` troca de toolchain sozinho (baixando 1.95 se
  faltar) ao entrar no diretorio.
