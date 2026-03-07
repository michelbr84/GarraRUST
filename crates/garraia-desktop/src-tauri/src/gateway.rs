use std::sync::{Arc, Mutex};
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

pub type GatewayHandle = Arc<Mutex<Option<tauri_plugin_shell::process::CommandChild>>>;

pub fn launch(app: &AppHandle) -> GatewayHandle {
    let handle: GatewayHandle = Arc::new(Mutex::new(None));
    spawn(app, handle.clone());
    handle
}

pub fn spawn(app: &AppHandle, handle: GatewayHandle) {
    match app.shell().sidecar("garraia") {
        Ok(cmd) => match cmd.args(["start"]).spawn() {
            Ok((_, child)) => {
                *handle.lock().unwrap() = Some(child);
            }
            Err(e) => {
                eprintln!("[garra] gateway spawn failed: {e}");
            }
        },
        Err(e) => {
            eprintln!("[garra] gateway sidecar not found: {e}");
        }
    }
}

pub fn restart(app: &AppHandle, handle: &GatewayHandle) {
    kill(handle);
    std::thread::sleep(std::time::Duration::from_millis(800));
    spawn(app, handle.clone());
}

pub fn kill(handle: &GatewayHandle) {
    if let Ok(mut guard) = handle.lock() {
        if let Some(child) = guard.take() {
            let _ = child.kill();
        }
    }
}
