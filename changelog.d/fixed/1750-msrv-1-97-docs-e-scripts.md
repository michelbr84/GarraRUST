- **Docs e scripts voltam a bater com a MSRV 1.97 (#1750).** O #1731 (que
  substituiu o Dependabot #1714) subiu o `rust-version` do workspace para 1.97,
  mas README (en e pt-BR: badge e Quick Start), `CONTRIBUTING.md`,
  `scripts/setup-toolchain.sh` (fazia `rustup override set 1.96`, que quebra o
  build de quem segue o CONTRIBUTING), `scripts/build-installer.ps1`, a wiki
  publicada e docs mais antigas (algumas em 1.95 ou 1.92) ficaram para tras. Um
  teste novo em `garraia-config` le o `rust-version` do `Cargo.toml` e confere
  cada espelho, para o proximo bump nao deixar nada atras. A convencao do
  `changelog.d/` passa a exigir o numero de uma issue que ja existe.
