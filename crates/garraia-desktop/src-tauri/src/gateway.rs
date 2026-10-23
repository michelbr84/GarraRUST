use std::sync::{Arc, Mutex};
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

pub type GatewayHandle = Arc<Mutex<Option<tauri_plugin_shell::process::CommandChild>>>;

/// Job Object do Windows que contem o sidecar `garraia start` — e, por heranca
/// de job, a arvore que ele lanca, inclusive a ponte Node do WhatsApp. Criado
/// uma vez e reusado entre reinicios.
///
/// O `RunEvent::Exit` (em `lib.rs`) ja mata o sidecar na saida NORMAL do app. O
/// Job Object cobre o buraco: quando o desktop morre de forma ABRUPTA (crash,
/// "Finalizar tarefa" do Gerenciador de Tarefas), nenhum `RunEvent` roda, mas o
/// SO fecha o handle do job e o `KILL_ON_JOB_CLOSE` encerra a arvore. No Unix o
/// papel e do PDEATHSIG/grupo de processos no proprio bridge.
///
/// Decisao (#1636): a logica do job vive em `garraia-desktop-core` (sem Tauri,
/// testada no CI) e a casca so a aplica por PID; nao migramos o lancamento para
/// o `supervise` do core porque a casca Tauri esta fora dos gates de CI e a
/// resolucao do sidecar pelo plugin shell e robusta. Ver docs/whatsapp.md.
#[cfg(windows)]
static GATEWAY_JOB: std::sync::OnceLock<Option<garraia_desktop_core::KillOnCloseJob>> =
    std::sync::OnceLock::new();

/// Poe o sidecar recem-lancado no Job Object. Best-effort: falha so loga, o
/// `RunEvent::Exit` e o `kill` continuam valendo.
#[cfg(windows)]
fn contain_in_kill_on_close_job(child: &tauri_plugin_shell::process::CommandChild) {
    let job = GATEWAY_JOB.get_or_init(|| match garraia_desktop_core::KillOnCloseJob::new() {
        Ok(job) => Some(job),
        Err(e) => {
            eprintln!("[garra] job object init failed: {e}");
            None
        }
    });
    if let Some(job) = job
        && let Err(e) = job.assign_pid(child.pid())
    {
        eprintln!("[garra] assign sidecar to job failed: {e}");
    }
}

pub fn launch(app: &AppHandle) -> GatewayHandle {
    let handle: GatewayHandle = Arc::new(Mutex::new(None));
    spawn(app, handle.clone());
    handle
}

pub fn spawn(app: &AppHandle, handle: GatewayHandle) {
    match app.shell().sidecar("garraia") {
        Ok(cmd) => match cmd.args(["start"]).spawn() {
            Ok((_, child)) => {
                #[cfg(windows)]
                contain_in_kill_on_close_job(&child);
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
