use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const WIN_W: f64 = 220.0;
const WIN_H: f64 = 320.0;

/// Creates the transparent always-on-top parrot overlay window.
/// Position adapts to screen resolution (bottom-right, with taskbar clearance).
/// Returns an `Arc<AtomicBool>` tracking visibility state (true = visible).
pub fn create_overlay(app: &AppHandle) -> tauri::Result<Arc<AtomicBool>> {
    // Bottom margin clears the taskbar.
    // Windows/Linux taskbar is typically ~40px at the bottom.
    // macOS menu bar is at the top, so the bottom is free.
    #[cfg(target_os = "macos")]
    let bottom_margin = 16.0_f64;
    #[cfg(not(target_os = "macos"))]
    let bottom_margin = 48.0_f64;

    // Headless/RDP sessions can report no primary monitor; fall back to a
    // fixed position instead of aborting the whole app in setup().
    let (x, y) = match app.primary_monitor()? {
        Some(monitor) => {
            let scale = monitor.scale_factor();
            let screen_w = monitor.size().width as f64 / scale;
            let screen_h = monitor.size().height as f64 / scale;

            // Adaptive right margin: more breathing room on ultra-wide displays.
            let right_margin = if screen_w > 2560.0 { 56.0 } else { 24.0 };

            (
                screen_w - WIN_W - right_margin,
                screen_h - WIN_H - bottom_margin,
            )
        }
        None => (100.0, 100.0),
    };

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
    let Some(win) = app.get_webview_window("parrot") else {
        return;
    };

    if visible.load(Ordering::Relaxed) {
        let _ = win.hide();
        visible.store(false, Ordering::Relaxed);
    } else {
        let _ = win.show();
        let _ = win.set_focus();
        visible.store(true, Ordering::Relaxed);
    }
}
