mod overlay;

use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

#[tauri::command]
fn set_ignore_mouse(window: tauri::WebviewWindow, ignore: bool) {
    let _ = window.set_ignore_cursor_events(ignore);
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let visible = overlay::create_overlay(app.handle())?;

            let handle = app.handle().clone();
            let shortcut = Shortcut::new(Some(Modifiers::ALT), Code::KeyG);
            app.global_shortcut().on_shortcut(shortcut, move |_app, _shortcut, event| {
                if event.state != ShortcutState::Pressed { return; }

                if visible.load(std::sync::atomic::Ordering::Relaxed) {
                    // Already visible: toggle the input bar inside the webview
                    if let Some(win) = handle.get_webview_window("parrot") {
                        let _ = win.emit("garra-toggle-input", ());
                    }
                } else {
                    // Hidden: show the window again
                    overlay::toggle_overlay(&handle, &visible);
                }
            })?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![set_ignore_mouse])
        .run(tauri::generate_context!())
        .expect("error while running garraia-desktop");
}
