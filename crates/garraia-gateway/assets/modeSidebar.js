import { GarraState } from './state.js';
import { EventBus } from './eventBus.js';
import { dom } from './dom.js';
import { escapeHtml, escapeAttr } from './utils.js';

// Mode icons (SVG paths)
const MODE_ICONS = {
  auto: 'M12 2L2 7l10 5 10-5-10-5zM2 17l10 5 10-5M2 12l10 5 10-5',
  search: 'M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z',
  architect: 'M3 3h18v18H3V3zm2 2v14h14V5H5z M7 7h10 M7 12h10 M7 17h6',
  code: 'M16 18l6-6-6-6M8 6l-6 6 6 6',
  ask: 'M8.228 9c.549-1.165 2.03-2 3.772-2 2.21 0 4 1.343 4 3 0 1.4-1.278 2.575-3.006 2.907-.542.104-.994.54-.994 1.093m0 3h.01M21 12a9 9 0 11-18 0 9 9 0 0118 0z',
  debug: 'M10.29 3.86L1.82 18a2 2 0 001.71 3h16.94a2 2 0 001.71-3L13.71 3.86a2 2 0 00-3.42 0zM12 9v4m0 4h.01',
  orchestrator: 'M4 4v5h.581m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15',
  review: 'M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2m-6 9l2 2 4-4',
  edit: 'M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z',
  custom: 'M12 4v16m-8-8h16',
};

// Mode colors (CSS variable references)
const MODE_COLORS = {
  auto: 'var(--online)',
  search: 'var(--brand)',
  architect: 'var(--warn-edge)',
  code: 'var(--brand-2)',
  ask: 'var(--ink)',
  debug: 'var(--error-edge)',
  orchestrator: 'var(--accent-line)',
  review: 'var(--offline)',
  edit: 'var(--online)',
  custom: 'var(--warn-edge)',
};

// Store for modes data
let modesData = [];
let customModes = [];

/**
 * Fetches all available modes from the API
 */
export async function fetchModes() {
  try {
    const response = await fetch('/api/modes');
    const data = await response.json();
    modesData = data.modes || [];
    
    // Load custom modes from API (and fallback to localStorage)
    await fetchCustomModes();
    
    return modesData;
  } catch (error) {
    console.error('Failed to fetch modes:', error);
    // Fallback to static modes
    modesData = getDefaultModes();
    return modesData;
  }
}

/**
 * Fetches custom modes from the API
 */
async function fetchCustomModes() {
  try {
    const response = await fetch('/api/modes/custom');
    const data = await response.json();
    
    if (data.success && data.modes) {
      // Convert API modes to local format with isCustom flag
      customModes = data.modes.map(mode => ({
        id: mode.id,
        name: mode.name,
        description: mode.description || '',
        baseMode: mode.base_mode,
        toolPolicyOverrides: mode.tool_policy_overrides || {},
        promptOverride: mode.prompt_override,
        defaults: mode.defaults || {},
        isCustom: true,
      }));
      
      // Save to localStorage as backup
      localStorage.setItem('garraia.custom_modes', JSON.stringify(customModes));
    }
  } catch (error) {
    console.error('Failed to fetch custom modes from API:', error);
    // Fallback to localStorage
    const savedCustomModes = localStorage.getItem('garraia.custom_modes');
    if (savedCustomModes) {
      try {
        customModes = JSON.parse(savedCustomModes);
      } catch (e) {
        customModes = [];
      }
    }
  }
}

/**
 * Gets default modes when API is unavailable
 */
function getDefaultModes() {
  return [
    { id: 'auto', name: 'Auto', description: 'Decide automaticamente o modo baseado no conteúdo' },
    { id: 'search', name: 'Search', description: 'Busca e inspeção sem modificar arquivos' },
    { id: 'architect', name: 'Architect', description: 'Design, planejamento e arquitetura' },
    { id: 'code', name: 'Code', description: 'Implementação ativa - permite escrita e execução' },
    { id: 'ask', name: 'Ask', description: 'Apenas perguntas - modo padrão' },
    { id: 'debug', name: 'Debug', description: 'Análise de erros, stack traces e logs' },
    { id: 'orchestrator', name: 'Orchestrator', description: 'Execução multi-etapas com planejamento' },
    { id: 'review', name: 'Review', description: 'Revisão de código e análise de changes' },
    { id: 'edit', name: 'Edit', description: 'Edição pontual de arquivos' },
  ];
}

