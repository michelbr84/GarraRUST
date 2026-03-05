'use strict';

// ── Sprite sheet config ─────────────────────────────────────────────────────
const FW = 160, FH = 200;

const STATES = {
  idle:     { row: 0, frames: 4, ms: 130 },
  thinking: { row: 1, frames: 6, ms: 110 },
  talking:  { row: 2, frames: 8, ms: 90  },
};

// ── DOM refs ────────────────────────────────────────────────────────────────
const parrotEl  = document.getElementById('parrot');
const bubbleEl  = document.getElementById('bubble');
const bubbleTxt = document.getElementById('bubble-text');
const inputBar  = document.getElementById('input-bar');
const queryEl   = document.getElementById('query');

// ── Animation engine ────────────────────────────────────────────────────────
let currentState = 'idle';
let frame = 0;
let animTimer = null;
let postTalkTimer = null;

function setState(s) {
  if (!STATES[s]) return;
  currentState = s;
  frame = 0;
  clearInterval(animTimer);
  const st = STATES[s];
  animTimer = setInterval(() => {
    parrotEl.style.backgroundPosition = `-${frame * FW}px -${st.row * FH}px`;
    frame = (frame + 1) % st.frames;
  }, st.ms);
}

// ── Speech bubble ───────────────────────────────────────────────────────────
let bubbleTimer = null;

function showBubble(text, duration = 7000) {
  clearTimeout(bubbleTimer);
  bubbleEl.classList.remove('hidden', 'bubble-hide');
  bubbleTxt.textContent = text.length > 280 ? text.slice(0, 277) + '…' : text;

  if (duration > 0) {
    bubbleTimer = setTimeout(() => {
      bubbleEl.classList.add('bubble-hide');
      setTimeout(() => bubbleEl.classList.add('hidden'), 350);
    }, duration);
  }
}

function hideBubble() {
  clearTimeout(bubbleTimer);
  bubbleEl.classList.add('bubble-hide');
  setTimeout(() => bubbleEl.classList.add('hidden'), 350);
}

function appendBubble(chunk) {
  clearTimeout(bubbleTimer);
  bubbleEl.classList.remove('hidden', 'bubble-hide');
  const current = bubbleTxt.textContent;
  bubbleTxt.textContent = (current + chunk).slice(-280);
}

// ── Input bar ───────────────────────────────────────────────────────────────
let inputVisible = false;

function showInput() {
  if (inputVisible) return;
  inputVisible = true;
  inputBar.classList.remove('hidden', 'input-hide');
  inputBar.classList.add('input-show');
  setTimeout(() => queryEl.focus(), 40);
}

function hideInput() {
  if (!inputVisible) return;
  inputVisible = false;
  inputBar.classList.remove('input-show');
  inputBar.classList.add('input-hide');
  setTimeout(() => {
    inputBar.classList.add('hidden');
    inputBar.classList.remove('input-hide');
    queryEl.value = '';
  }, 250);
}

// Expose for Alt+G Tauri shortcut to also trigger input toggle
window.__garraToggleInput = () => { inputVisible ? hideInput() : showInput(); };

queryEl.addEventListener('keydown', e => {
  if (e.key === 'Enter') {
    const text = queryEl.value.trim();
    hideInput();
    if (text) sendMessage(text);
  } else if (e.key === 'Escape') {
    hideInput();
  }
  e.stopPropagation();
});

// Prevent accidental drag when typing
queryEl.addEventListener('mousedown', e => e.stopPropagation());

// ── Click-through (Tauri command) ───────────────────────────────────────────
const invoke = window.__TAURI__?.core?.invoke ?? (() => Promise.resolve());

async function setClickThrough(ignore) {
  try { await invoke('set_ignore_mouse', { ignore }); } catch (_) {}
}

// ── WebSocket to GarraIA ────────────────────────────────────────────────────
const WS_URL = 'ws://localhost:3888/ws/parrot';
let ws = null;
let reconnectDelay = 2000;
let streamActive = false;

function connect() {
  try { ws = new WebSocket(WS_URL); } catch (_) { scheduleReconnect(); return; }

  ws.onopen = () => {
    reconnectDelay = 2000;
  };

  ws.onmessage = ev => {
    try {
      const msg = JSON.parse(ev.data);
      switch (msg.type) {
        case 'thinking':
          streamActive = false;
          clearTimeout(postTalkTimer);
          setState('thinking');
          hideBubble();
          break;

        case 'chunk':
          // Streaming chunk — accumulate in bubble while talking
          if (!streamActive) {
            streamActive = true;
            setState('talking');
            bubbleTxt.textContent = '';
            bubbleEl.classList.remove('hidden', 'bubble-hide');
          }
          appendBubble(msg.text ?? '');
          break;

        case 'response':
          streamActive = false;
          setState('talking');
          showBubble(msg.text ?? '', 8000);
          clearTimeout(postTalkTimer);
          postTalkTimer = setTimeout(() => setState('idle'), 5500);
          break;

        case 'error':
          streamActive = false;
          setState('idle');
          showBubble('\u26a0 ' + (msg.message ?? 'Erro desconhecido'), 5000);
          break;
      }
    } catch (_) {}
  };

  ws.onclose = () => { ws = null; scheduleReconnect(); };
  ws.onerror = () => { ws?.close(); };
}

function scheduleReconnect() {
  setTimeout(connect, reconnectDelay);
  reconnectDelay = Math.min(reconnectDelay * 2, 30000);
}

function sendMessage(text) {
  if (!ws || ws.readyState !== WebSocket.OPEN) {
    showBubble('Garra offline \u2014 inicie o GarraIA primeiro', 4000);
    return;
  }
  setState('thinking');
  hideBubble();
  ws.send(JSON.stringify({ type: 'message', text }));
}

// ── Public API ──────────────────────────────────────────────────────────────
window.__garra = { setState, showBubble, hideBubble, sendMessage, showInput, hideInput };

// ── Keyboard fallback (in case Tauri event bridge isn't ready yet) ──────────
window.addEventListener('keydown', e => {
  if (e.altKey && e.key.toLowerCase() === 'g') {
    e.preventDefault();
    window.__garraToggleInput?.();
  }
});

// ── Boot ────────────────────────────────────────────────────────────────────
setState('idle');
connect();
