mod commands;
mod gateway;
mod hotkey;
mod overlay;
mod quick_chat;
mod tray;

use tauri::Manager;

#[tauri::command]
fn set_ignore_mouse(window: tauri::WebviewWindow, ignore: bool) {
    let _ = window.set_ignore_cursor_events(ignore);
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let visible = overlay::create_overlay(app.handle())?;
            let gw = gateway::launch(app.handle());

            // Store in managed state so the exit handler can reach it
            app.manage(gw.clone());

            tray::setup_tray(app.handle(), visible.clone(), gw)?;

            // Register global hotkeys (Alt+G overlay toggle, Ctrl+Space quick-chat)
            hotkey::register_hotkeys(app.handle(), visible)?;

            // Copy default config if not present
            if let (Ok(resource_dir), Ok(config_dir)) =
                (app.path().resource_dir(), app.path().app_config_dir())
            {
                // Gateway reads from %APPDATA%\garraia\config.yml
                let config_file = config_dir
                    .parent()
                    .unwrap_or(&config_dir)
                    .join("garraia")
                    .join("config.yml");
                if !config_file.exists() {
                    let src = resource_dir.join("config.default.yml");
                    if src.exists() {
                        if let Some(parent) = config_file.parent() {
                            std::fs::create_dir_all(parent).ok();
                        }
                        std::fs::copy(&src, &config_file).ok();
                    }
                }
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            set_ignore_mouse,
            commands::select_project_folder,
            commands::select_files,
            commands::notify_message,
            commands::hide_quick_chat,
            commands::check_for_updates,
            commands::install_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while running garraia-desktop")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                let gw = app.state::<gateway::GatewayHandle>();
                gateway::kill(&gw);
            }
        });
}
