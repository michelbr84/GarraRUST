//! Teste de orfao de ponta a ponta (#1636), Windows: mata o PAI de forma
//! abrupta e afirma que nenhum descendente sobrevive.
//!
//! O bin `orphan_fixture` monta a arvore pai -> filho -> neto; o pai cria um
//! Job Object `KILL_ON_JOB_CLOSE` (o mesmo [`garraia_desktop_core::KillOnCloseJob`]
//! que o `Supervisor` usa) e assina o filho — o neto entra no job por heranca.
//! Aqui o teste mata o pai com `TerminateProcess` (via `Child::kill`), que e o
//! que o Gerenciador de Tarefas faz e nao roda nenhum `Drop`: a unica coisa que
//! encerra a arvore e o SO fechar o handle do job quando o pai morre.
//!
//! `#[cfg(windows)]`: e o Job Object que esta sob teste. No Unix a garantia e
//! outra (grupo de processos + watch de stdin do bridge), provada em
//! `garraia-channels` e no `node --test` da ponte.
#![cfg(windows)]

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
};

/// 259 (STILL_ACTIVE) enquanto roda. `GetExitCodeProcess` em vez de
/// `WaitForSingleObject` porque este exige `SYNCHRONIZE`, que
/// `PROCESS_QUERY_LIMITED_INFORMATION` nao da.
const STILL_ACTIVE_U32: u32 = 259;

fn esta_vivo(pid: u32) -> bool {
    // SAFETY: abre com o direito minimo de consulta; NULL = ja nao existe.
    let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if h.is_null() {
        return false;
    }
    let mut code: u32 = 0;
    // SAFETY: `h` e um handle valido recem-aberto; `code` e um u32 vivo.
    let ok = unsafe { GetExitCodeProcess(h, &mut code) };
    // SAFETY: fecha o handle de consulta, uma unica vez.
    unsafe { CloseHandle(h) };
    ok != 0 && code == STILL_ACTIVE_U32
}

fn morreu_ate(pid: u32, prazo: Duration) -> bool {
    let fim = Instant::now() + prazo;
    while Instant::now() < fim {
        if !esta_vivo(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    !esta_vivo(pid)
}

fn esperar_pid(path: &Path, prazo: Duration) -> Option<u32> {
    let fim = Instant::now() + prazo;
    while Instant::now() < fim {
        if let Ok(texto) = std::fs::read_to_string(path)
            && let Ok(pid) = texto.trim().parse::<u32>()
        {
            return Some(pid);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    None
}

#[test]
fn matar_o_pai_abruptamente_nao_deixa_descendente_orfao() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let child_file = tmp.path().join("child.pid");
    let grand_file = tmp.path().join("grand.pid");

    let mut pai = Command::new(env!("CARGO_BIN_EXE_orphan_fixture"))
        .arg("parent")
        .arg(&child_file)
        .arg(&grand_file)
        .spawn()
        .expect("spawn do fixture parent");

    let child_pid =
        esperar_pid(&child_file, Duration::from_secs(15)).expect("pid do filho a tempo");
    let grand_pid = esperar_pid(&grand_file, Duration::from_secs(15)).expect("pid do neto a tempo");

    // So prova algo se a arvore estava viva antes de matarmos o pai.
    assert!(
        esta_vivo(child_pid),
        "o filho {child_pid} deveria estar vivo"
    );
    assert!(
        esta_vivo(grand_pid),
        "o neto {grand_pid} deveria estar vivo"
    );

    // Morte ABRUPTA do pai: `Child::kill` chama `TerminateProcess`, como o
    // Gerenciador de Tarefas. Nenhum `Drop` do pai roda; so o SO fechando o
    // handle do job encerra a arvore.
    pai.kill().expect("mata o pai");
    let _ = pai.wait();

    assert!(
        morreu_ate(child_pid, Duration::from_secs(10)),
        "o filho {child_pid} sobreviveu a morte abrupta do pai — orfao (#1636)"
    );
    assert!(
        morreu_ate(grand_pid, Duration::from_secs(10)),
        "o neto {grand_pid} sobreviveu a morte abrupta do pai — orfao (#1636)"
    );
}
