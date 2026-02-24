import { GarraState } from '../state.js';
import { EventBus } from '../eventBus.js';
import { dom } from '../dom.js';

let memoryAutoRefreshTimer = null;

export async function loadMemory(query = "") {
  if (!dom.memoryList) return;

  GarraState.setLoading('memory', true);
  GarraState.setError('memory', null);
  dom.memoryList.innerHTML = `<p style="color: var(--text-secondary)">Carregando memórias...</p>`;

  try {
    const url = query ? `/api/memory/search?q=${encodeURIComponent(query)}` : `/api/memory/recent`;
    const res = await fetch(url, { headers: GarraState.gatewayKey ? { "X-Gateway-Key": GarraState.gatewayKey } : {} });
    if (!res.ok) throw new Error(`Falha ao buscar memórias (Status ${res.status})`);
    const data = await res.json();
    GarraState.setMemories(data.memories || []);
  } catch (e) {
    GarraState.setError('memory', e.message);
    dom.memoryList.innerHTML = `
      <div style="padding: 16px; background: var(--error); border: 1px solid var(--error-edge); border-radius: 8px; color: var(--ink);">
        <strong>Erro ao carregar memórias:</strong><br/>
        <span style="font-family: 'IBM Plex Mono', monospace; font-size: 0.85rem">${e.message}</span>
      </div>`;
  } finally {
    GarraState.setLoading('memory', false);
  }
}

EventBus.on("state:memories", (memories) => {
  if (!dom.memoryList) return;
  if (!memories || memories.length === 0) {
    dom.memoryList.innerHTML = `<p style="color: var(--text-secondary)">Nenhuma memória encontrada.</p>`;
    return;
  }
  
  const formatter = new Intl.DateTimeFormat(navigator.language || 'pt-BR', {
    dateStyle: 'short', timeStyle: 'short'
  });

  dom.memoryList.innerHTML = memories.map(m => {
    const roleColor = m.role === 'user' ? 'var(--brand)' : m.role === 'assistant' ? 'var(--brand-2)' : 'var(--ink-soft)';
    const dateStr = formatter.format(new Date(m.created_at));
    return `
      <div style="background: var(--bg); padding: 12px; border-radius: 8px; border: 1px solid var(--panel-edge); margin-bottom: 8px; border-left: 4px solid ${roleColor};">
        <div style="font-size: 0.8rem; color: var(--text-secondary); margin-bottom: 6px; display: flex; justify-content: space-between; font-family: 'Inter', sans-serif;">
          <span style="font-weight: 600; color: ${roleColor}; text-transform: uppercase; font-size: 0.75rem; letter-spacing: 0.5px;">${m.role}</span>
          <span>${dateStr}</span>
        </div>
        <div style="color: var(--ink); white-space: pre-wrap; font-family: 'IBM Plex Mono', monospace; font-size: 0.85rem; line-height: 1.4;">${m.content}</div>
      </div>
    `;
  }).join("");
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

export function initMemory() {
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
  if (dom.memoryExportBtn) {
    dom.memoryExportBtn.addEventListener("click", () => {
      const dataStr = "data:text/json;charset=utf-8," + encodeURIComponent(JSON.stringify(GarraState.memories, null, 2));
      const downloadAnchorNode = document.createElement('a');
      downloadAnchorNode.setAttribute("href", dataStr);
      downloadAnchorNode.setAttribute("download", `garraia_memory_${Date.now()}.json`);
      document.body.appendChild(downloadAnchorNode);
      downloadAnchorNode.click();
      downloadAnchorNode.remove();
    });
  }
  if (dom.memoryClearBtn) {
    dom.memoryClearBtn.addEventListener("click", async () => {
      if (!GarraState.session) {
        alert("No active session to clear.");
        return;
      }
      if (!confirm("Are you sure you want to clear the session memory? This action cannot be undone.")) return;
      try {
        const res = await fetch(`/api/memory?session_id=${encodeURIComponent(GarraState.session)}`, { 
          method: "DELETE",
          headers: GarraState.gatewayKey ? { "X-Gateway-Key": GarraState.gatewayKey } : {}
        });
        if (res.ok) {
          GarraState.setMemories([]);
        } else {
          alert("Failed to clear memory.");
        }
      } catch (e) {
        alert("Error clearing memory: " + e.message);
      }
    });
  }
}
