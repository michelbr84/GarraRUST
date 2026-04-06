use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const QUICK_CHAT_LABEL: &str = "quick-chat";
const WIN_W: f64 = 420.0;
const WIN_H: f64 = 180.0;

/// Toggles the quick-chat mini window. If it exists and is visible, hide it;
/// if it exists but is hidden, show it; if it doesn't exist, create it.
pub fn toggle_quick_chat(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(QUICK_CHAT_LABEL) {
        match win.is_visible() {
            Ok(true) => {
                let _ = win.hide();
            }
            Ok(false) => {
                let _ = win.show();
                let _ = win.set_focus();
            }
            Err(_) => {
                let _ = win.show();
                let _ = win.set_focus();
            }
        }
    } else {
        let _ = create_quick_chat(app);
    }
}

fn create_quick_chat(app: &AppHandle) -> tauri::Result<()> {
    // Center on primary monitor
    let monitor = app.primary_monitor()?;
    let (x, y) = if let Some(mon) = monitor {
        let scale = mon.scale_factor();
        let sw = mon.size().width as f64 / scale;
        let sh = mon.size().height as f64 / scale;
        ((sw - WIN_W) / 2.0, (sh - WIN_H) / 2.0)
    } else {
        (400.0, 300.0)
    };

    WebviewWindowBuilder::new(app, QUICK_CHAT_LABEL, WebviewUrl::App("quick-chat.html".into()))
        .title("Garra Quick Chat")
        .inner_size(WIN_W, WIN_H)
        .position(x, y)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .center()
        .build()?;

    Ok(())
}
