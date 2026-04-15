use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_updater::UpdaterExt;

use crate::gateway::GatewayHandle;

const TRAY_ID: &str = "garra_tray";

fn build_menu(app: &AppHandle, autostart_on: bool) -> tauri::Result<Menu<tauri::Wry>> {
    let open = MenuItem::with_id(app, "open", "Open Garra", true, None::<&str>)?;
    let quick_chat = MenuItem::with_id(app, "quick_chat", "Quick Chat", true, Some("Ctrl+Space"))?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let restart = MenuItem::with_id(
        app,
        "restart_gateway",
        "Restart Gateway",
        true,
        None::<&str>,
    )?;
    let voice = MenuItem::with_id(app, "toggle_voice", "Toggle Voice", true, None::<&str>)?;
    let logs = MenuItem::with_id(app, "open_logs", "Open Logs", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let auto_lbl = if autostart_on {
        "\u{2713} Start with OS"
    } else {
        "  Start with OS"
    };
    let autostart = MenuItem::with_id(app, "autostart", auto_lbl, true, None::<&str>)?;
    let update = MenuItem::with_id(app, "check_update", "Check for Updates", true, None::<&str>)?;
    let sep3 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Garra", true, None::<&str>)?;

    Menu::with_items(
        app,
        &[
            &open,
            &quick_chat,
            &sep1,
            &restart,
            &voice,
            &logs,
            &settings,
            &sep2,
            &autostart,
            &update,
            &sep3,
            &quit,
        ],
    )
}

pub fn setup_tray(
    app: &AppHandle,
    visible: Arc<AtomicBool>,
    gw: GatewayHandle,
) -> tauri::Result<()> {
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
    let menu = build_menu(app, autostart_on)?;

    let visible_menu = visible.clone();

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(tauri::include_image!("icons/32x32.png"))
        .tooltip("Garra Desktop")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "open" => crate::overlay::toggle_overlay(app, &visible_menu),
            "quick_chat" => crate::quick_chat::toggle_quick_chat(app),
            "restart_gateway" => crate::gateway::restart(app, &gw),
            "toggle_voice" => {
                if let Some(win) = app.get_webview_window("parrot") {
                    let _ = win.eval("window.__garra?.toggleVoice()");
                }
            }
            "open_logs" => open_log_dir(app),
            "settings" => open_settings(app),
            "autostart" => toggle_autostart(app),
            "check_update" => check_update_from_tray(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(move |tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::overlay::toggle_overlay(tray.app_handle(), &visible);
            }
        })
        .build(app)?;

    Ok(())
}

fn toggle_autostart(app: &AppHandle) {
    let mgr = app.autolaunch();
    let currently = mgr.is_enabled().unwrap_or(false);
    if currently {
        let _ = mgr.disable();
    } else {
        let _ = mgr.enable();
    }
    // Rebuild menu to reflect new state
    let new_state = mgr.is_enabled().unwrap_or(!currently);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        if let Ok(menu) = build_menu(app, new_state) {
            let _ = tray.set_menu(Some(menu));
        }
    }
}

fn open_log_dir(app: &AppHandle) {
    if let Ok(dir) = app.path().app_log_dir() {
        let _ = std::fs::create_dir_all(&dir);
        #[cfg(target_os = "windows")]
        let _ = std::process::Command::new("explorer").arg(&dir).spawn();
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(&dir).spawn();
        #[cfg(target_os = "linux")]
        let _ = std::process::Command::new("xdg-open").arg(&dir).spawn();
    }
}

fn open_settings(app: &AppHandle) {
    // If settings window already exists, just show/focus it
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.show();
        let _ = win.set_focus();
        return;
    }

    let _ = tauri::WebviewWindowBuilder::new(
        app,
        "settings",
        tauri::WebviewUrl::App("settings.html".into()),
    )
    .title("Garra Settings")
    .inner_size(520.0, 440.0)
    .center()
    .resizable(false)
    .build();
}

fn check_update_from_tray(app: &AppHandle) {
    use tauri_plugin_notification::NotificationExt;

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let updater = match app.updater_builder().build() {
            Ok(u) => u,
            Err(e) => {
                eprintln!("[garra] updater init error: {e}");
                return;
            }
        };

        match updater.check().await {
            Ok(Some(update)) => {
                let _ = app
                    .notification()
                    .builder()
                    .title("Garra Update Available")
                    .body(format!(
                        "Version {} is available. Click 'Check for Updates' in the tray to install.",
                        update.version
                    ))
                    .show();
            }
            Ok(None) => {
                let _ = app
                    .notification()
                    .builder()
                    .title("Garra Desktop")
                    .body("You are running the latest version.")
                    .show();
            }
            Err(e) => {
                eprintln!("[garra] update check failed: {e}");
                let _ = app
                    .notification()
                    .builder()
                    .title("Garra Update Check")
                    .body(format!("Failed to check for updates: {e}"))
                    .show();
            }
        }
    });
}