/**
 * Fetches current mode for the session
 */
export async function fetchCurrentMode() {
  try {
    const headers = {};
    if (GarraState.session) {
      headers['X-Session-Id'] = GarraState.session;
    }
    
    const response = await fetch('/api/mode/current', { headers });
    const data = await response.json();
    return data.mode;
  } catch (error) {
    console.error('Failed to fetch current mode:', error);
    return null;
  }
}

/**
 * Selects a mode for the session
 */
export async function selectMode(modeId) {
  try {
    const headers = { 'Content-Type': 'application/json' };
    if (GarraState.session) {
      headers['X-Session-Id'] = GarraState.session;
    }
    
    const response = await fetch('/api/mode/select', {
      method: 'POST',
      headers,
      body: JSON.stringify({ mode: modeId }),
    });
    
    const data = await response.json();
    
    if (data.success) {
      // Update state
      GarraState.setMode(modeId);
      
      // Update badge in UI
      updateModeBadge(modeId);
      
      // Emit event for other components
      EventBus.emit('mode:changed', modeId);
      
      return { success: true, mode: modeId };
    } else {
      return { success: false, message: data.message };
    }
  } catch (error) {
    console.error('Failed to select mode:', error);
    return { success: false, message: error.message };
  }
}

/**
 * Gets the icon SVG for a mode
 */
function getModeIcon(modeId) {
  const path = MODE_ICONS[modeId.toLowerCase()] || MODE_ICONS.ask;
  return `<svg viewBox="0 0 24 24" width="18" height="18" stroke="currentColor" fill="none" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="${path}"></path></svg>`;
}

/**
 * Gets the color for a mode
 */
function getModeColor(modeId) {
  return MODE_COLORS[modeId.toLowerCase()] || MODE_COLORS.ask;
}

/**
 * Filters modes based on search query
 */
function filterModes(query, modes) {
  if (!query || query.trim() === '') {
    return modes;
  }
  
  const lowerQuery = query.toLowerCase();
  return modes.filter(mode => 
    mode.name.toLowerCase().includes(lowerQuery) ||
    mode.description.toLowerCase().includes(lowerQuery) ||
    mode.id.toLowerCase().includes(lowerQuery)
  );
}

/**
 * Renders the mode list in the sidebar
 */
function renderModeList(modes, searchQuery = '') {
  const filteredModes = filterModes(searchQuery, modes);
  
  // Separate standard modes and custom modes
  const standardModes = filteredModes.filter(m => !m.isCustom);
  const customFiltered = filteredModes.filter(m => m.isCustom);
  
  let html = '';
  
  // Auto mode (highlighted)
  const autoMode = standardModes.find(m => m.id === 'auto');
  if (autoMode && (!searchQuery || autoMode.name.toLowerCase().includes(searchQuery.toLowerCase()))) {
    html += renderModeItem(autoMode, true);
  }
  
  // Standard modes
  const otherModes = standardModes.filter(m => m.id !== 'auto');
  otherModes.forEach(mode => {
    html += renderModeItem(mode, false);
  });
  
  // Custom modes section
  if (customFiltered.length > 0 || !searchQuery) {
    html += '<div class="mode-section-title">Custom</div>';
    customFiltered.forEach(mode => {
      html += renderModeItem(mode, false, true);
    });
  }
  
  const modeListEl = document.getElementById('mode-list');
  if (modeListEl) {
    modeListEl.innerHTML = html;
    
    // Add click handlers for mode selection
    modeListEl.querySelectorAll('.mode-item').forEach(item => {
      item.addEventListener('click', (e) => {
        // Don't trigger mode select when clicking edit/delete buttons
        if (e.target.closest('.mode-actions')) return;
        const modeId = item.dataset.modeId;
        handleModeSelect(modeId);
      });
    });
    
    // Add click handlers for edit buttons
    modeListEl.querySelectorAll('.mode-edit-btn').forEach(btn => {
      btn.addEventListener('click', (e) => {
        e.stopPropagation();
        const modeId = btn.dataset.modeId;
        handleEditMode(modeId);
      });
    });
    
    // Add click handlers for delete buttons
    modeListEl.querySelectorAll('.mode-delete-btn').forEach(btn => {
      btn.addEventListener('click', (e) => {
        e.stopPropagation();
        const modeId = btn.dataset.modeId;
        handleDeleteMode(modeId);
      });
    });
  }
}

