import { GarraState } from './state.js';
import { EventBus } from './eventBus.js';
import { loadMemory } from './views/memoryView.js';
import { loadLogs } from './views/logsView.js';

export function initRouter() {
  document.addEventListener("click", (e) => {
    const btn = e.target.closest(".nav-item[data-view]");
    if (!btn) return;
    e.preventDefault();
    GarraState.setView(btn.dataset.view);
  });

  EventBus.on("state:view", (viewName) => {
    document.querySelectorAll(".nav-item[data-view]").forEach(btn => {
      btn.classList.toggle("active", btn.dataset.view === viewName);
      btn.classList.toggle("is-active", btn.dataset.view === viewName);
    });

    document.querySelectorAll(".panel-view[data-view-panel]").forEach(panel => {
      if (panel.dataset.viewPanel === viewName) {
        panel.style.display = "";
        panel.classList.add("is-active");
      } else {
        panel.style.display = "none";
        panel.classList.remove("is-active");
      }
    });

    if (viewName === "memory") loadMemory();
    if (viewName === "logs") loadLogs();
  });
}
