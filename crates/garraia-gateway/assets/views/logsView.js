import { GarraState } from '../state.js';
import { EventBus } from '../eventBus.js';
import { dom } from '../dom.js';

export async function loadLogs() {
  if (!dom.logsView) return;

  GarraState.setLoading('logs', true);
  GarraState.setError('logs', null);
  dom.logsView.innerHTML = `<span style="color: var(--ink-soft)">Carregando logs...</span>`;

  try {
    const res = await fetch("/api/logs", { headers: GarraState.gatewayKey ? { "X-Gateway-Key": GarraState.gatewayKey } : {} });
    if (!res.ok) throw new Error(`Falha ao buscar logs (Status ${res.status})`);
    const data = await res.json();
    GarraState.setLogs(data.logs || "Sem logs disponiveis.");
  } catch (e) {
    GarraState.setError('logs', e.message);
    GarraState.setLogs(`Erro ao carregar logs:\n${e.message}\n\nO servidor backend pode estar indisponível ou ocorreu um erro de permissão.`);
  } finally {
    GarraState.setLoading('logs', false);
  }
}

EventBus.on("state:logs", (logs) => {
  if (!dom.logsView) return;
  dom.logsView.textContent = logs;
  dom.logsView.scrollTop = dom.logsView.scrollHeight;
});

EventBus.on("state:view", (viewName) => {
  if (viewName === "logs") {
    loadLogs();
  }
});

export function initLogs() {
  if (dom.logsRefreshBtn) {
    dom.logsRefreshBtn.addEventListener("click", loadLogs);
  }
}