/**
 * Renders a single mode item
 */
function renderModeItem(mode, isAuto = false, isCustom = false) {
  const icon = getModeIcon(mode.id);
  const color = getModeColor(mode.id);
  const currentMode = GarraState.currentMode || 'ask';
  const isActive = currentMode === mode.id;
  
  const safeId = escapeAttr(mode.id);
  const safeName = escapeHtml(mode.name);
  const safeDesc = escapeHtml(mode.description);
  const safeDescAttr = escapeAttr(mode.description);

  const actionsHtml = isCustom ? `
    <div class="mode-actions">
      <button class="mode-edit-btn" data-mode-id="${safeId}" title="Edit mode">
        <svg viewBox="0 0 24 24" width="14" height="14" stroke="currentColor" fill="none" stroke-width="2"><path d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z"></path></svg>
      </button>
      <button class="mode-delete-btn" data-mode-id="${safeId}" title="Delete mode">
        <svg viewBox="0 0 24 24" width="14" height="14" stroke="currentColor" fill="none" stroke-width="2"><path d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16"></path></svg>
      </button>
    </div>
  ` : '';

  return `
    <div class="mode-item ${isActive ? 'active' : ''} ${isAuto ? 'mode-auto' : ''} ${isCustom ? 'mode-custom' : ''}"
         data-mode-id="${safeId}"
         title="${safeDescAttr}">
      <div class="mode-icon" style="color: ${color}">${icon}</div>
      <div class="mode-info">
        <div class="mode-name">${safeName}</div>
        <div class="mode-desc">${safeDesc}</div>
      </div>
      ${actionsHtml}
      ${isActive ? '<div class="mode-check">✓</div>' : ''}
    </div>
  `;
}

/**
 * Handles mode selection
 */
async function handleModeSelect(modeId) {
  const result = await selectMode(modeId);
  
  if (result.success) {
    // Re-render to show active state
    const searchQuery = dom.modeSearchInput?.value || '';
    renderModeList([...modesData, ...customModes], searchQuery);
  }
}

/**
 * Handles edit mode click
 */
function handleEditMode(modeId) {
  const mode = customModes.find(m => m.id === modeId);
  if (mode) {
    showCustomModeForm(mode);
  }
}

/**
 * Handles delete mode click
 */
async function handleDeleteMode(modeId) {
  const mode = customModes.find(m => m.id === modeId);
  if (!mode) return;
  
  if (confirm(`Delete custom mode "${mode.name}"?`)) {
    try {
      const response = await fetch(`/api/modes/custom/${modeId}`, {
        method: 'DELETE',
      });
      
      const data = await response.json();
      
      if (data.success) {
        // Remove from local array
        customModes = customModes.filter(m => m.id !== modeId);
        
        // Update localStorage
        localStorage.setItem('garraia.custom_modes', JSON.stringify(customModes));
        
        // Re-render list
        const searchQuery = dom.modeSearchInput?.value || '';
        renderModeList([...modesData, ...customModes], searchQuery);
        
        // If deleted mode was active, switch to ask
        if (GarraState.currentMode === modeId) {
          selectMode('ask');
        }
        
        EventBus.emit('mode:customDeleted', modeId);
      } else {
        alert('Failed to delete mode: ' + data.message);
      }
    } catch (error) {
      console.error('Failed to delete custom mode:', error);
      alert('Failed to delete custom mode');
    }
  }
}

/**
 * Shows the custom mode form modal
 */
