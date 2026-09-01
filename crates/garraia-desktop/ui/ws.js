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
  const WS_URL = 'ws://localhost:3888/ws/parrot';

  function connect(handlers) {
    const h = handlers || {};
    let ws = null;
    let reconnectDelay = 2000;

    function open() {
      try { ws = new WebSocket(WS_URL); } catch (_) { scheduleReconnect(); return; }
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
