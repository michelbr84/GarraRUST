import { EventBus } from './eventBus.js';

export const GarraState = {
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
  loading: { logs: false, memory: false, api: false },
  errors: { logs: null, memory: null, api: null },

  setLoading(key, status) {
    this.loading[key] = status;
    EventBus.emit(`state:loading:${key}`, status);
  },
  setError(key, msg) {
    this.errors[key] = msg;
    EventBus.emit(`state:error:${key}`, msg);
  },

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
