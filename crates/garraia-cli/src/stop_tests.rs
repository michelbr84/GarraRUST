//! Process regressions for the Unix stop grace period (#1913).
//! Synthetic gateway/descendants only: no live config, ports or services.
use super::*;
use std::io::{BufRead, BufReader};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::process::{Child, Command, Stdio};

/// The synthetic descendant, held by a pidfd instead of a bare PID (#1913).
///
/// The descendant is not our child: once it exits and something else reaps
/// it, its PID can be recycled. A bare PID would then make `vivo()` report an
/// unrelated process as the surviving descendant, and the cleanup in
/// [`Fixture::drop`] would `SIGKILL` that unrelated process. A pidfd keeps
/// naming the original process for as long as the handle is open.
struct Descendente {
    pid: u32,
    pidfd: OwnedFd,
}

impl Descendente {
    fn abrir(pid: u32) -> Self {
        // SAFETY: plain syscall; the result is checked before it becomes an fd.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0) };
        assert!(
            fd >= 0,
            "pidfd_open({pid}) failed: {}",
            std::io::Error::last_os_error()
        );
        // SAFETY: `fd` is a fresh descriptor owned by nobody else.
        let pidfd = unsafe { OwnedFd::from_raw_fd(fd as libc::c_int) };
        Self { pid, pidfd }
    }

    /// A pidfd becomes readable when its process exits, so "alive" means "not
    /// readable yet". An exited-but-unreaped descendant counts as gone, as in
    /// [`is_process_running`].
    fn vivo(&self) -> bool {
        let mut pfd = libc::pollfd {
            fd: self.pidfd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        loop {
            // SAFETY: one valid pollfd, zero timeout.
            let n = unsafe { libc::poll(&mut pfd, 1, 0) };
            if n >= 0 {
                return n == 0;
            }
            let erro = std::io::Error::last_os_error();
            assert_eq!(
                erro.kind(),
                std::io::ErrorKind::Interrupted,
                "poll(pidfd of {}) failed: {erro}",
                self.pid
            );
        }
    }

    /// Kills exactly this process; a no-op (ESRCH) when it is already gone.
    fn matar(&self) {
        // SAFETY: plain syscall on a pidfd we own; the result is ignored on
        // purpose (best-effort cleanup).
        unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.pidfd.as_raw_fd(),
                libc::SIGKILL,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            );
        }
    }
}

/// Owns the synthetic gateway from the moment it is spawned, so a readiness
/// read that fails (fixture crashed, malformed line) still kills and reaps it
/// instead of leaking it past the test (#1913).
struct GatewayGuard(Option<Child>);

impl GatewayGuard {
    fn child(&mut self) -> &mut Child {
        self.0.as_mut().expect("gateway still owned by the guard")
    }

    fn into_inner(mut self) -> Child {
        self.0.take().expect("gateway still owned by the guard")
    }
}

impl Drop for GatewayGuard {
    fn drop(&mut self) {
        if let Some(mut gateway) = self.0.take() {
            let _ = gateway.kill();
            let _ = gateway.wait();
        }
    }
}

struct Fixture {
    gateway: Child,
    descendant: Descendente,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // The gateway owns and reaps the descendant on the successful path.
        // On a regression, clean up the isolated child before any assertion unwinds.
        if self.gateway.try_wait().ok().flatten().is_none() {
            let _ = kill_and_wait_with_grace(self.gateway.id(), std::time::Duration::from_secs(1));
        }
        self.descendant.matar();
        let _ = self.gateway.kill();
        let _ = self.gateway.wait();
    }
}

fn fixture(delay: &str) -> Fixture {
    let mut gateway = GatewayGuard(Some(
        Command::new("python3")
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
            .expect("python3 process fixture"),
    ));
    let mut line = String::new();
    BufReader::new(gateway.child().stdout.take().expect("stdout"))
        .read_line(&mut line)
        .expect("fixture ready");
    let pid: u32 = line.trim().parse().expect("descendant PID");
    // The descendant sleeps for 120 s and nothing signals it before the test
    // acts, so the PID just printed still names it here. If readiness failed
    // above, the guard reaps the gateway; an orphaned descendant (own session)
    // is bounded by its 120 s sleep.
    let descendant = Descendente::abrir(pid);
    Fixture {
        gateway: gateway.into_inner(),
        descendant,
    }
}

/// The pidfd handle tracks exit of the exact process and never resolves to a
/// different one: after kill + exit it reports gone, and killing again is a
/// harmless no-op.
#[test]
fn descendant_handle_tracks_the_exact_process() {
    let mut proprio = Command::new("sleep").arg("30").spawn().expect("sleep");
    let alvo = Descendente::abrir(proprio.id());
    assert!(alvo.vivo());
    alvo.matar();
    let status = proprio.wait().expect("reap");
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(status.signal(), Some(libc::SIGKILL));
    assert!(!alvo.vivo(), "exited process must not read as alive");
    alvo.matar();
}

