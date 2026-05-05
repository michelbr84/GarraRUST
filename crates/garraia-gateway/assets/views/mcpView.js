import { GarraState } from '../state.js';
import { EventBus } from '../eventBus.js';
import { escapeHtml } from '../utils.js';

async function loadMcps() {
  const list = document.getElementById('mcps-list');
  if (!list) return;

  list.innerHTML = '<p class="text-secondary">Carregando MCPs...</p>';

  try {
    const headers = GarraState.gatewayKey
      ? { 'X-Gateway-Key': GarraState.gatewayKey }
      : {};
    const res = await fetch('/api/mcp', { headers });
    if (!res.ok) throw new Error(`Status ${res.status}`);
    const data = await res.json();
    const servers = data.servers || [];

    if (servers.length === 0) {
      list.innerHTML = '<p class="text-secondary">Nenhum servidor MCP configurado.</p>';
      return;
    }

    list.innerHTML = servers
      .map(s => `
        <div class="card">
          <div class="card-header">
            <h3 class="card-title">${escapeHtml(s.name)}</h3>
            <span class="pill ${s.connected ? 'online' : 'offline'}">
              ${s.connected ? 'Conectado' : 'Desconectado'}
            </span>
          </div>
          <div class="card-meta">${escapeHtml(String(s.tools ?? 0))} ferramenta(s) disponível(is)</div>
        </div>
      `)
      .join('');
  } catch (e) {
    list.innerHTML = `<p class="text-secondary">Erro ao carregar MCPs: ${escapeHtml(e.message)}</p>`;
  }
}

export function initMcps() {
  EventBus.on('state:view', viewName => {
    if (viewName === 'mcps') loadMcps();
  });
}
