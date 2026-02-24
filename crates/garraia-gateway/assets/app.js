// ===============================
// 1. Core State & Event Bus
// ===============================

const EventBus = {
  events: {},
  on(event, listener) {
    if (!this.events[event]) this.events[event] = [];
    this.events[event].push(listener);
  },
  emit(event, data) {
    if (this.events[event]) {
      this.events[event].forEach((listener) => listener(data));
    }
  },
};

const GarraState = {
  currentView: "chat",
  memories: [],
  logs: "",
  session: localStorage.getItem("garraia.session_id") || "",
  gatewayKey: localStorage.getItem("garraia.gateway_key") || "",
  selectedProvider: localStorage.getItem("garraia.provider") || "",
  authRequired: false,
  connected: false,
  agentThinking: false,
  providers: [],

  // Setters that emit events
  setSession(id) {
    this.session = id || "";
    if (this.session) {
      localStorage.setItem("garraia.session_id", this.session);
    } else {
      localStorage.removeItem("garraia.session_id");
    }
    EventBus.emit("state:session", this.session);
  },
  setGatewayKey(key) {
    this.gatewayKey = key;
    localStorage.setItem("garraia.gateway_key", key);
    EventBus.emit("state:gatewayKey", key);
  },
  setProvider(id) {
    this.selectedProvider = id;
    localStorage.setItem("garraia.provider", id);
    EventBus.emit("state:provider", id);
  },
  setConnected(status) {
    if (this.connected !== status) {
      this.connected = status;
      EventBus.emit("state:connection", status);
    }
  },
  setView(viewName) {
    if (this.currentView !== viewName) {
      this.currentView = viewName;
      EventBus.emit("state:view", viewName);
    }
  },
  setThinking(status) {
    if (this.agentThinking !== status) {
      this.agentThinking = status;
      EventBus.emit("state:thinking", status);
    }
  },
  setLogs(logs) {
    this.logs = logs;
    EventBus.emit("state:logs", logs);
  },
  appendLog(line) {
    this.logs += (this.logs ? "\n" : "") + line;
    EventBus.emit("state:logs", this.logs);
  },
  setMemories(memories) {
    this.memories = memories;
    EventBus.emit("state:memories", memories);
  }
};

// ===============================
// 2. DOM Elements
// ===============================
const dom = {
  chatEl: document.getElementById("chat"),
  inputEl: document.getElementById("input"),
  sendBtn: document.getElementById("send"),
  reconnectBtn: document.getElementById("reconnect"),
  refreshBtn: document.getElementById("refresh"),
  clearBtn: document.getElementById("clear"),
  connPill: document.getElementById("conn-pill"),
  themeToggleBtn: document.getElementById("theme-toggle"),
  sessionEl: document.getElementById("session"),
  apiStatusEl: document.getElementById("api-status"),
  sessionCountEl: document.getElementById("session-count"),
  channelListEl: document.getElementById("channel-list"),
  providerSelect: document.getElementById("provider-select"),
  providerStatus: document.getElementById("provider-status"),
  providerKeySection: document.getElementById("provider-key-section"),
  providerApiKey: document.getElementById("provider-api-key"),
  providerActivateBtn: document.getElementById("provider-activate"),
  authSection: document.getElementById("auth-section"),
  keyGatewayEl: document.getElementById("key-gateway"),
  authConnectBtn: document.getElementById("auth-connect"),
  updateBanner: document.getElementById("update-banner"),
  updateBannerText: document.getElementById("update-banner-text"),
  updateBannerClose: document.getElementById("update-banner-close"),

  // Memory UI
  memoryRefreshBtn: document.getElementById("memory-refresh-btn"),
  memorySearchBtn: document.getElementById("memory-search-btn"),
  memorySearchInput: document.getElementById("memory-search-input"),
  memoryList: document.getElementById("memory-list"),

  // Logs UI
  logsRefreshBtn: document.getElementById("logs-refresh-btn"),
  logsView: document.getElementById("logs-view"),

  // Nano agents widget
  nanoAgentsWidget: document.getElementById("nano-agents"),
  nanoTimeEl: document.getElementById("nano-time"),
};

