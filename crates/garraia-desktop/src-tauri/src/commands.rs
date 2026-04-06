use serde::{Deserialize, Serialize};
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_updater::UpdaterExt;

// ── File picker commands ────────────────────────────────────────────────────

/// Opens a native folder picker and returns the selected folder path.
#[tauri::command]
pub async fn select_project_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let folder = app
        .dialog()
        .file()
        .set_title("Select project folder")
        .blocking_pick_folder();

    Ok(folder.map(|p| p.to_string()))
}

/// Opens a native file picker (multiple selection) and returns the selected file paths.
#[tauri::command]
pub async fn select_files(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let files = app
        .dialog()
        .file()
        .set_title("Select files")
        .blocking_pick_files();

    Ok(files
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.to_string())
        .collect())
}

// ── Notification commands ───────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct NotifyOptions {
    pub sound: Option<bool>,
}

/// Sends a system notification for a chat message.
#[tauri::command]
pub async fn notify_message(
    app: tauri::AppHandle,
    channel: String,
    sender: String,
    preview: String,
    options: Option<NotifyOptions>,
) -> Result<(), String> {
    let title = format!("[{channel}] {sender}");
    let body = if preview.len() > 200 {
        format!("{}...", &preview[..197])
    } else {
        preview
    };

    let mut builder = app.notification().builder().title(&title).body(&body);

    if options.as_ref().and_then(|o| o.sound).unwrap_or(true) {
        builder = builder.sound("default");
    }

    builder.show().map_err(|e| format!("Notification error: {e}"))?;

    Ok(())
}

// ── Quick-chat window management ────────────────────────────────────────────

/// Hides the quick-chat window (called from the quick-chat JS on Escape).
#[tauri::command]
pub async fn hide_quick_chat(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(win) = app.get_webview_window("quick-chat") {
        win.hide().map_err(|e| format!("Failed to hide quick-chat: {e}"))?;
    }
    Ok(())
}

// ── Updater commands ────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct UpdateInfo {
    pub available: bool,
    pub version: Option<String>,
    pub body: Option<String>,
}

/// Checks for updates from GitHub releases.
#[tauri::command]
pub async fn check_for_updates(app: tauri::AppHandle) -> Result<UpdateInfo, String> {
    let updater = app
        .updater_builder()
        .build()
        .map_err(|e| format!("Updater init error: {e}"))?;

    match updater.check().await {
        Ok(Some(update)) => Ok(UpdateInfo {
            available: true,
            version: Some(update.version.clone()),
            body: update.body.clone(),
        }),
        Ok(None) => Ok(UpdateInfo {
            available: false,
            version: None,
            body: None,
        }),
        Err(e) => Err(format!("Update check failed: {e}")),
    }
}

/// Downloads and installs the pending update.
#[tauri::command]
pub async fn install_update(app: tauri::AppHandle) -> Result<(), String> {
    let updater = app
        .updater_builder()
        .build()
        .map_err(|e| format!("Updater init error: {e}"))?;

    let update = updater
        .check()
        .await
        .map_err(|e| format!("Update check failed: {e}"))?
        .ok_or_else(|| "No update available".to_string())?;

    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| format!("Update install failed: {e}"))?;

    Ok(())
}
