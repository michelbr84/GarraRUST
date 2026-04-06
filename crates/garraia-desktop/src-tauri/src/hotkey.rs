use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

/// Registers global hotkeys:
/// - Alt+G: toggle input bar in the parrot overlay
/// - Ctrl+Space: toggle quick-chat overlay
pub fn register_hotkeys(
    app: &AppHandle,
    visible: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    let handle_alt_g = app.clone();
    let visible_alt_g = visible.clone();
    let alt_g = Shortcut::new(Some(Modifiers::ALT), Code::KeyG);

    app.global_shortcut().on_shortcut(alt_g, move |_app, _shortcut, event| {
        if event.state != ShortcutState::Pressed {
            return;
        }

        if visible_alt_g.load(Ordering::Relaxed) {
            if let Some(win) = handle_alt_g.get_webview_window("parrot") {
                let _ = win.eval("window.__garra?.toggleInput()");
            }
        } else {
            crate::overlay::toggle_overlay(&handle_alt_g, &visible_alt_g);
        }
    })?;

    let handle_ctrl_space = app.clone();
    let ctrl_space = Shortcut::new(Some(Modifiers::CONTROL), Code::Space);

    app.global_shortcut().on_shortcut(ctrl_space, move |_app, _shortcut, event| {
        if event.state != ShortcutState::Pressed {
            return;
        }
        crate::quick_chat::toggle_quick_chat(&handle_ctrl_space);
    })?;

    Ok(())
}