// ===============================
// 3. I18N & Themes
// ===============================
document.addEventListener("DOMContentLoaded", async () => {
  const savedLang = localStorage.getItem("lang");
  const browserLang = navigator.language || "en-US";
  if (savedLang) await loadLocale(savedLang);
  else await loadLocale(browserLang.startsWith("pt") ? "pt-BR" : "en-US");
});

window.setLanguage = function (lang) {
  localStorage.setItem("lang", lang);
  loadLocale(lang);
};

const themeStorageKey = "garraia.ui.theme";
const themeOrder = ["light", "dark", "brasil"];

function getCurrentTheme() {
  const fromDom = document.documentElement.getAttribute("data-theme");
  if (fromDom && themeOrder.includes(fromDom)) return fromDom;
  const stored = localStorage.getItem(themeStorageKey);
  if (stored && themeOrder.includes(stored)) return stored;
  const prefersDark = window.matchMedia && window.matchMedia("(prefers-color-scheme: dark)").matches;
  return prefersDark ? "dark" : "light";
}

function applyTheme(theme, persist = true) {
  const selected = themeOrder.includes(theme) ? theme : "light";
  document.documentElement.setAttribute("data-theme", selected);
  if (persist) localStorage.setItem(themeStorageKey, selected);

  const btn = dom.themeToggleBtn;
  if (btn) {
    const nextTheme = theme === "light" ? "dark" : theme === "dark" ? "brasil" : "light";
    const key = `theme.${nextTheme}_mode`;
    btn.setAttribute("data-i18n", key);
    if (typeof window.t === "function") {
      btn.textContent = window.t(key) || key;
    }
  }
}

function cycleTheme() {
  const current = getCurrentTheme();
  const next = themeOrder[(themeOrder.indexOf(current) + 1) % themeOrder.length];
  applyTheme(next, true);
}

function initTheme() {
  applyTheme(getCurrentTheme(), false);
  if (dom.themeToggleBtn) {
    dom.themeToggleBtn.removeEventListener("click", cycleTheme);
    dom.themeToggleBtn.addEventListener("click", cycleTheme);
  }
}

// ===============================
// 4. View Router
// ===============================
const navItemsList = document.querySelectorAll(".nav-item");
const viewSections = document.querySelectorAll(".panel-view");

function initRouter() {
  navItemsList.forEach(item => {
    const viewName = item.id.replace("nav-", "");
    item.addEventListener("click", () => GarraState.setView(viewName));
  });

  EventBus.on("state:view", (viewName) => {
    // Update nav active states
    navItemsList.forEach(item => {
      if (item.id === `nav-${viewName}`) item.classList.add("active");
      else item.classList.remove("active");
    });

    // Update view visibility
    viewSections.forEach(section => {
      if (section.id === `view-${viewName}`) section.style.display = "";
      else section.style.display = "none";
    });

    // View-specific actions
    if (viewName === "memory") loadMemory();
    if (viewName === "logs") loadLogs();
  });
}

// ===============================
// 5. Memory & Logs Logic
// ===============================
let memoryAutoRefreshTimer = null;

async function loadMemory(query = "") {
  if (!dom.memoryList) return;
  dom.memoryList.innerHTML = `<p style="color: var(--text-secondary)">Loading...</p>`;
  try {
    const url = query ? `/api/memory/search?q=${encodeURIComponent(query)}` : `/api/memory/recent`;
    const res = await fetch(url, { headers: GarraState.gatewayKey ? { "X-Gateway-Key": GarraState.gatewayKey } : {} });
    if (!res.ok) throw new Error("Failed to fetch memories");
    const data = await res.json();
    GarraState.setMemories(data.memories || []);
  } catch (e) {
    dom.memoryList.innerHTML = `<p style="color: var(--warn-text)">Error: ${e.message}</p>`;
  }
}

EventBus.on("state:memories", (memories) => {
  if (!dom.memoryList) return;
  if (!memories || memories.length === 0) {
    dom.memoryList.innerHTML = `<p style="color: var(--text-secondary)">No memories found.</p>`;
    return;
  }
  dom.memoryList.innerHTML = memories.map(m => `
    <div style="background: var(--bg); padding: 12px; border-radius: 8px; border: 1px solid var(--panel-edge);">
      <div style="font-size: 0.8rem; color: var(--ink-soft); margin-bottom: 4px; display: flex; justify-content: space-between;">
        <span>${new Date(m.created_at).toLocaleString()}</span>
        <span>${m.role}</span>
      </div>
      <div style="color: var(--ink); white-space: pre-wrap; font-family: 'IBM Plex Mono', monospace; font-size: 0.85rem;">${m.content}</div>
    </div>
  `).join("");
});

