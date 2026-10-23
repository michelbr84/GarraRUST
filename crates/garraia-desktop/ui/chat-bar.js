'use strict';
// Chat Bar (#1636, F3): script externo para a CSP `script-src 'self'`.
const queryEl = document.getElementById('query');
const panelEl = document.getElementById('panel');
const dotEl = document.getElementById('dot');
const invoke = window.__TAURI__?.core?.invoke ?? (() => Promise.resolve());

let streaming = false;

// O redimensionamento da janela vive no Rust (set_chat_bar_expanded):
// a webview não recebe a capability core:window:allow-set-size e as
// constantes de geometria ficam num lugar só (chat_bar.rs).
function expand() {
  document.body.classList.add('expanded');
  invoke('set_chat_bar_expanded', { expanded: true }).catch(() => {});
}
function collapse() {
  document.body.classList.remove('expanded');
  invoke('set_chat_bar_expanded', { expanded: false }).catch(() => {});
}

const chat = window.GarraWS.connect({
  onThinking() {
    streaming = false;
    dotEl.classList.add('thinking');
    expand();
    panelEl.textContent = 'Pensando...';
  },
  onChunk(text) {
    if (!streaming) {
      streaming = true;
      expand();
      panelEl.textContent = '';
    }
    panelEl.textContent += text;
    panelEl.scrollTop = panelEl.scrollHeight;
  },
  onResponse(text) {
    streaming = false;
    dotEl.classList.remove('thinking');
    expand();
    panelEl.textContent = text;
    panelEl.scrollTop = panelEl.scrollHeight;
  },
  onError(message) {
    streaming = false;
    dotEl.classList.remove('thinking');
    expand();
    panelEl.textContent = '⚠ ' + message;
  },
});

queryEl.addEventListener('keydown', e => {
  if (e.key === 'Enter') {
    const text = queryEl.value.trim();
    if (!text) return;
    if (chat.send(text)) {
      queryEl.value = '';
    } else {
      expand();
      panelEl.textContent = 'Garra offline — inicie o GarraIA primeiro';
    }
  } else if (e.key === 'Escape') {
    if (document.body.classList.contains('expanded')) {
      collapse();
    } else {
      invoke('hide_chat_bar').catch(() => {});
    }
  }
  e.stopPropagation();
});

// Evita arraste acidental enquanto digita
queryEl.addEventListener('mousedown', e => e.stopPropagation());

document.getElementById('close').addEventListener('click', () => {
  invoke('hide_chat_bar').catch(() => {});
});
document.getElementById('collapse').addEventListener('click', collapse);

// Quando a barra é reexibida (bandeja/atalho), o input volta ao foco.
window.addEventListener('focus', () => queryEl.focus());
