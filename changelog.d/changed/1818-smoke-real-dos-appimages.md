- **Release executa os AppImages Linux antes de publicar (#1818).** A rc2 saiu
  verde sem que o AppImage do desktop publicado tivesse sido aberto uma vez. O
  job novo `smoke-appimages-linux` (e o `smoke-linux-appimage-real` do
  `desktop.yml`, na PR) roda `scripts/release/smoke-appimages-linux.sh` sobre
  os arquivos que vao para a release: o desktop sob Xvfb, sem Node no PATH,
  com o sidecar em `/api/health` na versao certa, a ponte do WhatsApp no Node
  privado, a janela do assistente aberta e o sidecar morrendo com o app; a CLI
  com `--version`, sem argumento saindo sozinha e o duplo clique abrindo
  terminal. Com o desktop presente, uma falha bloqueia o release.