EventBus.on("state:view", (viewName) => {
  if (viewName === "memory") {
    if (!memoryAutoRefreshTimer) {
      memoryAutoRefreshTimer = setInterval(() => {
        if (!dom.memorySearchInput || !dom.memorySearchInput.value.trim()) {
           loadMemory();
        }
      }, 5000);
    }
  } else {
    if (memoryAutoRefreshTimer) {
      clearInterval(memoryAutoRefreshTimer);
      memoryAutoRefreshTimer = null;
    }
  }
});

// Logs Logic
async function loadLogs() {
  if (!dom.logsView) return;
  try {
    const res = await fetch("/api/logs", { headers: GarraState.gatewayKey ? { "X-Gateway-Key": GarraState.gatewayKey } : {} });
    if (res.ok) {
      const data = await res.json();
      GarraState.setLogs(data.logs || "No logs available.");
    }
  } catch (e) {
    GarraState.setLogs(`Error: ${e.message}`);
  }
}

EventBus.on("state:logs", (logs) => {
  if (!dom.logsView) return;
  dom.logsView.textContent = logs;
  dom.logsView.scrollTop = dom.logsView.scrollHeight;
});

let logsAutoRefreshTimer = null;
EventBus.on("state:view", (viewName) => {
  if (viewName === "logs") {
    // Periodic polling to stream logs if not supported via WS natively
    if (!logsAutoRefreshTimer) {
       logsAutoRefreshTimer = setInterval(loadLogs, 2000);
    }
  } else {
    if (logsAutoRefreshTimer) {
      clearInterval(logsAutoRefreshTimer);
      logsAutoRefreshTimer = null;
    }
  }
});

if (dom.memoryRefreshBtn) {
  dom.memoryRefreshBtn.addEventListener("click", () => {
    if (dom.memorySearchInput) dom.memorySearchInput.value = "";
    loadMemory();
  });
}
if (dom.memorySearchBtn) {
  dom.memorySearchBtn.addEventListener("click", () => {
    if (dom.memorySearchInput) loadMemory(dom.memorySearchInput.value.trim());
  });
}
if (dom.memorySearchInput) {
  dom.memorySearchInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter") loadMemory(e.target.value.trim());
  });
}
if (dom.logsRefreshBtn) {
  dom.logsRefreshBtn.addEventListener("click", loadLogs);
}

// ===============================
// 6. Chat & WebSocket Logic
// ===============================
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
    ? '<span class="online">' + (window.t ? t("status.connected") : "Connected") + "</span>"
    : '<span class="offline">' + (window.t ? t("status.disconnected") : "Disconnected") + "</span>";
});

