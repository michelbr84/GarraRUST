//! Process regressions for the Unix stop grace period (#1913).
//! Synthetic gateway/descendants only: no live config, ports or services.
use super::*;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

struct Fixture {
    gateway: Child,
    descendant: u32,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // The gateway owns and reaps the descendant on the successful path.
        // On a regression, clean up the isolated child before any assertion unwinds.
        if self.gateway.try_wait().ok().flatten().is_none() {
            let _ = kill_and_wait_with_grace(self.gateway.id(), std::time::Duration::from_secs(1));
        }
        if is_process_running(self.descendant) {
            unsafe { libc::kill(self.descendant as libc::pid_t, libc::SIGKILL) };
        }
        let _ = self.gateway.kill();
        let _ = self.gateway.wait();
    }
}

fn fixture(delay: &str) -> Fixture {
    let mut gateway = Command::new("python3")
        .args([
            "-u",
            "-c",
            r#"
import os, signal, subprocess, sys, time
os.setsid()
child = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(120)'], start_new_session=True)
# Block AFTER spawning the descendant so it does not inherit the mask.
# Block BEFORE readiness: an early SIGTERM stays pending until sigwait.
# A Python handler followed by pause can lose that wakeup (Python 3.14).
signal.pthread_sigmask(signal.SIG_BLOCK, {signal.SIGTERM})
print(child.pid, flush=True)
signal.sigwait({signal.SIGTERM})
time.sleep(float(sys.argv[1]))
child.terminate()
child.wait(timeout=5)
sys.exit(0)
"#,
            delay,
        ])
        .stdout(Stdio::piped())
        .spawn()
        .expect("python3 process fixture");
    let mut line = String::new();
    BufReader::new(gateway.stdout.take().expect("stdout"))
        .read_line(&mut line)
        .expect("fixture ready");
    let descendant = line.trim().parse().expect("descendant PID");
    Fixture {
        gateway,
        descendant,
    }
}

#[test]
fn stop_waits_for_cleanup_past_the_old_five_second_limit() {
    let mut target = fixture("6");
    let unrelated = fixture("0");
    assert!(is_process_running(target.descendant));
    assert!(kill_and_wait(target.gateway.id()));
    let status = target.gateway.wait().expect("reap gateway");
    assert!(is_process_running(unrelated.gateway.id()));
    assert!(is_process_running(unrelated.descendant));
    assert!(
        status.success(),
        "cleanup was interrupted by force kill: {status}"
    );
    assert!(
        !is_process_running(target.descendant),
        "descendant survived cleanup"
    );
}

#[test]
fn stop_still_escalates_when_the_grace_expires() {
    use std::os::unix::process::ExitStatusExt;
    let mut target = fixture("60");
    assert!(kill_and_wait_with_grace(
        target.gateway.id(),
        std::time::Duration::from_millis(100),
    ));
    assert_eq!(
        target.gateway.wait().expect("reap gateway").signal(),
        Some(libc::SIGKILL)
    );
    // The forced path cannot guarantee descendant cleanup: keep that limitation
    // explicit, and let Fixture::drop clean up this synthetic process.
    assert!(is_process_running(target.descendant));
}

#[test]
fn stop_returns_promptly_when_cleanup_finishes() {
    let mut target = fixture("0");
    let start = std::time::Instant::now();
    assert!(kill_and_wait(target.gateway.id()));
    assert!(target.gateway.wait().expect("reap gateway").success());
    assert!(!is_process_running(target.descendant));
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
}

/// A parada por porta pode receber vários PIDs do `lsof`. Antes, cada um
/// esperava o grace inteiro: dois alvos com 3 s de limpeza custavam ~6 s. Com
/// o orçamento único, os dois recebem `SIGTERM` juntos e a espera é paralela.
#[test]
fn collective_stop_spends_one_budget_for_every_target() {
    let mut primeiro = fixture("3");
    let mut segundo = fixture("3");
    let unrelated = fixture("0");

    let start = std::time::Instant::now();
    let resultados = kill_and_wait_many(
        &[primeiro.gateway.id(), segundo.gateway.id()],
        std::time::Duration::from_secs(20),
    );
    let decorrido = start.elapsed();

    assert_eq!(resultados.len(), 2, "um resultado por PID, na ordem pedida");
    assert!(
        resultados.iter().all(|(_, saiu)| *saiu),
        "os dois alvos devem sair dentro do orçamento: {resultados:?}"
    );
    assert!(primeiro.gateway.wait().expect("reap primeiro").success());
    assert!(segundo.gateway.wait().expect("reap segundo").success());
    assert!(!is_process_running(primeiro.descendant));
    assert!(!is_process_running(segundo.descendant));

    // Sequencial seriam ~6 s; o orçamento único fica perto dos 3 s de cada
    // limpeza. A folga cobre a fila do runner sem deixar passar o caso N × grace.
    assert!(
        decorrido < std::time::Duration::from_secs(5),
        "parada coletiva gastou {decorrido:?} — orçamento não foi compartilhado"
    );

    // Um processo alheio não faz parte do pedido e continua vivo.
    assert!(is_process_running(unrelated.gateway.id()));
    assert!(is_process_running(unrelated.descendant));
}

/// A escalada para `SIGKILL` só atinge o processo que recebeu o `SIGTERM`.
#[test]
fn escalation_is_skipped_when_the_pid_was_recycled() {
    assert!(pode_escalar(Some(7), Some(7)), "mesmo processo: escala");
    assert!(
        !pode_escalar(Some(7), Some(9)),
        "start-time diferente = PID reciclado: não escala"
    );
    assert!(
        pode_escalar(Some(7), None),
        "identidade atual ilegível: mantém o comportamento anterior"
    );
    assert!(
        pode_escalar(None, Some(9)),
        "identidade não capturada: mantém o comportamento anterior"
    );
}

/// A identidade é estável para o mesmo processo e ausente para quem não existe.
#[test]
fn process_identity_comes_from_the_start_time() {
    let proprio = std::process::id();
    let primeira = identidade_do_processo(proprio).expect("start-time do próprio processo");
    let segunda = identidade_do_processo(proprio).expect("start-time do próprio processo");
    assert_eq!(primeira, segunda, "start-time não muda durante a vida");
    assert!(
        identidade_do_processo(u32::MAX - 1).is_none(),
        "PID inexistente não tem identidade"
    );
}

/// Um alvo que já era zumbi antes do pedido não é escalado nem contado como
/// vivo: o `SIGTERM` a um zumbi não faz nada e o `stop` não deve esperar.
#[test]
fn collective_stop_returns_at_once_for_an_already_dead_target() {
    // Filho já saído e ainda não colhido: o PID continua sendo nosso (não há
    // risco de reciclagem) e `is_process_running` o vê como zumbi.
    let mut morto = Command::new("true").spawn().expect("processo efêmero");
    let pid = morto.id();
    std::thread::sleep(std::time::Duration::from_millis(100));
    let inicio = std::time::Instant::now();
    let resultados = kill_and_wait_many(&[pid], std::time::Duration::from_secs(10));
    assert_eq!(resultados, vec![(pid, true)]);
    assert!(inicio.elapsed() < std::time::Duration::from_secs(2));
    let _ = morto.wait();
}
