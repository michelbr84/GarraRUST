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
