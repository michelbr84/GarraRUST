//! Closes the procfs channel from a tool child back to its parent's secrets.
//!
//! # The hole
//!
//! `#1075` scrubbed the environment a tool child inherits: it gets `PATH`,
//! `HOME`, `LANG`, `LC_ALL`, `TERM`, `USER` and nothing else. That closes
//! *inheritance*. It does not close *procfs*: a child of the same UID can open
//! `/proc/<ppid>/environ` and read the parent's environment directly, which on
//! a gateway is where `GARRAIA_JWT_SECRET`, `ANTHROPIC_API_KEY` and friends
//! live. The command gate lists `environ` as a pattern needing confirmation,
//! but that only inspects the text of a command — not what runs *inside* a
//! script the gate made someone confirm without reading.
//!
//! Measured on Linux 7.0, before and after:
//!
//! ```text
//! baseline          child sees: SEGREDO_DO_PAI=abracadabra
//! PR_SET_DUMPABLE=0 child sees: (nothing — EACCES)
//! ```
//!
//! # Why `prctl` and not Landlock
//!
//! The evaluation written for `#1078` reached for Landlock, and rejected it as
//! too big to land: it needs a per-platform mechanism, and above all a
//! *policy* — which paths may the agent read and write? — that is a product
//! decision nobody had made.
//!
//! `PR_SET_DUMPABLE` needs no policy. It does not restrict the child at all;
//! it makes the **parent** unreadable, which is the actual asymmetry being
//! exploited. One syscall, no allow-list to get wrong, nothing legitimate
//! broken: everything the process needs from its own `/proc` — `self/exe`,
//! `self/maps`, `self/cmdline`, `self/status`, `self/fd` — keeps working;
//! `environ` is what becomes root-only, and nothing reads it through procfs.
//!
//! This is not a sandbox and does not pretend to be one. It closes one named
//! channel. Confining what a tool child may touch on disk is still open, still
//! needs a policy decision, and is still where Landlock belongs — see
//! `docs/adr/0019-process-hardening-and-sandbox.md`.
//!
//! # Platforms
//!
//! Linux and Android only, because the hole is Linux-only: macOS and Windows
//! have no `/proc/<pid>/environ` to read. The "three implementations" problem
//! the evaluation worried about does not arise for *this* channel.

/// What [`harden_current_process`] managed to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hardening {
    /// The parent's `/proc/<pid>/environ` is now root-only.
    ProcfsClosed,

    /// The platform has no such channel to close (macOS, Windows).
    NotApplicable,

    /// The kernel refused. The process keeps running — a gateway that will not
    /// start because a hardening knob failed is a worse outcome than one that
    /// starts and says so.
    Failed,
}

impl Hardening {
    /// For logs and `garra status`.
    pub fn describe(self) -> &'static str {
        match self {
            Hardening::ProcfsClosed => {
                "procfs fechado: /proc/<pid>/environ deste processo e root-only"
            }
            Hardening::NotApplicable => "sem canal procfs nesta plataforma; nada a fechar",
            Hardening::Failed => {
                "prctl(PR_SET_DUMPABLE) falhou; o environ deste processo segue legivel por processos do mesmo UID"
            }
        }
    }
}

/// Makes this process's `/proc/<pid>/environ` unreadable to other processes of
/// the same user.
///
/// Call it as early as possible in `main`, before any tool child can be
/// spawned. It is idempotent and cheap.
///
/// Deliberately **not** fail-closed. The failure mode is a kernel that does
/// not support the knob, and refusing to boot the gateway there would trade a
/// narrow information leak for a total outage. The result is returned so the
/// caller can log it and so `garra status` can report it.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn harden_current_process() -> Hardening {
    // SAFETY: `prctl(PR_SET_DUMPABLE, 0)` takes no pointers, affects only the
    // calling process and cannot fail in a way that leaves state half-set.
    let rc = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0) };
    if rc == 0 {
        Hardening::ProcfsClosed
    } else {
        Hardening::Failed
    }
}

/// No `/proc/<pid>/environ` exists here, so there is nothing to close.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub fn harden_current_process() -> Hardening {
    Hardening::NotApplicable
}

/// Whether this process's environment is currently readable through procfs by
/// another process of the same user.
///
/// Asks the kernel (`PR_GET_DUMPABLE`) rather than remembering what we
/// requested, so a caller reporting the state reports what actually happened.
///
/// Not `/proc/self/status`: that file carries no `Dumpable:` line — checked on
/// Linux 7.0, where the field simply is not exposed there.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn environ_readable_by_same_uid() -> Option<bool> {
    // SAFETY: `PR_GET_DUMPABLE` takes no pointers and only reads state of the
    // calling process.
    let value = unsafe { libc::prctl(libc::PR_GET_DUMPABLE) };
    if value < 0 {
        return None;
    }
    // 0 = not dumpable. 1 (and the 2 of `suid_dumpable`) leave the environment
    // reachable by a same-UID process.
    Some(value != 0)
}

