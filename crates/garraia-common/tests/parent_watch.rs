//! Teste de ponta a ponta da vigia de pai (#1636): a vigia sai quando o PID
//! que ela observa morre de forma abrupta.
//!
//! Reproduz o mecanismo de `garra start --exit-with-parent`: o bin
//! `parent_watch_fixture` observa um PID; quando esse processo e morto com
//! `SIGKILL` (e colhido, para o PID nao ficar zumbi), a vigia detecta o sumico
//! e sai. Sem isso, no Linux/macOS o sidecar ficaria orfao numa morte abrupta
//! do app desktop.
//!
//! `#[cfg(unix)]`: a vigia por PID so e significativa no Unix — no Windows o
//! Job Object cobre o par app->sidecar, e `observe_parent` e no-op. A `garra`
//! so liga a task no Unix.
#![cfg(unix)]

use std::process::{Child, Command};
use std::time::{Duration, Instant};

fn ainda_rodando(child: &mut Child) -> bool {
    matches!(child.try_wait(), Ok(None))
}

#[test]
fn a_vigia_sai_quando_o_pid_observado_morre() {
    // A "vitima": um processo qualquer que a vigia vai observar.
    let mut victim = Command::new("sh")
        .arg("-c")
        .arg("sleep 300")
        .spawn()
        .expect("spawn da vitima");
    let victim_pid = victim.id();

    // A vigia: filho direto deste teste, observando o PID da vitima.
    let mut watcher = Command::new(env!("CARGO_BIN_EXE_parent_watch_fixture"))
        .arg(victim_pid.to_string())
        .spawn()
        .expect("spawn da vigia");

    // So prova algo se a vigia continua viva ENQUANTO a vitima vive.
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        ainda_rodando(&mut watcher),
        "a vigia saiu antes de a vitima morrer"
    );

    // Morte abrupta da vitima. Colher em seguida: um PID zumbi ainda "existe"
    // para `kill(pid, 0)`, e a vigia so deve ver o sumico depois do reap.
    victim.kill().expect("kill da vitima");
    victim.wait().expect("reap da vitima");

    // A vigia tem de sair em <= 10 s.
    let prazo = Instant::now() + Duration::from_secs(10);
    loop {
        match watcher.try_wait().expect("try_wait da vigia") {
            Some(status) => {
                assert!(status.success(), "a vigia saiu com erro: {status:?}");
                break;
            }
            None => {
                if Instant::now() >= prazo {
                    let _ = watcher.kill();
                    let _ = watcher.wait();
                    panic!("a vigia nao saiu em 10 s apos a morte do PID observado");
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}
