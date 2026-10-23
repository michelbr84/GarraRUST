- **Runtime Node.js privado empacotado no instalador do Garra Desktop (#1636, fatia C).**
  Os instaladores do desktop (MSI/NSIS no Windows, `.deb`/AppImage no Linux,
  `.dmg`/`.app` no macOS) agora trazem o runtime Node privado e a ponte
  pre-construida do WhatsApp (`node_modules` + `THIRD_PARTY_NOTICES.txt` +
  `manifest.json`), para que o usuario nao precise instalar Node, npm ou usar
  terminal, e nenhuma dependencia seja baixada na primeira execucao. Os scripts
  fonte-unica de build (`build-installer.ps1`, `build-desktop-linux.sh`,
  `build-desktop-macos.sh`) montam o bundle da fatia B em
  `crates/garraia-desktop/src-tauri/node-runtime/` antes do `cargo tauri build`,
  e o `tauri.conf.json` mapeia esse diretorio para os resources do Tauri. O
  `node/` e o `bridge-whatsapp/` ficam exatamente num dos caminhos do contrato
  do resolver (fatia A), relativos ao sidecar `garraia`: ao lado do exe no
  Windows, em `Contents/Resources/` no macOS e em `/usr/lib/garraia-desktop/` no
  `.deb`/AppImage. O Node privado e usado sempre por caminho explicito, sem
  mexer no `PATH` global nem substituir instalacoes existentes. LICENSE do Node
  e THIRD_PARTY_NOTICES vao no pacote.
- **Smoke de instalador em ambiente limpo no CI (#1636, fatia C).** O
  `desktop.yml` ganhou jobs `smoke-windows`/`smoke-linux`/`smoke-macos` que, por
  plataforma, pegam o instalador gerado, extraem a arvore num caminho com
  espaco, escondem todo Node global do `PATH` e rodam o sidecar instalado
  `garraia whatsapp self-test --json`, exigindo `node_origin == privado`,
  `self_test == ok` e o carregamento de baileys + pino + sharp, alem de provar
  que o `node/` e o `bridge-whatsapp/node_modules` sao irmaos do sidecar no
  layout do contrato. Sem `continue-on-error`. A inicializacao pela GUI do
  desktop e o pareamento real do WhatsApp nao sao automatizados (dependem de
  sessao grafica e de intervencao externa) e estao marcados como tal em
  `docs/desktop.md`.
- **Novo `docs/desktop.md` (#1636, fatia C).** Documenta o layout por formato, o
  que o instalador traz, o que a desinstalacao remove e o que fica
  (`~/.garraia/whatsapp/` preservado), como o bundle e montado e o que nao e
  validado automaticamente.