/// A fixture whose readiness line is unusable must not leak its gateway: the
/// guard kills and reaps it while the panic unwinds.
#[test]
fn failed_readiness_does_not_leak_the_gateway() {
    let child = Command::new("python3")
        .args([
            "-u",
            "-c",
            "import time; print('not-a-pid', flush=True); time.sleep(120)",
        ])
        .stdout(Stdio::piped())
        .spawn()
        .expect("python3");
    let pid = child.id();
    let identidade = identidade_do_processo(pid).expect("start time of the fixture");
    let resultado = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let mut gateway = GatewayGuard(Some(child));
        let mut line = String::new();
        BufReader::new(gateway.child().stdout.take().expect("stdout"))
            .read_line(&mut line)
            .expect("fixture ready");
        let _: u32 = line.trim().parse().expect("descendant PID");
    }));
    assert!(
        resultado.is_err(),
        "malformed readiness must fail the fixture"
    );
    // Reaped by the guard: the PID either no longer exists or (recycled)
    // belongs to a process with another start time.
    assert_ne!(
        identidade_do_processo(pid),
        Some(identidade),
        "gateway {pid} survived a failed readiness read"
    );
}

#[test]
fn stop_waits_for_cleanup_past_the_old_five_second_limit() {
    let mut target = fixture("6");
    let unrelated = fixture("0");
    assert!(target.descendant.vivo());
    assert!(kill_and_wait(target.gateway.id()));
    let status = target.gateway.wait().expect("reap gateway");
    assert!(is_process_running(unrelated.gateway.id()));
    assert!(unrelated.descendant.vivo());
    assert!(
        status.success(),
        "cleanup was interrupted by force kill: {status}"
    );
    assert!(!target.descendant.vivo(), "descendant survived cleanup");
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
    assert!(target.descendant.vivo());
}

#[test]
fn stop_returns_promptly_when_cleanup_finishes() {
    let mut target = fixture("0");
    let start = std::time::Instant::now();
    assert!(kill_and_wait(target.gateway.id()));
    assert!(target.gateway.wait().expect("reap gateway").success());
    assert!(!target.descendant.vivo());
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
    assert_eq!(
        resultados.iter().map(|(pid, _)| *pid).collect::<Vec<_>>(),
        vec![primeiro.gateway.id(), segundo.gateway.id()],
        "a ordem de saída acompanha a ordem pedida"
    );
    assert!(
        resultados.iter().all(|(_, saiu)| *saiu),
        "os dois alvos devem sair dentro do orçamento: {resultados:?}"
    );
    assert!(primeiro.gateway.wait().expect("reap primeiro").success());
    assert!(segundo.gateway.wait().expect("reap segundo").success());
    assert!(!primeiro.descendant.vivo());
    assert!(!segundo.descendant.vivo());

    // Sequencial seriam ~6 s; o orçamento único fica perto dos 3 s de cada
    // limpeza. A folga cobre a fila do runner sem deixar passar o caso N × grace.
    assert!(
        decorrido < std::time::Duration::from_secs(5),
        "parada coletiva gastou {decorrido:?} — orçamento não foi compartilhado"
    );

    // Um processo alheio não faz parte do pedido e continua vivo.
    assert!(is_process_running(unrelated.gateway.id()));
    assert!(unrelated.descendant.vivo());
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

/// O parsing trava o campo 22 mesmo com `comm` contendo espaço e parêntese —
/// o nome do executável do gateway pode ter os dois.
#[test]
fn stat_parsing_pins_field_22_after_the_last_parenthesis() {
    // Campos 3..=22 depois do ÚLTIMO `)`: starttime (22) = 987654.
    let linha =
        "1234 (garra) (x) y) S 1 1234 1234 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 987654 0 0";
    assert_eq!(start_time_do_stat(linha), Some(987654));
    assert_eq!(start_time_do_stat("sem parentese"), None);
    assert_eq!(
        start_time_do_stat("12 (a) S 1"),
        None,
        "linha curta não inventa campo"
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

/// Dois alvos que não saem: o prazo coletivo vale para os dois e a escalada
/// atinge cada um que continua sendo o mesmo processo.
#[test]
fn collective_stop_escalates_every_target_that_stays() {
    use std::os::unix::process::ExitStatusExt;
    let mut primeiro = fixture("60");
    let mut segundo = fixture("60");
    let inicio = std::time::Instant::now();
    let resultados = kill_and_wait_many(
        &[primeiro.gateway.id(), segundo.gateway.id()],
        std::time::Duration::from_millis(300),
    );
    assert!(
        resultados.iter().all(|(_, saiu)| *saiu),
        "os dois devem terminar pelo SIGKILL: {resultados:?}"
    );
    assert_eq!(
        primeiro.gateway.wait().expect("reap primeiro").signal(),
        Some(libc::SIGKILL)
    );
    assert_eq!(
        segundo.gateway.wait().expect("reap segundo").signal(),
        Some(libc::SIGKILL)
    );
    // Um alvo só já custaria grace + 2 s; dois alvos em sequência custariam o
    // dobro. Aqui os dois prazos são compartilhados.
    assert!(
        inicio.elapsed() < std::time::Duration::from_secs(4),
        "escalada coletiva levou {:?}",
        inicio.elapsed()
    );
}
