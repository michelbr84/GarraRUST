- **1568 — Desktop: DMG no macOS, artefactos de updater assinados e `latest.json` publicado.** O desktop nao e vendavel como distribuicao empacotada: nao havia DMG no macOS,
o `tauri-plugin-updater` estava ligado com `pubkey` vazio e nenhum workflow
publicava `latest.json` — o botao de atualizar nunca funcionou. O item que
bloqueia o canal distribuidor (assinatura de codigo EV/OV) continua externo e
rastreado na propria issue.

- `tauri.conf.json`: `bundle.createUpdaterArtifacts: true` e `bundle.macOS.targets: ["app", "dmg"]` explicitos — sem eles o bundler nao emite o `.app.tar.gz` nem o DMG que o updater e o macOS precisam.
- `release.yml`: job `build-macos-desktop` (macos-latest) compila o DMG pelo mesmo `cargo tauri build`, e o job `release` passa a gerar e publicar `latest.json` assinado ao lado dos binarios — o endpoint que o `updater` do app ja consultava.
- `desktop.yml`: espelho em PR (`build-macos-bundles`) para que mudanca de desktop seja verificada no macOS antes de existir tag, mesma janela de drift que o workflow ja fecha para Windows e Linux.
- `docs/installation.md`: secao "Garra Desktop on macOS" e tabela consolidada "CLI vs Desktop — what each one supports", com o aviso de Gatekeeper (app nao assinado) espelhando o aviso de SmartScreen ja documentado para o Windows.

Refs #1568
