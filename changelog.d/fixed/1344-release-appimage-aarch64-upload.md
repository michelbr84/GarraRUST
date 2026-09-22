- **A release passa a subir o AppImage aarch64 da CLI (#1344).** O `release.yml`
  copiava `garraia-linux-aarch64.AppImage` para `release/` e o `SHA256SUMS`
  o listava, mas a lista `files:` do upload nunca teve a linha dele: na
  v0.4.4, primeira release a produzir o arquivo, so o `.sha256` subiu (pelo
  glob) e o asset foi anexado a mao depois, com o hash conferido contra o
  `SHA256SUMS`. A linha entrou, e `scripts/release/check_files.py` — que le o
  workflow e exige que todo nome copiado para `release/` case com um padrao
  do upload — roda no CI de todo PR, com testes proprios.
