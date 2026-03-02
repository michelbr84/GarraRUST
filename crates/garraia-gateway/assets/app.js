import { GarraState } from './state.js';
import { EventBus } from './eventBus.js';
import { dom } from './dom.js';
import { initTheme } from './theme.js';
import { initRouter } from './router.js';
import { connect, initChat, initNanoAgents, appendMessage } from './views/chatView.js';
import { initApiView, refreshStatus } from './api.js';
import { initMemory } from './views/memoryView.js';
import { initLogs } from './views/logsView.js';
import { initModeSidebar } from './modeSidebar.js';

async function boot() {
  initTheme();
  initRouter();
  initMemory();
  initLogs();
  initChat();
  initApiView();
  initNanoAgents();
  initModeSidebar();
  
  // Set initial state from DOM / Storage
  EventBus.emit("state:session", GarraState.session);
  // Router handles initial view automatically now based on hash
  
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
      appendMessage("sys", window.t ? window.t("auth.api_key_required") : "Gateway API Key required.");
    }
  } else {
    if (dom.authSection) dom.authSection.style.display = "none";
    connect();
  }
}

document.addEventListener("DOMContentLoaded", () => {
    boot();
});