function showCustomModeForm(existingMode = null) {
  // Remove existing modal if any
  const existingModal = document.getElementById('custom-mode-modal');
  if (existingModal) {
    existingModal.remove();
  }
  
  const isEdit = !!existingMode;
  const modalHtml = `
    <div class="modal-overlay" id="custom-mode-modal">
      <div class="modal-content">
        <div class="modal-header">
          <h3>${isEdit ? 'Edit Custom Mode' : 'Create Custom Mode'}</h3>
          <button class="modal-close" id="modal-close-btn">&times;</button>
        </div>
        <form id="custom-mode-form">
          <div class="form-group">
            <label for="mode-name">Mode Name</label>
            <input type="text" id="mode-name" name="name" required placeholder="My Custom Mode" value="${escapeAttr(existingMode?.name || '')}">
          </div>
          
          <div class="form-group">
            <label for="mode-description">Description</label>
            <input type="text" id="mode-description" name="description" placeholder="Brief description of this mode" value="${escapeAttr(existingMode?.description || '')}">
          </div>
          
          <div class="form-group">
            <label for="mode-base">Base Mode</label>
            <select id="mode-base" name="baseMode" required>
              <option value="auto" ${existingMode?.baseMode === 'auto' ? 'selected' : ''}>Auto</option>
              <option value="search" ${existingMode?.baseMode === 'search' ? 'selected' : ''}>Search</option>
              <option value="architect" ${existingMode?.baseMode === 'architect' ? 'selected' : ''}>Architect</option>
              <option value="code" ${(!existingMode?.baseMode || existingMode?.baseMode === 'code') ? 'selected' : ''}>Code</option>
              <option value="ask" ${existingMode?.baseMode === 'ask' ? 'selected' : ''}>Ask</option>
              <option value="debug" ${existingMode?.baseMode === 'debug' ? 'selected' : ''}>Debug</option>
              <option value="orchestrator" ${existingMode?.baseMode === 'orchestrator' ? 'selected' : ''}>Orchestrator</option>
              <option value="review" ${existingMode?.baseMode === 'review' ? 'selected' : ''}>Review</option>
              <option value="edit" ${existingMode?.baseMode === 'edit' ? 'selected' : ''}>Edit</option>
            </select>
          </div>
          
          <div class="form-group">
            <label for="mode-prompt">System Prompt Override (optional)</label>
            <textarea id="mode-prompt" name="promptOverride" rows="3" placeholder="Custom system prompt...">${escapeHtml(existingMode?.promptOverride || '')}</textarea>
          </div>
          
          <div class="form-group">
            <label>LLM Defaults (optional)</label>
            <div class="llmdefaults-grid">
              <div>
                <label for="mode-temp">Temperature</label>
                <input type="number" id="mode-temp" name="temperature" min="0" max="2" step="0.1" value="${escapeAttr(existingMode?.defaults?.temperature ?? 0.7)}">
              </div>
              <div>
                <label for="mode-maxtokens">Max Tokens</label>
                <input type="number" id="mode-maxtokens" name="maxTokens" min="1" max="32000" value="${escapeAttr(existingMode?.defaults?.max_tokens ?? 4096)}">
              </div>
              <div>
                <label for="mode-topp">Top P</label>
                <input type="number" id="mode-topp" name="topP" min="0" max="1" step="0.1" value="${escapeAttr(existingMode?.defaults?.top_p ?? 0.9)}">
              </div>
            </div>
          </div>
          
          <div class="form-actions">
            <button type="button" class="btn-secondary" id="modal-cancel-btn">Cancel</button>
            <button type="submit" class="btn-primary">${isEdit ? 'Update' : 'Create'}</button>
          </div>
        </form>
      </div>
    </div>
  `;
  
  // Add modal to DOM
  document.body.insertAdjacentHTML('beforeend', modalHtml);
  
  // Add event listeners
  const modal = document.getElementById('custom-mode-modal');
  const closeBtn = document.getElementById('modal-close-btn');
  const cancelBtn = document.getElementById('modal-cancel-btn');
  const form = document.getElementById('custom-mode-form');
  
  closeBtn.addEventListener('click', () => modal.remove());
  cancelBtn.addEventListener('click', () => modal.remove());
  
  modal.addEventListener('click', (e) => {
    if (e.target === modal) modal.remove();
  });
  
  form.addEventListener('submit', async (e) => {
    e.preventDefault();
    
    const formData = new FormData(form);
    const data = {
      name: formData.get('name'),
      description: formData.get('description') || null,
      base_mode: formData.get('baseMode'),
      tool_policy_overrides: {},
      prompt_override: formData.get('promptOverride') || null,
      defaults: {
        temperature: parseFloat(formData.get('temperature')) || 0.7,
        max_tokens: parseInt(formData.get('maxTokens')) || 4096,
        top_p: parseFloat(formData.get('topP')) || 0.9,
      },
    };
    
    try {
      let url = '/api/modes/custom';
      let method = 'POST';
      
      if (isEdit) {
        url = `/api/modes/custom/${existingMode.id}`;
        method = 'PATCH';
      }
      
      const response = await fetch(url, {
        method,
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(data),
      });
      
      const result = await response.json();
      
      if (result.success) {
        // Refresh custom modes
        await fetchCustomModes();
        
        // Re-render list
        const searchQuery = dom.modeSearchInput?.value || '';
        renderModeList([...modesData, ...customModes], searchQuery);
        
        // Close modal
        modal.remove();
        
        EventBus.emit('mode:customCreated', result.mode);
      } else {
        alert('Failed to save mode: ' + result.message);
      }
    } catch (error) {
      console.error('Failed to save custom mode:', error);
      alert('Failed to save custom mode');
    }
  });
}

