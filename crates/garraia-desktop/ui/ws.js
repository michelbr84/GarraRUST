'use strict';

// Cliente WebSocket compartilhado das webviews do Garra Desktop (overlay do
// papagaio e chat bar). As duas janelas falam o protocolo do /ws/parrot
// (crates/garraia-gateway/src/parrot_ws.rs) e compartilham a sessão fixa
// "parrot-desktop" — o histórico é um fio contínuo, não importa qual
// superfície enviou a mensagem.
//
// Uso:
//   const chat = GarraWS.connect({
//     onOpen()         {},
//     onThinking()     {},
//     onChunk(text)    {},  // deltas de streaming (gateway >= v0.3.5)
//     onResponse(text) {},  // texto final completo — sempre autoritativo
//     onError(message) {},
//     onClose()        {},
//   });
//   chat.send('olá');   // false quando o socket não está aberto
//   chat.isOpen();
(function () {
  // Porta 3888 é o default do config (resources/config.default.yml); o
  // gateway sobe como sidecar em localhost via src-tauri/src/gateway.rs.
  const WS_BASE = 'ws://localhost:3888/ws/parrot';

  // #1240: com `gateway.api_key` configurada, o handshake do /ws/parrot exige
  // a credencial (crates/garraia-gateway/src/parrot_ws.rs). `new WebSocket()`
  // não permite mandar header nenhum, então o token vai pela query string —
  // exatamente o formato que o Web Console já usa em webchat.html:
  // `?token=${encodeURIComponent(...)}`.
  //
  // A chave sai do MESMO config.yml que o gateway lê, via comando Tauri
  // `gateway_api_key` (src-tauri/src/commands.rs), que por sua vez usa o
  // `garraia_config::ConfigLoader` — o resolvedor de path do próprio gateway.
  // Fora do Tauri (ou sem chave configurada) o fallback é a URL nua, que é o
  // comportamento de sempre numa instalação sem `api_key`.
  //
  // O `invoke` é resolvido na hora da chamada, não no load: ws.js é o
  // primeiro script das duas janelas, e capturar `window.__TAURI__` cedo
  // demais congelaria `undefined` para sempre — o desktop voltaria a conectar
  // sem chave, que é exatamente a falha muda que esta correção fecha.
  function invoke(cmd) {
    const fn = window.__TAURI__?.core?.invoke;
    return fn ? fn(cmd) : Promise.resolve(null);
  }

  async function urlDoSocket() {
    let chave = '';
    try { chave = (await invoke('gateway_api_key')) || ''; } catch (_) {}
    return chave ? `${WS_BASE}?token=${encodeURIComponent(chave)}` : WS_BASE;
  }

  function connect(handlers) {
    const h = handlers || {};
    let ws = null;
    let reconnectDelay = 2000;

    async function open() {
      // A chave é relida a cada tentativa: quem configurar `gateway.api_key`
      // com o app já aberto reconecta sozinho, sem reiniciar o desktop.
      let url = WS_BASE;
      try { url = await urlDoSocket(); } catch (_) {}
      try { ws = new WebSocket(url); } catch (_) { scheduleReconnect(); return; }
      ws.onopen = () => { reconnectDelay = 2000; h.onOpen?.(); };
      ws.onmessage = ev => {
        try {
          const msg = JSON.parse(ev.data);
          switch (msg.type) {
            case 'thinking': h.onThinking?.(); break;
            case 'chunk':    h.onChunk?.(msg.text ?? ''); break;
            case 'response': h.onResponse?.(msg.text ?? ''); break;
            case 'error':    h.onError?.(msg.message ?? 'Erro desconhecido'); break;
          }
        } catch (_) {}
      };
      ws.onclose = () => { ws = null; h.onClose?.(); scheduleReconnect(); };
      ws.onerror = () => { ws?.close(); };
    }

    function scheduleReconnect() {
      setTimeout(open, reconnectDelay);
      reconnectDelay = Math.min(reconnectDelay * 2, 30000);
    }

    open();

    return {
      isOpen: () => !!ws && ws.readyState === WebSocket.OPEN,
      send(text) {
        if (!ws || ws.readyState !== WebSocket.OPEN) return false;
        ws.send(JSON.stringify({ type: 'message', text }));
        return true;
      },
    };
  }

  window.GarraWS = { connect };
})();
