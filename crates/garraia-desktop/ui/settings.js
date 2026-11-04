'use strict';
// Garra Settings (#1636, F3): script externo para a CSP `script-src 'self'`.
const invoke = window.__TAURI__?.core?.invoke ?? (() => Promise.resolve());

// Version comes from the crate (workspace-inherited) — never hardcode it here
// again: this page shipped saying "v0.2.0" for four minor versions.
window.__TAURI__?.app?.getVersion?.().then(v => {
  for (const id of ['app-version', 'app-version-footer']) {
    const el = document.getElementById(id);
    if (el) el.textContent = 'v' + v;
  }
}).catch(() => {});

// ── WhatsApp: modo (proprio/dedicado) + refazer pareamento (#1636) ──────────
const waHints = {
  dedicado: 'Só você conversa com o Garra, pelo seu número pessoal. É o padrão seguro.',
  proprio: 'O Garra responde qualquer pessoa que mandar mensagem no seu número.',
};
const waModoSel = document.getElementById('wa-modo');
const waHint = document.getElementById('wa-modo-hint');
const waStatus = document.getElementById('wa-status');
const waProprioRow = document.getElementById('wa-proprio-row');
const waDedicadoRow = document.getElementById('wa-dedicado-row');
const waConfirmo = document.getElementById('wa-proprio-confirmo');
const waNumero = document.getElementById('wa-numero');

function waRender() {
  if (waHint) waHint.textContent = waHints[waModoSel.value] || '';
  if (waProprioRow) waProprioRow.style.display = waModoSel.value === 'proprio' ? '' : 'none';
  if (waDedicadoRow) waDedicadoRow.style.display = waModoSel.value === 'dedicado' ? '' : 'none';
}

invoke('onboarding_estado').then((st) => {
  if (st && st.modo) waModoSel.value = st.modo;
  waRender();
}).catch(() => { waRender(); });

waModoSel?.addEventListener('change', waRender);

// #1824: com um gateway de fora do app, "Aplicar modo" muda a configuracao
// DELE — so com a confirmacao marcada (o backend recusa sem ela).
let waExterno = null;
invoke('gateway_estado').then((g) => {
  if (g && g.origem === 'externo' && typeof g.versao === 'string') {
    waExterno = g;
    const row = document.getElementById('wa-externo-row');
    const txt = document.getElementById('wa-externo-texto');
    if (txt) txt.textContent = 'Este modo vale para o Garra ' + g.versao + ' que já estava rodando nesta máquina, '
      + 'fora deste app. Antes de mudar, o app guarda uma cópia do config.yml na pasta backups.';
    if (row) row.style.display = '';
  }
}).catch(() => {});

document.getElementById('wa-aplicar')?.addEventListener('click', async () => {
  const modo = waModoSel.value;
  const args = { modo, numeroPessoal: null, confirmoAberto: false, confirmoExterno: false };
  if (waExterno) {
    if (!document.getElementById('wa-externo-confirmo')?.checked) {
      waStatus.textContent = 'Marque a confirmação: este modo muda o Garra ' + waExterno.versao + ' que já estava rodando.';
      return;
    }
    args.confirmoExterno = true;
  }
  if (modo === 'proprio') {
    if (!waConfirmo.checked) { waStatus.textContent = 'Marque a confirmação antes de abrir o Garra para todos.'; return; }
    args.confirmoAberto = true;
  } else if (modo === 'dedicado') {
    try {
      args.numeroPessoal = await invoke('onboarding_validar_numero', { numero: waNumero.value });
    } catch (e) {
      waStatus.textContent = 'Número inválido: ' + String(e);
      return;
    }
  }
  waStatus.textContent = 'Aplicando e reiniciando o Garra…';
  try {
    await invoke('onboarding_aplicar_modo', args);
    waStatus.textContent = 'Pronto — o modo foi atualizado.';
  } catch (e) {
    waStatus.textContent = 'Falhou ao aplicar: ' + String(e);
  }
});

document.getElementById('wa-refazer')?.addEventListener('click', () => {
  invoke('onboarding_reabrir');
});

// ── Check for updates button ────────────────────────────────────────────────
document.getElementById('check-update')?.addEventListener('click', async () => {
  const btn = document.getElementById('check-update');
  btn.textContent = 'Checking...';
  btn.disabled = true;
  try {
    const info = await invoke('check_for_updates');
    if (info?.available) {
      btn.textContent = 'Update to v' + info.version;
      btn.onclick = async () => {
        btn.textContent = 'Installing...';
        btn.disabled = true;
        try {
          await invoke('install_update');
          btn.textContent = 'Restart to finish';
        } catch (e) {
          btn.textContent = 'Install failed';
        }
      };
    } else {
      btn.textContent = 'Up to date!';
      setTimeout(() => { btn.textContent = 'Check for Updates'; btn.disabled = false; }, 3000);
    }
  } catch (e) {
    btn.textContent = 'Check failed';
    setTimeout(() => { btn.textContent = 'Check for Updates'; btn.disabled = false; }, 3000);
  }
});