/**
 * Updates the mode badge in the chat header
 */
function updateModeBadge(modeId) {
  const badgeEl = document.getElementById('mode-badge');
  if (!badgeEl) return;
  
  const mode = modesData.find(m => m.id === modeId) || customModes.find(m => m.id === modeId);
  if (!mode) return;
  
  const icon = getModeIcon(modeId);
  const color = getModeColor(modeId);
  
  badgeEl.innerHTML = `
    <span class="mode-badge-icon" style="color: ${color}">${icon}</span>
    <span class="mode-badge-text">${escapeHtml(mode.name)}</span>
  `;
  badgeEl.style.display = 'inline-flex';
}

/**
 * Initializes the mode sidebar
 */
export async function initModeSidebar() {
  // Add mode state to GarraState
  GarraState.currentMode = localStorage.getItem('garraia.current_mode') || null;
  
  // Fetch modes from API
  await fetchModes();
  
  // Render mode list
  renderModeList([...modesData, ...customModes]);
  
  // Setup "Add Custom Mode" button
  const addCustomModeBtn = document.getElementById('add-custom-mode-btn');
  if (addCustomModeBtn) {
    addCustomModeBtn.addEventListener('click', () => {
      openCreateCustomMode();
    });
  }
  
  // Fetch and set current mode
  const currentMode = await fetchCurrentMode();
  if (currentMode) {
    GarraState.setMode(currentMode);
    updateModeBadge(currentMode);
  } else if (GarraState.currentMode) {
    updateModeBadge(GarraState.currentMode);
  }
  
  // Setup search functionality
  if (dom.modeSearchInput) {
    dom.modeSearchInput.addEventListener('input', (e) => {
      renderModeList([...modesData, ...customModes], e.target.value);
    });
  }
  
  // Listen for mode changes from other sources
  EventBus.on('mode:changed', (modeId) => {
    updateModeBadge(modeId);
    const searchQuery = dom.modeSearchInput?.value || '';
    renderModeList([...modesData, ...customModes], searchQuery);
  });
  
  // Listen for session changes to refresh mode
  EventBus.on('state:session', async (sessionId) => {
    if (sessionId) {
      const currentMode = await fetchCurrentMode();
      if (currentMode) {
        GarraState.setMode(currentMode);
        updateModeBadge(currentMode);
      }
    }
  });
}

/**
 * Opens the custom mode creation form
 * This function can be called externally (e.g., from a button)
 */
export function openCreateCustomMode() {
  showCustomModeForm(null);
}

// Extend GarraState with mode methods
GarraState.setMode = function(modeId) {
  this.currentMode = modeId;
  localStorage.setItem('garraia.current_mode', modeId);
  EventBus.emit('state:mode', modeId);
};

GarraState.getMode = function() {
  return this.currentMode;
};

export { MODE_ICONS, MODE_COLORS };
