//! Teste de orfao da ponte no Unix: a subarvore inteira morre no teardown.
//!
//! Exercita o codigo REAL de producao — `apply_process_group` (que poe o filho
//! no proprio grupo com `setpgid`, rodado dentro de `BridgeConnection::spawn`)
//! e `kill_process_group` (o `kill(-pgid)` que `BridgeConnection::kill` dispara)
//! —, sem Node e sem telefone: a "ponte" aqui e um `sh` que lanca um `sleep`
//! neto. Sem o `killpg`, matar o `sh` deixaria o `sleep` orfao (reparentado ao
//! init); o teste prova que ele morre junto.
//!
//! `#[cfg(unix)]`: `setpgid`/grupo de processos sao premissa de Unix, e `sh`
//! esta na PATH. No Windows esse papel e do Job Object (`jobobject.rs`),
//! testado em `jobobject::tests`. Morte ABRUPTA do pai no macOS (onde nao ha
//! PDEATHSIG) fica com o watch de stdin do lado do bridge, provado por
//! `node --test` em `bridge/whatsapp`.
#![cfg(unix)]
#![cfg(feature = "whatsapp-linked")]

use std::path::PathBuf;
use std::process::Command as StdCommand;
use std::time::{Duration, Instant};

use garraia_channels::whatsapp_linked::bridge::{BridgeConnection, BridgeError, BridgeLauncher};
use tokio::process::Command;

/// Lancador cuja "ponte" e um `sh` que forka um `sleep` neto e grava o pid do
/// neto num arquivo, depois segura o filho vivo com `wait`.
struct GroupTreeLauncher {
    gc_pidfile: PathBuf,
    dir: PathBuf,
}

impl BridgeLauncher for GroupTreeLauncher {
    fn command(&self) -> Result<Command, BridgeError> {
        let mut cmd = Command::new("sh");
        // O path do tempdir nao tem aspas simples; a interpolacao e segura.
        let script = format!(
            "sleep 300 & echo $! > '{}'; wait",
            self.gc_pidfile.display()
        );
        cmd.arg("-c").arg(script);
        Ok(cmd)
    }

    fn describe(&self) -> String {
        "sh -c <arvore de grupo de teste>".into()
    }

    fn dir(&self) -> PathBuf {
        self.dir.clone()
    }
}

/// `kill -0 <pid>` — sucesso = o processo existe. Portavel entre Linux e macOS
/// (sem `/proc`, sem `libc` como dev-dep).
fn pid_vivo(pid: u32) -> bool {
    StdCommand::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .is_ok_and(|s| s.success())
}

fn morreu_ate(pid: u32, prazo: Duration) -> bool {
    let fim = Instant::now() + prazo;
    while Instant::now() < fim {
        if !pid_vivo(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    !pid_vivo(pid)
}

async fn esperar_pid_no_arquivo(path: &std::path::Path, prazo: Duration) -> Option<u32> {
    let fim = Instant::now() + prazo;
    while Instant::now() < fim {
        if let Ok(texto) = std::fs::read_to_string(path)
            && let Ok(pid) = texto.trim().parse::<u32>()
        {
            return Some(pid);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    None
}

/// `kill` (o teardown normal) varre o grupo: o `sleep` neto nao fica orfao.
#[tokio::test]
async fn kill_varre_o_grupo_e_o_neto_nao_fica_orfao() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let gc_pidfile = tmp.path().join("gc.pid");
    let launcher = GroupTreeLauncher {
        gc_pidfile: gc_pidfile.clone(),
        dir: tmp.path().to_path_buf(),
    };

    let mut conn = BridgeConnection::spawn(&launcher).await.expect("spawn");

    let neto = esperar_pid_no_arquivo(&gc_pidfile, Duration::from_secs(10))
        .await
        .expect("o neto nao gravou o pid a tempo");
    assert!(
        pid_vivo(neto),
        "o neto {neto} deveria estar vivo antes do kill"
    );

    // Roda o `kill_process_group` real (killpg) + `start_kill`.
    conn.kill().await;

    assert!(
        morreu_ate(neto, Duration::from_secs(10)),
        "o neto {neto} sobreviveu ao teardown — ficou orfao, que e o bug #1636"
    );
}

/// O mesmo pelo caminho do `Drop` (saida por erro/panic), sem `kill` explicito.
#[tokio::test]
async fn drop_varre_o_grupo_e_o_neto_nao_fica_orfao() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let gc_pidfile = tmp.path().join("gc.pid");
    let launcher = GroupTreeLauncher {
        gc_pidfile: gc_pidfile.clone(),
        dir: tmp.path().to_path_buf(),
    };

    let neto = {
        let conn = BridgeConnection::spawn(&launcher).await.expect("spawn");
        let pid = esperar_pid_no_arquivo(&gc_pidfile, Duration::from_secs(10))
            .await
            .expect("o neto nao gravou o pid a tempo");
        assert!(
            pid_vivo(pid),
            "o neto {pid} deveria estar vivo antes do drop"
        );
        drop(conn); // <- sem kill() explicito: so o Drop
        pid
    };

    assert!(
        morreu_ate(neto, Duration::from_secs(10)),
        "o neto {neto} sobreviveu ao Drop — ficou orfao, que e o bug #1636"
    );
}
