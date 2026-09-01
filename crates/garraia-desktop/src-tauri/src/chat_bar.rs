//! Garra Chat Bar — barra de chat flutuante no topo central do desktop.
//!
//! Substitui a antiga janela "quick-chat" (Ctrl+Space). Diferenças de projeto:
//! - Criada no startup e visível por padrão ("a princípio ela ficará sempre no
//!   desktop na parte superior central"); o usuário oculta via ✕/Esc/bandeja/
//!   Ctrl+Space.
//! - Posição e visibilidade persistem entre execuções em `chat-bar.json` no
//!   `app_config_dir()`. Persistência manual de propósito: o
//!   tauri-plugin-window-state também restaura o TAMANHO, o que brigaria com o
//!   expand/collapse do painel de resposta — aqui só precisamos de x, y e
//!   visible.
//! - O redimensionamento (barra 56px ⇄ painel 320px) é feito aqui no Rust via
//!   comando, para a webview não precisar da capability
//!   `core:window:allow-set-size`.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder};

pub const LABEL: &str = "chat-bar";
const BAR_W: f64 = 560.0;
const BAR_H: f64 = 56.0;
const EXPANDED_H: f64 = 320.0;
const STATE_FILE: &str = "chat-bar.json";
const SAVE_DEBOUNCE_MS: u64 = 500;
const TOP_MARGIN: f64 = 16.0;

/// O que sobrevive entre execuções. Coordenadas em px lógicos (independentes
/// do fator de escala do monitor).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct PersistedState {
    x: f64,
    y: f64,
    visible: bool,
}

/// Estado gerenciado (`app.manage`) — fonte de verdade em memória.
/// `visible` é flag explícita, não `is_visible()`, pelo mesmo motivo do
/// overlay (overlay.rs): o valor reportado pelo WM não é confiável.
#[derive(Default)]
pub struct ChatBarState {
    pos: Mutex<Option<(f64, f64)>>,
    visible: AtomicBool,
    save_gen: AtomicU64,
}

/// Cria a janela da barra. Requer `app.manage(ChatBarState::default())` antes.
pub fn create_chat_bar(app: &AppHandle) -> tauri::Result<()> {
    let persisted = load_state(app);
    let (x, y) = match persisted.filter(|s| on_some_monitor(app, s.x, s.y)) {
        Some(s) => (s.x, s.y),
        // Primeira execução, arquivo corrompido ou monitor desconectado:
        // volta ao topo central do monitor primário.
        None => top_center(app)?,
    };
    let visible = persisted.map(|s| s.visible).unwrap_or(true);

    let state = app.state::<ChatBarState>();
    if let Ok(mut guard) = state.pos.lock() {
        *guard = Some((x, y));
    }
    state.visible.store(visible, Ordering::Relaxed);

    let win = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("chat-bar.html".into()))
        .title("Garra Chat Bar")
        .inner_size(BAR_W, BAR_H)
        .position(x, y)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .visible(visible)
        // Sem roubo de foco no login/autostart; foco só em toggle explícito.
        .focused(false)
        .build()?;

    let handle = app.clone();
    win.on_window_event(move |event| {
        if let tauri::WindowEvent::Moved(position) = event {
            on_moved(&handle, *position);
        }
    });

    Ok(())
}

/// Mostra/oculta a barra (bandeja e Ctrl+Space) e persiste a escolha.
pub fn toggle(app: &AppHandle) {
    let Some(win) = app.get_webview_window(LABEL) else {
        return;
    };
    let state = app.state::<ChatBarState>();

    if state.visible.load(Ordering::Relaxed) {
        let _ = win.hide();
        state.visible.store(false, Ordering::Relaxed);
    } else {
        let _ = win.show();
        let _ = win.set_focus();
        state.visible.store(true, Ordering::Relaxed);
    }
    save_state(app);
}

/// Oculta a barra (✕ / Esc na webview) e persiste.
pub fn hide(app: &AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window(LABEL) {
        win.hide()
            .map_err(|e| format!("Failed to hide chat bar: {e}"))?;
    }
    let state = app.state::<ChatBarState>();
    state.visible.store(false, Ordering::Relaxed);
    save_state(app);
    Ok(())
}

