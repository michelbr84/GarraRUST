import { GarraState } from '../state.js';
import { EventBus } from '../eventBus.js';
import { dom } from '../dom.js';
import { refreshStatus } from '../api.js';

let socket = null;
let reconnectTimer = null;
let nanoTimerInterval = null;
let nanoElapsed = 0;
let thinkingTimeout = null;

function wsUrl() {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  const base = `${proto}//${location.host}/ws`;
  return GarraState.gatewayKey ? `${base}?token=${encodeURIComponent(GarraState.gatewayKey)}` : base;
}

EventBus.on("state:connection", (isConnected) => {
  if (!dom.connPill) return;
  dom.connPill.innerHTML = isConnected
    ? '<span class="online">' + (window.t ? window.t("status.connected") : "Connected") + "</span>"
    : '<span class="offline">' + (window.t ? window.t("status.disconnected") : "Disconnected") + "</span>";
});

EventBus.on("state:session", (sessionId) => {
  if (!dom.sessionEl) return;
  if (sessionId) {
    dom.sessionEl.textContent = sessionId.slice(0, 8) + "...";
    dom.sessionEl.title = sessionId;
  } else {
    dom.sessionEl.textContent = window.t ? window.t("session.none") : "none";
    dom.sessionEl.title = "";
  }
});

EventBus.on("state:thinking", (isThinking) => {
  const widget = dom.nanoAgentsWidget;
  const timeEl = dom.nanoTimeEl;
  if (!widget) return;

  if (isThinking) {
    widget.style.display = "inline-flex";
    if (!nanoTimerInterval) {
      nanoElapsed = 0;
      if (timeEl) timeEl.textContent = "0s";
      nanoTimerInterval = setInterval(() => {
        nanoElapsed++;
        if (timeEl) timeEl.textContent = nanoElapsed + "s";
      }, 1000);
    }
  } else {
    widget.style.display = "none";
    if (nanoTimerInterval) {
      clearInterval(nanoTimerInterval);
      nanoTimerInterval = null;
    }
    if (thinkingTimeout) {
      clearTimeout(thinkingTimeout);
      thinkingTimeout = null;
    }
  }
});

function resetThinkingDebounce() {
  GarraState.setThinking(true);
  if (thinkingTimeout) clearTimeout(thinkingTimeout);
  thinkingTimeout = setTimeout(() => {
    GarraState.setThinking(false);
  }, 1500);
}

export function appendMessage(kind, text) {
  if (!dom.chatEl) return;
  const div = document.createElement("div");
  div.className = `msg ${kind}`;
  div.textContent = text;
  dom.chatEl.appendChild(div);
  dom.chatEl.scrollTop = dom.chatEl.scrollHeight;
}

function appendOrUpdateStreamMessage(role, text) {
  let isStreamChunk = false;
  let parsedContent = "";
  try {
    const lines = text.split("\\n");
    for (const line of lines) {
      if (line.trim().startsWith("{") && line.trim().endsWith("}")) {
        const data = JSON.parse(line);
        if (data.content !== undefined) {
          isStreamChunk = true;
          parsedContent += data.content;
        }
      }
    }
  } catch (e) {}

  if (!isStreamChunk) {
    appendMessage(role, text);
    GarraState.setThinking(false);
    return;
  }

  resetThinkingDebounce();
  if (!dom.chatEl) return;
  const msgs = dom.chatEl.querySelectorAll(".msg.assistant");
  if (msgs.length > 0) {
    const lastMsg = msgs[msgs.length - 1];
    lastMsg.textContent += parsedContent;
    dom.chatEl.scrollTop = dom.chatEl.scrollHeight;
  } else {
    appendMessage(role, parsedContent);
  }
}

function handleServerEvent(raw) {
  let evt;
  try {
    evt = JSON.parse(raw);
  } catch {
    appendMessage("sys", (window.t ? window.t("message.raw", { raw: raw }) : `[RAW] ${raw}`));
    return;
  }

  if (evt.session_id) GarraState.setSession(evt.session_id);

  switch (evt.type) {
    case "connected":
      if (evt.note) {
        appendMessage("sys", (window.t ? window.t("message.connected") : "Connected") + ` (${evt.note}).`);
      }
      refreshStatus();
      break;
    case "resumed":
      appendMessage("sys", window.t ? window.t("message.session_resumed", { count: evt.history_length ?? 0 }) : "Session resumed.");
      refreshStatus();
      break;
    case "message":
      appendOrUpdateStreamMessage("assistant", evt.content || (window.t ? window.t("message.empty_response") : "Empty response"));
      break;
    case "error":
      GarraState.setThinking(false);
      appendMessage("error", `${evt.code || "Error"}: ${evt.message || "Unknown error"}`);
      break;
    case "log":
      GarraState.appendLog(`[${evt.level || "INFO"}] ${evt.message}`);
      break;
    default:
      appendMessage("sys", (window.t ? window.t("message.event") : "Event") + " " + (evt.type || "unknown") + ": " + JSON.stringify(evt));
  }
}

function scheduleReconnect() {
  if (reconnectTimer) return;
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    connect();
  }, 2000);
}

