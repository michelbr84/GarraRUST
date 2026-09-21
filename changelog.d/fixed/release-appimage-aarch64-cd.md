- `release.yml`: o passo do AppImage aarch64 fazia `cd "$RUNNER_TEMP"` e
  depois usava caminhos relativos ao workspace (`artifacts/`, `packaging/`,
  `stage/`), entao so passava quando o binario aarch64 NAO existia; na v0.4.3
  — primeira release com o binario ARM64 presente — o `cp` falhou e o job
  `package-linux` caiu antes de publicar os `.deb`/`.rpm`/AppImage x86_64.
  O runtime agora baixa por caminho absoluto, sem mudar de diretorio.
