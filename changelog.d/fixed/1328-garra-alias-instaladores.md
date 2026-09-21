- **Instaladores passam a deixar o alias `garra` ao lado de `garraia` (#1328).**
  A CLI imprime "rode `garra start`", a wiki manda `garra init` e o README usa
  `garra` no quick start — o nome do `[[bin]]` do Cargo — mas o `install.sh`
  instalava so `garraia` (o asset da release, congelado pela regra 15) e o
  `install.ps1` so `garraia.exe`; num pod novo nenhum `garra` existia. Agora
  o `install.sh` cria `garra` como symlink RELATIVO para `garraia` no mesmo
  diretorio (Termux incluso, e pelo mesmo ramo `sudo` do binario), o
  `install.ps1` grava o shim `garra.cmd` (`"%~dp0garraia.exe" %*`, sobrevive a
  mover a pasta) e os pacotes `.deb`/`.rpm` trazem `/usr/bin/garra ->
  garraia`. Link e shim, nunca copia: o `garra update` continua trocando um
  unico binario — e, para isso valer tambem no macOS, o `update.rs` passa a
  canonizar `current_exe()` antes de gravar (`_NSGetExecutablePath` pode
  devolver o proprio link; no Linux `/proc/self/exe` ja vem resolvido e no
  Windows o shim executa `garraia.exe` direto). `rollback` e
  `--check-binaries` seguem o mesmo caminho. Em macOS, um binario v0.4.3 ou
  anterior ainda grava por cima do link: rode `garraia update` uma vez para
  cruzar essa versao. Um `garra` pre-existente que nao seja do instalador (arquivo
  real, `garra.exe`, ou `garra.cmd` de outro conteudo) e preservado com aviso;
  symlink obsoleto e reapontado; falha ao criar o alias e aviso, nunca aborta a
  instalacao. Testes espelhados em `tests/install_sh/garra_alias.sh` e
  `tests/install_ps1/garra_alias.ps1`.
