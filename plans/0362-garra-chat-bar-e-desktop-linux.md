# Plan 0362 — Papagaio de volta, Garra Chat Bar e desktop Linux

- **Data:** 2026-09-01
- **Branch:** `claude/v0.3.4-garra-chat-bar-wyk27m`
- **Origem:** report do usuário — "nenhum executável da v0.3.4 funciona; o
  passarinho do canto inferior direito sumiu" + pedido de uma barra de chat
  flutuante (topo central, móvel, ocultável) compatível com Windows e Linux.
- **Antecedentes:** plans/0359 (MSI revival), plans/0361 (pacotes Linux da
  CLI), ADR 0015 (+ Amendment 2026-09-01).

## Diagnóstico

1. O "passarinho" é o papagaio do overlay (`crates/garraia-desktop`, janela
   `parrot`). O sprite `ui/assets/parrot-sprite.png` estava no `.gitignore` e
   nenhum build de CI rodava `gen_sprite.py` — a v0.3.4 (primeiro MSI de CI
   desde a v0.2.1) embarcou um papagaio invisível. Confirmado por
   `git ls-tree -r 56e0e9b`: sem `ui/assets/`.
2. Os assets Linux da v0.3.4 são a CLI; não existia app desktop para Linux em
   release nenhuma.
3. Secundários: panic sem monitor no `overlay.rs`, READMEs da raiz com o
   basename errado do sidecar, `settings.html` com `v0.2.0` fóssil e toggles
   mortos.

## Entregas (4 fatias, uma por commit)

1. `fix(desktop)` — sprite commitado (32KB) + `.gitignore` documentando a
   regra nova + job `assert-ui-assets` no `desktop.yml` (asset referenciado
   existe; sprite é PNG 1280x600 — sem byte-compare: zlib varia entre builds)
   + fallback sem panic no `overlay.rs` + limpezas de docs/settings.
2. `feat(desktop)` — **Garra Chat Bar** substitui o quick-chat: janela
   `chat-bar` no topo central, arrastável pelo grip, oculta por ✕/Esc/bandeja/
   `Ctrl+Space`, posição+visibilidade persistidas em `chat-bar.json`
   (`WindowEvent::Moved` com debounce de 500ms por contador de geração; flush
   no Exit). Expand/collapse do painel via comando Rust
   (`set_chat_bar_expanded`) — webview sem `allow-set-size`. Cliente WS
   extraído para `ui/ws.js` (era copiado-e-colado em 2 lugares); sessão
   `parrot-desktop` compartilhada com o papagaio.
3. `feat(gateway)` — streaming no `/ws/parrot`: deltas viram frames `chunk`
   (`process_message_streaming_with_agent_config` + `forward_deltas` genérico
   em `Sink<Message>`, drenagem concorrente — lição do `stream_turn`),
   `response` final autoritativo. 4 unit tests inline.
4. `feat(release)` — pacotes Linux do desktop, aditivos (regra 15):
   `garraia-desktop-linux-x86_64.{deb,AppImage}` via
   `scripts/build-desktop-linux.sh` (fonte única, irmão do
   `build-installer.ps1`), job `build-linux-bundles` no `desktop.yml` (gate de
   PR) e `build-linux-desktop` best-effort no `release.yml`. Deb com
   `Provides/Conflicts/Replaces: garraia` (sidecar em `/usr/bin/garraia`).

## Decisões de produto (confirmadas com o usuário)

- Papagaio E barra, ambos visíveis por padrão, ocultáveis separadamente.
- A barra substitui o Quick Chat (Ctrl+Space passa a ser dela).
- Release ganha os pacotes Linux do desktop.
- Streaming entra já.

## Limitações declaradas

- Wayland não honra always-on-top/skip-taskbar (documentado em
  `docs/installation.md`); X11 ok.
- O papagaio não anima para mensagens enviadas pela barra (o gateway responde
  só ao socket solicitante); broadcast fica para um follow-up.
- Débito do updater Tauri (`latest.json`/`pubkey`) permanece — fora de escopo,
  ver `docs/releasing.md`.

## Verificação

- Local: `cargo check/test/clippy -p garraia-gateway` (4 tests novos verdes),
  asserts do `assert-ui-assets` rodados na mão, `bash -n` no script novo,
  YAML dos workflows parseado.
- CI (PR): `desktop.yml` (assert-ui-assets + MSI/NSIS + deb/AppImage Linux),
  `ci.yml` (gateway).
- Release: próxima tag exercita `build-linux-desktop`; checklist manual de
  Windows/Ubuntu X11/Wayland no corpo do PR.