EventBus.on("state:session", (sessionId) => {
  if (!dom.sessionEl) return;
  if (sessionId) {
    dom.sessionEl.textContent = sessionId.slice(0, 8) + "...";
    dom.sessionEl.title = sessionId;
  } else {
    dom.sessionEl.textContent = window.t ? t("session.none") : "none";
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

function appendMessage(kind, text) {
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
    const lines = text.split("\n");
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
    appendMessage("sys", (window.t ? t("message.raw", { raw: raw }) : `[RAW] ${raw}`));
    return;
  }

  if (evt.session_id) GarraState.setSession(evt.session_id);

  switch (evt.type) {
    case "connected":
      if (evt.note) {
        appendMessage("sys", (window.t ? t("message.connected") : "Connected") + ` (${evt.note}).`);
      }
      refreshStatus();
      break;
    case "resumed":
      appendMessage("sys", window.t ? t("message.session_resumed", { count: evt.history_length ?? 0 }) : "Session resumed.");
      refreshStatus();
      break;
    case "message":
      appendOrUpdateStreamMessage("assistant", evt.content || (window.t ? t("message.empty_response") : "Empty response"));
      break;
    case "error":
      GarraState.setThinking(false);
      appendMessage("error", `${evt.code || "Error"}: ${evt.message || "Unknown error"}`);
      break;
    default:
      appendMessage("sys", (window.t ? t("message.event") : "Event") + " " + (evt.type || "unknown") + ": " + JSON.stringify(evt));
  }
}

function scheduleReconnect() {
  if (reconnectTimer) return;
  reconnectTimer = setTimeout(() => {
    reconnectTimer = null;
    connect();
  }, 2000);
}

function connect() {
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

function reconnectFresh() {
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

function sendMessage() {
  if (!dom.inputEl) return;
  const content = dom.inputEl.value.trim();
  if (!content) return;

  if (!socket || socket.readyState !== WebSocket.OPEN) {
    appendMessage("error", window.t ? t("error.not_connected") : "Not connected to server.");
    return;
  }

  appendMessage("user", content);
  GarraState.setThinking(true);
  
  const msg = { content };
  if (GarraState.selectedProvider) {
    msg.provider = GarraState.selectedProvider;
  }
  socket.send(JSON.stringify(msg));
  
  dom.inputEl.value = "";
  dom.inputEl.focus();
}

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

// ===============================
// 7. Gateway & Provider APIs
// ===============================
async function refreshStatus() {
  try {
    const r = await fetch("/api/status");
    const j = await r.json();
    if (dom.apiStatusEl) {
        dom.apiStatusEl.innerHTML = `<span class="status-dot dot-ok"></span>${j.status}`;
    }
    if (dom.sessionCountEl) dom.sessionCountEl.textContent = j.sessions;

    if (j.latest_version && j.version && dom.updateBanner) {
      const dismissed = sessionStorage.getItem("garraia.update_dismissed");
      if (dismissed !== j.latest_version) {
        if (dom.updateBannerText) {
          dom.updateBannerText.innerHTML = (window.t ? t("update.available", {
            version: j.version,
            latest: j.latest_version.replace(/^v/, "")
          }) : `Update available: ${j.latest_version}`).replace("garraia update", "<code>garraia update</code>");
        }
        dom.updateBanner.style.display = "";
      }
    } else if (dom.updateBanner) {
      dom.updateBanner.style.display = "none";
    }

    if (dom.channelListEl) {
      if (j.channels && j.channels.length > 0) {
        dom.channelListEl.innerHTML = j.channels
          .map((ch) => `<span class="channel-tag"><span class="status-dot dot-ok"></span>${ch}</span>`)
          .join("");
      } else {
        dom.channelListEl.innerHTML = '<span class="no-channels">' + (window.t ? t("none_configured") : "No channels") + "</span>";
      }
    }
  } catch {
    if (dom.apiStatusEl) dom.apiStatusEl.innerHTML = '<span class="status-dot dot-off"></span>' + (window.t ? t("status.unavailable") : "Unavailable");
    if (dom.sessionCountEl) dom.sessionCountEl.textContent = "-";
    if (dom.channelListEl) dom.channelListEl.innerHTML = '<span class="no-channels">-</span>';
  }
  loadProviders();
}

if (dom.refreshBtn) dom.refreshBtn.addEventListener("click", refreshStatus);
if (dom.updateBannerClose) {
  dom.updateBannerClose.addEventListener("click", () => {
    dom.updateBanner.style.display = "none";
    if (dom.updateBannerText) {
      const match = dom.updateBannerText.innerHTML.match(/v([\d.]+)\s/);
      if (match) sessionStorage.setItem("garraia.update_dismissed", match[1]);
    }
  });
}

async function loadProviders() {
  if (!dom.providerSelect) return;
  try {
    const r = await fetch("/api/providers");
    const j = await r.json();
    GarraState.providers = j.providers || [];

    dom.providerSelect.innerHTML = "";
    for (const p of GarraState.providers) {
      const opt = document.createElement("option");
      opt.value = p.id;
      opt.textContent = p.active ? p.display_name : p.display_name + (window.t ? t("provider.not_configured_suffix") : " (Not Configured)");
      dom.providerSelect.appendChild(opt);
    }

    const defaultProvider = GarraState.providers.find((p) => p.is_default);
    const saved = GarraState.selectedProvider || (defaultProvider ? defaultProvider.id : "");
    if (saved && [...dom.providerSelect.options].some((o) => o.value === saved)) {
      dom.providerSelect.value = saved;
    }
    updateProviderUI();
  } catch {
    dom.providerSelect.innerHTML = '<option value="">' + (window.t ? t("status.unavailable") : "Unavailable") + "</option>";
    if (dom.providerStatus) dom.providerStatus.textContent = "";
  }
}

function updateProviderUI() {
  if (!dom.providerSelect || !dom.providerStatus) return;
  const id = dom.providerSelect.value;
  const p = GarraState.providers.find((x) => x.id === id);
  if (!p) {
    dom.providerStatus.textContent = "";
    if (dom.providerKeySection) dom.providerKeySection.style.display = "none";
    return;
  }
  if (p.active) {
    const tag = p.is_default ? (window.t ? t("provider.active_default") : "Active Default") : (window.t ? t("provider.active") : "Active");
    dom.providerStatus.innerHTML = `<span class="status-dot dot-ok"></span>${tag}`;
    if (dom.providerKeySection) dom.providerKeySection.style.display = "none";
  } else {
    dom.providerStatus.innerHTML = '<span class="status-dot dot-off"></span>' + (window.t ? t("provider.not_configured") : "Not Configured");
    if (dom.providerKeySection) dom.providerKeySection.style.display = p.needs_api_key ? "" : "none";
  }
  GarraState.setProvider(id);
}

if (dom.providerSelect) dom.providerSelect.addEventListener("change", () => {
  updateProviderUI();
  const p = GarraState.providers.find((x) => x.id === dom.providerSelect.value);
  if (p && p.active) {
    fetch("/api/providers", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ provider_type: p.id, set_default: true }),
    }).then(() => loadProviders());
  }
});

if (dom.providerActivateBtn) {
  dom.providerActivateBtn.addEventListener("click", async () => {
    const id = dom.providerSelect.value;
    const key = dom.providerApiKey.value.trim();
    if (!key) return;

    dom.providerActivateBtn.textContent = window.t ? t("button.activating") : "Activating...";
    dom.providerActivateBtn.disabled = true;

    try {
      const r = await fetch("/api/providers", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ provider_type: id, api_key: key, set_default: true }),
      });
      const j = await r.json();
      if (r.ok) {
        dom.providerApiKey.value = "";
        appendMessage("sys", window.t ? t("message.provider_activated", { id: id }) : `Provider ${id} activated.`);
        await loadProviders();
      } else {
        appendMessage("error", j.message || (window.t ? t("error.provider_activation") : "Activation failed"));
      }
    } catch (e) {
      appendMessage("error", (window.t ? t("error.provider_activation") : "Activation failed") + " " + e);
    } finally {
      dom.providerActivateBtn.textContent = window.t ? t("button.save_activate") : "Save & Activate";
      dom.providerActivateBtn.disabled = false;
    }
  });
}

