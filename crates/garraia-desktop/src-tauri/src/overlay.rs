use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const WIN_W: f64 = 220.0;
const WIN_H: f64 = 320.0;

/// Creates the transparent always-on-top parrot overlay window.
/// Position adapts to screen resolution (bottom-right, with taskbar clearance).
/// Returns an `Arc<AtomicBool>` tracking visibility state (true = visible).
pub fn create_overlay(app: &AppHandle) -> tauri::Result<Arc<AtomicBool>> {
    let monitor = app.primary_monitor()?.unwrap_or_else(|| panic!("No monitor found"));

    let scale = monitor.scale_factor();
    let screen_w = monitor.size().width as f64 / scale;
    let screen_h = monitor.size().height as f64 / scale;

    // Adaptive right margin: give more breathing room on ultra-wide displays.
    let right_margin = if screen_w > 2560.0 { 56.0 } else { 24.0 };

    // Bottom margin clears the taskbar.
    // Windows/Linux taskbar is typically ~40px at the bottom.
    // macOS menu bar is at the top, so the bottom is free.
    #[cfg(target_os = "macos")]
    let bottom_margin = 16.0_f64;
    #[cfg(not(target_os = "macos"))]
    let bottom_margin = 48.0_f64;

    let x = screen_w - WIN_W - right_margin;
    let y = screen_h - WIN_H - bottom_margin;

    WebviewWindowBuilder::new(app, "parrot", WebviewUrl::App("index.html".into()))
        .title("Garra")
        .inner_size(WIN_W, WIN_H)
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