/// Not applicable off Linux.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub fn environ_readable_by_same_uid() -> Option<bool> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_never_empty() {
        for state in [
            Hardening::ProcfsClosed,
            Hardening::NotApplicable,
            Hardening::Failed,
        ] {
            assert!(!state.describe().is_empty());
        }
    }

    // ── The channel, end to end ──────────────────────────────────────────
    //
    // `PR_SET_DUMPABLE` is per-process: setting it inside the test binary
    // would silently change every other test in the same process. So the real
    // check runs across three processes, each one a re-exec of this same test
    // binary playing a role given by `GARRA_HARDENING_ROLE`:
    //
    //   driver  → spawns the subject and reads its verdict
    //   subject → carries a fake secret, hardens (or not), spawns the reader
    //   reader  → tries to read the *subject's* /proc/<pid>/environ
    //
    // The unhardened driver is not decoration: without it, a test asserting
    // "the child saw nothing" would also pass if the channel had never worked.

    #[cfg(any(target_os = "linux", target_os = "android"))]
    const ROLE: &str = "GARRA_HARDENING_ROLE";
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const SECRET: &str = "GARRA_HARDENING_FAKE_SECRET";
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const TARGET: &str = "GARRA_HARDENING_TARGET_PID";
    #[cfg(any(target_os = "linux", target_os = "android"))]
    const SECRET_VALUE: &str = "abracadabra-nao-e-segredo-de-verdade";

    /// The name the test harness knows a test in this module by.
    ///
    /// `--exact` matches the full path, so the bare function name selects
    /// nothing and the re-exec silently runs zero tests. Derived from
    /// `module_path!()` (minus the crate segment, which the harness omits) so
    /// that moving this module does not quietly turn these tests into no-ops.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn test_path(name: &str) -> String {
        let module = module_path!();
        let without_crate = module.split_once("::").map(|(_, r)| r).unwrap_or(module);
        format!("{without_crate}::{name}")
    }

    /// Re-runs this binary with only `role_test` enabled, under `role`.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn spawn_role(role: &str, role_test: &str, extra: &[(&str, String)]) -> String {
        use std::process::Command;

        let exe = std::env::current_exe().expect("test binary path");
        let mut cmd = Command::new(exe);
        cmd.args([&test_path(role_test), "--exact", "--nocapture"])
            .env(ROLE, role);
        for (key, value) in extra {
            cmd.env(key, value);
        }
        let out = cmd.output().expect("re-exec the test binary");
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    fn role() -> Option<String> {
        std::env::var(ROLE).ok()
    }

    /// Roles run *inside* re-executed copies. Without the marker they are
    /// no-ops, which is what keeps a plain `cargo test` from recursing.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn role_subject() {
        let Some(role) = role() else { return };
        if role != "subject" && role != "subject-unhardened" {
            return;
        }
        if role == "subject" {
            assert_eq!(harden_current_process(), Hardening::ProcfsClosed);
        }
        let seen = spawn_role(
            "reader",
            "role_reader",
            &[(TARGET, std::process::id().to_string())],
        );
        // Forward the reader's verdict to the driver.
        print!("{seen}");
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn role_reader() {
        let Some(role) = role() else { return };
        if role != "reader" {
            return;
        }
        let pid = std::env::var(TARGET).expect("target pid");
        let verdict = match std::fs::read(format!("/proc/{pid}/environ")) {
            Ok(bytes) => {
                if String::from_utf8_lossy(&bytes).contains(SECRET_VALUE) {
                    "CHILD_SAW=secret"
                } else {
                    "CHILD_SAW=other"
                }
            }
            Err(_) => "CHILD_SAW=denied",
        };
        println!("{verdict}");
    }

    /// The control: the channel is real, and this test can see it.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn without_hardening_a_child_reads_the_parent_environ() {
        if role().is_some() {
            return;
        }
        let out = spawn_role(
            "subject-unhardened",
            "role_subject",
            &[(SECRET, SECRET_VALUE.to_string())],
        );
        assert!(
            out.contains("CHILD_SAW=secret"),
            "the procfs channel should be open without hardening: {out}"
        );
    }

    /// The fix: same three processes, one `prctl` apart.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn after_hardening_a_child_cannot_read_the_parent_environ() {
        if role().is_some() {
            return;
        }
        let out = spawn_role(
            "subject",
            "role_subject",
            &[(SECRET, SECRET_VALUE.to_string())],
        );
        assert!(
            out.contains("CHILD_SAW=denied"),
            "the child should be refused by the kernel: {out}"
        );
    }

    /// The reported state comes from the kernel, not from a cached flag.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn kernel_reports_the_flip() {
        if role().is_some() {
            return;
        }
        let out = spawn_role("status", "role_status", &[]);
        assert!(out.contains("BEFORE=true"), "{out}");
        assert!(out.contains("AFTER=false"), "{out}");
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn role_status() {
        let Some(role) = role() else { return };
        if role != "status" {
            return;
        }
        println!("BEFORE={}", environ_readable_by_same_uid().unwrap());
        assert_eq!(harden_current_process(), Hardening::ProcfsClosed);
        println!("AFTER={}", environ_readable_by_same_uid().unwrap());
    }
}
