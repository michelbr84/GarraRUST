use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

/// Creates the transparent always-on-top parrot overlay window.
/// Returns an `Arc<AtomicBool>` tracking visibility state (true = visible).
pub fn create_overlay(app: &AppHandle) -> tauri::Result<Arc<AtomicBool>> {
    let monitor = app.primary_monitor()?.unwrap_or_else(|| {
        panic!("No monitor found")
    });

    let scale = monitor.scale_factor();
    let screen_w = monitor.size().width as f64 / scale;
    let screen_h = monitor.size().height as f64 / scale;

    let win_w = 220.0_f64;
    let win_h = 320.0_f64;

    let x = screen_w - win_w - 24.0;
    let y = screen_h - win_h - 48.0;

    WebviewWindowBuilder::new(app, "parrot", WebviewUrl::App("index.html".into()))
        .title("Garra")
        .inner_size(win_w, win_h)
        .position(x, y)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        .build()?;

    Ok(Arc::new(AtomicBool::new(true)))
}

/// Toggles overlay visibility using an explicit state flag (avoids is_visible() unreliability).
pub fn toggle_overlay(app: &AppHandle, visible: &Arc<AtomicBool>) {
    let Some(win) = app.get_webview_window("parrot") else { return };

    if visible.load(Ordering::Relaxed) {
        let _ = win.hide();
        visible.store(false, Ordering::Relaxed);
    } else {
        let _ = win.show();
        let _ = win.set_focus();
        visible.store(true, Ordering::Relaxed);
    }
}
