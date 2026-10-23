'use strict';
// Overlay do papagaio (#1636, F3): script externo para a CSP `script-src 'self'`.
// Rust (via evento Tauri) alterna a barra de input do overlay.
if (window.__TAURI__?.event) {
  window.__TAURI__.event.listen('garra-toggle-input', () => {
    window.__garraToggleInput?.();
  });
}