export function connect() {
  if (socket && (socket.readyState === WebSocket.OPEN || socket.readyState === WebSocket.CONNECTING)) {
    return;
  }
  socket = new WebSocket(wsUrl());
  socket.onopen = () => {
    GarraState.setConnected(true);
    if (GarraState.session) {
      socket.send(JSON.stringify({ type: "resume", session_id: GarraState.session }));
    } else {
      socket.send(JSON.stringify({ type: "init" }));
    }
  };
  socket.onmessage = (ev) => handleServerEvent(ev.data);
  socket.onclose = () => {
    GarraState.setConnected(false);
    scheduleReconnect();
  };
  socket.onerror = () => {
    GarraState.setConnected(false);
  };
}

export function reconnectFresh() {
  if (reconnectTimer) {
    clearTimeout(reconnectTimer);
    reconnectTimer = null;
  }
  if (socket) {
    socket.onclose = null;
    try { socket.close(); } catch {}
    socket = null;
  }
  GarraState.setConnected(false);
  connect();
}

export function sendMessage() {
  if (!dom.inputEl) return;
  const content = dom.inputEl.value.trim();
  if (!content) return;

  if (!socket || socket.readyState !== WebSocket.OPEN) {
    appendMessage("error", window.t ? window.t("error.not_connected") : "Not connected to server.");
    return;
  }

  appendMessage("user", content);
  GarraState.setThinking(true);
  
  const msg = { content };
  if (GarraState.selectedProvider) {
    msg.provider = GarraState.selectedProvider;
  }
  if (dom.providerModelInput && dom.providerModelInput.value.trim()) {
    msg.model = dom.providerModelInput.value.trim();
  }
  socket.send(JSON.stringify(msg));
  
  dom.inputEl.value = "";
  dom.inputEl.focus();
}

export function initChat() {
  if (dom.sendBtn) dom.sendBtn.addEventListener("click", sendMessage);
  if (dom.inputEl) {
    dom.inputEl.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        sendMessage();
      }
    });
  }
  if (dom.reconnectBtn) dom.reconnectBtn.addEventListener("click", reconnectFresh);
  if (dom.clearBtn) {
    dom.clearBtn.addEventListener("click", () => {
      if (dom.chatEl) dom.chatEl.innerHTML = "";
      GarraState.setSession("");
      reconnectFresh();
    });
  }
}

export function initNanoAgents() {
  const bg = document.getElementById("nano-bg");
  const grid = document.getElementById("nano-grid");
  const widget = document.getElementById("nano-agents");

  if (!bg || !grid) return;
  if (widget) widget.style.display = "none"; 

  const colors = [ "var(--brand)", "var(--brand-2)", "var(--online)", "var(--accent-line)" ];
  const bits = [];
  for (let i = 0; i < 6; i++) {
    const bit = document.createElement("div");
    bit.className = "nano-bit";
    const size = Math.random() * 2 + 1;
    bit.style.width = size + "px";
    bit.style.height = size + "px";
    bit.style.left = Math.random() * 100 + "%";
    bit.style.top = Math.random() * 100 + "%";
    bit.style.backgroundColor = colors[Math.floor(Math.random() * colors.length)];
    bit.style.opacity = 0;
    bg.appendChild(bit);
    bits.push({ el: bit, id: Math.random() });
  }

  setInterval(() => {
    bits.forEach((b) => {
      if (Math.random() > 0.95) {
        b.el.style.left = Math.random() * 100 + "%";
        b.el.style.top = Math.random() * 100 + "%";
        b.el.style.opacity = 0;
      } else {
        b.el.style.opacity = Math.sin(Date.now() / 1500 + b.id * 10) * 0.1 + 0.1;
      }
    });
  }, 250);

  const agentColors = [
    ["var(--brand)", "var(--brand-2)"],
    ["var(--online)", "#4ade80"],
    ["var(--warn-text)", "var(--warn-edge)"],
    ["var(--ink-soft)", "var(--ink)"],
  ];

  const agentEls = [];
  let positions = [0, 1, 2, 3];

  for (let i = 0; i < 4; i++) {
    const agent = document.createElement("div");
    agent.className = "nano-agent";
    for (let p = 0; p < 4; p++) {
      const pixel = document.createElement("div");
      pixel.className = "nano-pixel";
      agent.appendChild(pixel);
    }
    grid.appendChild(agent);
    agentEls.push({ el: agent, colors: agentColors[i] });
  }

  setInterval(() => {
    agentEls.forEach((a) => {
      const pixels = a.el.children;
      for (let i = 0; i < pixels.length; i++) {
        pixels[i].style.backgroundColor = a.colors[Math.floor(Math.random() * a.colors.length)];
        pixels[i].style.opacity = 0.7 + Math.random() * 0.3;
      }
    });
  }, 700);

  function getCoords(index) {
    return { x: (index % 2) * 12, y: Math.floor(index / 2) * 12 };
  }

  function updatePositions() {
    agentEls.forEach((a, i) => {
      const pos = positions[i];
      const coords = getCoords(pos);
      a.el.style.transform = `translate(${coords.x}px, ${coords.y}px)`;
    });
  }

  updatePositions();
  setInterval(() => {
    const next = [...positions];
    next.unshift(next.pop());
    positions = next;
    updatePositions();
  }, 2700);
}