/// Cresce (painel de resposta) ou encolhe a janela. A âncora é o canto
/// superior esquerdo, então a barra expande para baixo — correto para uma
/// barra no topo da tela.
pub fn set_expanded(app: &AppHandle, expanded: bool) -> Result<(), String> {
    let Some(win) = app.get_webview_window(LABEL) else {
        return Ok(());
    };
    let height = if expanded { EXPANDED_H } else { BAR_H };
    win.set_size(LogicalSize::new(BAR_W, height))
        .map_err(|e| format!("Failed to resize chat bar: {e}"))
}

/// Grava o snapshot atual em `chat-bar.json`. Erros de IO são logados e
/// engolidos — persistência de UI nunca pode derrubar o app.
pub fn save_state(app: &AppHandle) {
    let state = app.state::<ChatBarState>();
    let Some((x, y)) = state.pos.lock().ok().and_then(|guard| *guard) else {
        return;
    };
    let snapshot = PersistedState {
        x,
        y,
        visible: state.visible.load(Ordering::Relaxed),
    };
    let Some(path) = state_path(app) else {
        return;
    };
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    match serde_json::to_string_pretty(&snapshot) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                eprintln!("[garra] failed to persist chat-bar state: {e}");
            }
        }
        Err(e) => eprintln!("[garra] failed to serialize chat-bar state: {e}"),
    }
}

fn load_state(app: &AppHandle) -> Option<PersistedState> {
    let raw = std::fs::read_to_string(state_path(app)?).ok()?;
    serde_json::from_str(&raw).ok()
}

fn state_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join(STATE_FILE))
}

/// Debounce do `WindowEvent::Moved`: um arraste emite dezenas de eventos por
/// segundo; só a geração mais recente grava, 500ms depois de o movimento
/// cessar. Contador de geração em vez de timer cancelável: mais simples e sem
/// estado extra.
fn on_moved(app: &AppHandle, position: tauri::PhysicalPosition<i32>) {
    let state = app.state::<ChatBarState>();
    let scale = app
        .get_webview_window(LABEL)
        .and_then(|w| w.scale_factor().ok())
        .unwrap_or(1.0);
    if let Ok(mut guard) = state.pos.lock() {
        *guard = Some((f64::from(position.x) / scale, f64::from(position.y) / scale));
    }
    let generation = state.save_gen.fetch_add(1, Ordering::SeqCst) + 1;

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(SAVE_DEBOUNCE_MS)).await;
        let state = handle.state::<ChatBarState>();
        if state.save_gen.load(Ordering::SeqCst) == generation {
            save_state(&handle);
        }
    });
}

/// Posição default: topo central do monitor primário. Sem monitor
/// (headless/RDP), posição fixa — mesmo fallback do overlay.
fn top_center(app: &AppHandle) -> tauri::Result<(f64, f64)> {
    Ok(match app.primary_monitor()? {
        Some(monitor) => {
            let scale = monitor.scale_factor();
            let screen_w = monitor.size().width as f64 / scale;
            (((screen_w - BAR_W) / 2.0).max(0.0), TOP_MARGIN)
        }
        None => (100.0, TOP_MARGIN),
    })
}

/// Uma posição persistida só é reaproveitada se ainda cair num monitor
/// conectado — senão a barra "nasceria" fora da tela, inalcançável.
fn on_some_monitor(app: &AppHandle, x: f64, y: f64) -> bool {
    let Ok(monitors) = app.available_monitors() else {
        // Não dá para saber — confia na posição gravada.
        return true;
    };
    if monitors.is_empty() {
        return true;
    }
    monitors.iter().any(|m| {
        let scale = m.scale_factor();
        let mx = f64::from(m.position().x) / scale;
        let my = f64::from(m.position().y) / scale;
        let mw = m.size().width as f64 / scale;
        let mh = m.size().height as f64 / scale;
        x >= mx && x < mx + mw && y >= my && y < my + mh
    })
}