if (dom.keyGatewayEl) dom.keyGatewayEl.value = GarraState.gatewayKey;

if (dom.authConnectBtn) {
  dom.authConnectBtn.addEventListener("click", () => {
    GarraState.setGatewayKey(dom.keyGatewayEl.value.trim());
    reconnectFresh();
  });
}

// ===============================
// 8. Boot Initialization
// ===============================
async function boot() {
  initTheme();
  initRouter();
  
  // Set initial state from DOM / Storage
  EventBus.emit("state:session", GarraState.session);
  GarraState.setView("chat"); // Trigger initial router state
  
  requestAnimationFrame(() => {
    document.body.classList.add("ready");
  });
  
  refreshStatus();

  try {
    const r = await fetch("/api/auth-check");
    const j = await r.json();
    GarraState.authRequired = j.auth_required;
  } catch {
    GarraState.authRequired = false;
  }

  if (GarraState.authRequired) {
    if (dom.authSection) dom.authSection.style.display = "";
    if (GarraState.gatewayKey) {
      connect();
    } else {
      appendMessage("sys", window.t ? t("auth.api_key_required") : "Gateway API Key required.");
    }
  } else {
    if (dom.authSection) dom.authSection.style.display = "none";
    connect();
  }
}

// ===============================
// 9. Extra UI (Nano Agents)
// ===============================
function initNanoAgents() {
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

// Start
boot();
initNanoAgents();
