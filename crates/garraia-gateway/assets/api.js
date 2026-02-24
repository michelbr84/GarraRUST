import { GarraState } from './state.js';
import { EventBus } from './eventBus.js';
import { dom } from './dom.js';

export async function refreshStatus() {
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
          dom.updateBannerText.innerHTML = (window.t ? window.t("update.available", {
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
        dom.channelListEl.innerHTML = '<span class="no-channels">' + (window.t ? window.t("none_configured") : "No channels") + "</span>";
      }
    }
  } catch {
    if (dom.apiStatusEl) dom.apiStatusEl.innerHTML = '<span class="status-dot dot-off"></span>' + (window.t ? window.t("status.unavailable") : "Unavailable");
    if (dom.sessionCountEl) dom.sessionCountEl.textContent = "-";
    if (dom.channelListEl) dom.channelListEl.innerHTML = '<span class="no-channels">-</span>';
  }
  loadProviders();
}

export async function loadProviders() {
  if (!dom.providerSelect) return;
  try {
    const r = await fetch("/api/providers");
    const j = await r.json();
    GarraState.providers = j.providers || [];

    dom.providerSelect.innerHTML = "";
    for (const p of GarraState.providers) {
      const opt = document.createElement("option");
      opt.value = p.id;
      opt.textContent = p.active ? p.display_name : p.display_name + (window.t ? window.t("provider.not_configured_suffix") : " (Not Configured)");
      dom.providerSelect.appendChild(opt);
    }

    const defaultProvider = GarraState.providers.find((p) => p.is_default);
    const saved = GarraState.selectedProvider || (defaultProvider ? defaultProvider.id : "");
    if (saved && [...dom.providerSelect.options].some((o) => o.value === saved)) {
      dom.providerSelect.value = saved;
    }
    updateProviderUI();
  } catch {
    dom.providerSelect.innerHTML = '<option value="">' + (window.t ? window.t("status.unavailable") : "Unavailable") + "</option>";
    if (dom.providerStatus) dom.providerStatus.textContent = "";
  }
}

export function updateProviderUI() {
  if (!dom.providerSelect || !dom.providerStatus) return;
  const id = dom.providerSelect.value;
  const p = GarraState.providers.find((x) => x.id === id);
  if (!p) {
    dom.providerStatus.textContent = "";
    if (dom.providerKeySection) dom.providerKeySection.style.display = "none";
    return;
  }
  if (p.active) {
    const tag = p.is_default ? (window.t ? window.t("provider.active_default") : "Active Default") : (window.t ? window.t("provider.active") : "Active");
    dom.providerStatus.innerHTML = `<span class="status-dot dot-ok"></span>${tag}`;
    if (dom.providerKeySection) dom.providerKeySection.style.display = "none";
  } else {
    dom.providerStatus.innerHTML = '<span class="status-dot dot-off"></span>' + (window.t ? window.t("provider.not_configured") : "Not Configured");
    if (dom.providerKeySection) dom.providerKeySection.style.display = p.needs_api_key ? "" : "none";
  }

  // Handle Model Datalist and Default value
  if (dom.providerModelOptions) {
    dom.providerModelOptions.innerHTML = "";
    if (p.models && p.models.length > 0) {
      for (const m of p.models) {
        const option = document.createElement("option");
        option.value = m;
        dom.providerModelOptions.appendChild(option);
      }
    }
  }

  if (dom.providerModelInput) {
    // If the provider has a known configured model from backend, use it
    if (p.model) {
      dom.providerModelInput.value = p.model;
    } else if (p.models && p.models.length > 0) {
       // If no model is configured, but there are available models, default to it
       dom.providerModelInput.value = p.models[0];
    } else {
       dom.providerModelInput.value = "";
    }
  }

  GarraState.setProvider(id);
}

export function initApiView() {
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

            dom.providerActivateBtn.textContent = window.t ? window.t("button.activating") : "Activating...";
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
                    const { appendMessage } = await import('./views/chatView.js');
                    appendMessage("sys", window.t ? window.t("message.provider_activated", { id: id }) : `Provider ${id} activated.`);
                    await loadProviders();
                } else {
                    const { appendMessage } = await import('./views/chatView.js');
                    appendMessage("error", j.message || (window.t ? window.t("error.provider_activation") : "Activation failed"));
                }
            } catch (e) {
                const { appendMessage } = await import('./views/chatView.js');
                appendMessage("error", (window.t ? window.t("error.provider_activation") : "Activation failed") + " " + e);
            } finally {
                dom.providerActivateBtn.textContent = window.t ? window.t("button.save_activate") : "Save & Activate";
                dom.providerActivateBtn.disabled = false;
            }
        });
    }

    if (dom.keyGatewayEl) dom.keyGatewayEl.value = GarraState.gatewayKey;

    if (dom.authConnectBtn) {
        dom.authConnectBtn.addEventListener("click", async () => {
            GarraState.setGatewayKey(dom.keyGatewayEl.value.trim());
            const { reconnectFresh } = await import('./views/chatView.js');
            reconnectFresh();
        });
    }
}
