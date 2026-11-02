- **Testes do binario `garra` e clippy da CLI e do gateway verdes em host Windows (#1762).**
  O pacote `garraia` nao tem alvo lib, entao o job `Unit tests` nunca rodava os testes do
  binario fora do Linux, e no Windows 7 deles falhavam por premissa de plataforma: varreduras
  de fonte que procuram `"\n}\n"` num checkout CRLF, a dica de remocao esperada como `sudo rm`
  (no Windows e `Remove-Item`), caminho do bloco Termux do `doctor` comparado como texto com
  `/` e a mensagem da recusa do sandbox fora de Unix. O `clippy --all-targets` do Windows
  tambem acusava import e helpers de teste mortos, usados so por testes `cfg(unix)`. As
  correcoes ficam nos testes (normalizacao de fim de linha, expectativa por plataforma,
  `cfg(unix)` nos helpers), sem mudanca de comportamento; o leg Windows do `Unit tests` passa
  a rodar `cargo test -p garraia --bin garra` e o clippy de `garraia` e `garraia-gateway`.
