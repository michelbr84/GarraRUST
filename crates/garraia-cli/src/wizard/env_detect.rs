//! Environment detection probes — plan 0126 §M1.3.
//!
//! Pure read-only detection. Never mutates the filesystem or installs
//! anything. The wizard orchestrator calls [`detect`] once and routes
//! prompts based on the returned [`EnvSnapshot`].
//!
//! All probes are infallible at the type level — failures collapse to
//! `false` / `None` / `OllamaState::NotFound` so the wizard never aborts
//! because a command was missing or a port query timed out.
//!
//! Probe latency caps (wall-clock):
//!
//! * `nvidia-smi -L` — 5 s (slow GPUs can take >1 s)
//! * `systemctl is-system-running` — 2 s
//! * Ollama HTTP `/api/tags` — 1 s
//! * TCP port `bind` — synchronous, near-instant
//!
//! Tests inject [`FakeProbe`] to deterministically cover the
//! presence/absence matrix of each detected capability without exercising
//! the host machine's `nvidia-smi` / `systemctl` / `ollama`.

#![allow(dead_code)] // M1.7 orchestrator wires these up; until then the
// module exposes them only to its own tests.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

/// Aggregated result of all probes run by [`detect`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvSnapshot {
    pub os: OsId,
    pub is_root: bool,
    pub is_runpod: bool,
    pub has_systemd: bool,
    pub has_nvidia: bool,
    /// First line of `nvidia-smi -L` when [`Self::has_nvidia`]. Useful
    /// for the wizard's one-line GPU summary.
    pub gpu_summary: Option<String>,
    pub ollama: OllamaState,
    pub ports: PortReport,
}

impl EnvSnapshot {
    /// Returns `true` when the wizard should offer the local AI stack
    /// prompts (Ollama install + Qwen3 pull + voice endpoints).
    ///
    /// Honored only by the orchestrator, not the probes themselves —
    /// keeps detection decoupled from policy.
    pub fn supports_local_stack(&self) -> bool {
        self.has_nvidia
    }

    /// Heuristic for "server / RunPod / cloud VM" — drives the choice of
    /// `gateway.host = 0.0.0.0` over `127.0.0.1` in the emitted config.
    pub fn is_server_like(&self) -> bool {
        self.is_runpod || self.is_root
    }
}

/// Operating-system identification. The current PR only differentiates
/// the broad family — the wizard does not branch on distro yet, but the
/// `Linux { distro, version }` payload is surfaced to the operator so
/// `garraia init` reports something useful at the top.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OsId {
    Linux { distro: String, version: String },
    MacOs,
    Windows,
    Unknown,
}

/// Whether Ollama is installed and / or running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OllamaState {
    NotFound,
    InstalledNotRunning,
    Running { models: Vec<String> },
}

impl OllamaState {
    pub fn is_running(&self) -> bool {
        matches!(self, Self::Running { .. })
    }
}

/// Per-port availability for the well-known ports the wizard cares about.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PortReport {
    pub gateway_3888: PortStatus,
    pub admin_8080: PortStatus,
    pub ollama_11434: PortStatus,
    pub tts_7860: PortStatus,
    pub stt_9090: PortStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PortStatus {
    /// The wizard could `bind` this port on `127.0.0.1` — nothing is
    /// listening locally.
    #[default]
    Free,
    /// `bind` returned an error — something else holds the port.
    InUse,
}

/// Trait abstracting every external observation the env probes make,
/// so unit tests can supply a [`FakeProbe`] without touching the host.
///
/// `RealProbe` (the production impl) shells out to `nvidia-smi`,
/// `systemctl`, `which ollama`, and the local network stack. `FakeProbe`
/// returns whatever the test wants.
pub trait EnvProbe {
    fn os_release(&self) -> Option<String>;
    fn is_root(&self) -> bool;
    fn env(&self, key: &str) -> Option<String>;
    fn path_exists(&self, path: &Path) -> bool;
    fn run_with_timeout(&self, program: &str, args: &[&str], timeout: Duration) -> ProcessOutput;
    /// `GET http://127.0.0.1:11434/api/tags` returns body when reachable.
    fn http_get(&self, url: &str, timeout: Duration) -> Option<String>;
    fn port_status(&self, port: u16) -> PortStatus;
}

/// Lightweight `Command::output()`-style result with truncated streams.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProcessOutput {
    pub success: bool,
    pub stdout: String,
}

// ---------- Production impl ---------------------------------------------------

/// Real probes against the host. Used by [`detect`].
pub struct RealProbe;

impl EnvProbe for RealProbe {
    fn os_release(&self) -> Option<String> {
        std::fs::read_to_string("/etc/os-release").ok()
    }

    fn is_root(&self) -> bool {
        #[cfg(unix)]
        unsafe {
            libc::geteuid() == 0
        }
        #[cfg(not(unix))]
        {
            false
        }
    }

    fn env(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    fn path_exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn run_with_timeout(&self, program: &str, args: &[&str], timeout: Duration) -> ProcessOutput {
        use std::process::{Command, Stdio};
        use std::time::Instant;

        let mut child = match Command::new(program)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(_) => return ProcessOutput::default(),
        };

        let deadline = Instant::now() + timeout;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let mut stdout = String::new();
                    if let Some(mut out) = child.stdout.take() {
                        use std::io::Read;
                        let _ = out.read_to_string(&mut stdout);
                    }
                    return ProcessOutput {
                        success: status.success(),
                        stdout,
                    };
                }
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return ProcessOutput::default();
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(_) => return ProcessOutput::default(),
            }
        }
    }

    fn http_get(&self, url: &str, timeout: Duration) -> Option<String> {
        // Raw HTTP/1.1 GET over a TcpStream. Used only for the local
        // Ollama probe (`http://127.0.0.1:11434/api/tags`), so a tiny
        // hand-rolled client is enough — no need to drag in
        // `reqwest::blocking` (which would require enabling the
        // `blocking` feature on the workspace `reqwest`).
        let stripped = url.strip_prefix("http://")?;
        let (host_port, path) = match stripped.split_once('/') {
            Some((hp, rest)) => (hp.to_string(), format!("/{rest}")),
            None => (stripped.to_string(), "/".to_string()),
        };
        let addrs = host_port.to_socket_addrs().ok()?;
        let mut last_err: Option<std::io::Error> = None;
        for addr in addrs {
            match TcpStream::connect_timeout(&addr, timeout) {
                Ok(mut stream) => {
                    let _ = stream.set_read_timeout(Some(timeout));
                    let _ = stream.set_write_timeout(Some(timeout));
                    let req = format!(
                        "GET {path} HTTP/1.1\r\nHost: {host_port}\r\nConnection: close\r\nUser-Agent: garraia-init\r\n\r\n"
                    );
                    if stream.write_all(req.as_bytes()).is_err() {
                        continue;
                    }
                    let mut buf = Vec::with_capacity(4096);
                    let _ = stream.read_to_end(&mut buf);
                    let raw = String::from_utf8_lossy(&buf).into_owned();
                    // Split off the response body after the blank line.
                    let body = raw
                        .split_once("\r\n\r\n")
                        .map(|(_, b)| b.to_string())
                        .unwrap_or(raw);
                    return Some(body);
                }
                Err(e) => last_err = Some(e),
            }
        }
        let _ = last_err;
        None
    }

    fn port_status(&self, port: u16) -> PortStatus {
        let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
        match TcpListener::bind(addr) {
            Ok(_listener) => PortStatus::Free,
            Err(_) => PortStatus::InUse,
        }
    }
}

// ---------- Orchestrator entry point ------------------------------------------

/// Run every probe via [`RealProbe`] and return an [`EnvSnapshot`].
pub fn detect() -> EnvSnapshot {
    detect_with(&RealProbe)
}

/// Generic over the probe so tests can inject `FakeProbe`.
pub fn detect_with<P: EnvProbe>(probe: &P) -> EnvSnapshot {
    let os = detect_os(probe);
    let is_root = probe.is_root();
    let is_runpod = detect_runpod(probe);
    let has_systemd = detect_systemd(probe);
    let (has_nvidia, gpu_summary) = detect_nvidia(probe);
    let ollama = detect_ollama(probe);
    let ports = PortReport {
        gateway_3888: probe.port_status(3888),
        admin_8080: probe.port_status(8080),
        ollama_11434: probe.port_status(11434),
        tts_7860: probe.port_status(7860),
        stt_9090: probe.port_status(9090),
    };

    EnvSnapshot {
        os,
        is_root,
        is_runpod,
        has_systemd,
        has_nvidia,
        gpu_summary,
        ollama,
        ports,
    }
}

fn detect_os<P: EnvProbe>(probe: &P) -> OsId {
    if cfg!(target_os = "macos") {
        return OsId::MacOs;
    }
    if cfg!(target_os = "windows") {
        return OsId::Windows;
    }
    let release = match probe.os_release() {
        Some(s) => s,
        None => return OsId::Unknown,
    };
    let mut distro = String::new();
    let mut version = String::new();
    for line in release.lines() {
        if let Some(rest) = line.strip_prefix("ID=") {
            distro = unquote(rest);
        } else if let Some(rest) = line.strip_prefix("VERSION_ID=") {
            version = unquote(rest);
        }
    }
    if distro.is_empty() {
        OsId::Unknown
    } else {
        OsId::Linux { distro, version }
    }
}

fn unquote(s: &str) -> String {
    s.trim_matches(|c| c == '"' || c == '\'').to_string()
}

fn detect_runpod<P: EnvProbe>(probe: &P) -> bool {
    probe.env("RUNPOD_POD_ID").is_some()
        || probe.env("RUNPOD_API_KEY").is_some()
        || probe.env("RUNPOD_PUBLIC_IP").is_some()
}

fn detect_systemd<P: EnvProbe>(probe: &P) -> bool {
    if !probe.path_exists(Path::new("/run/systemd/system")) {
        return false;
    }
    let out = probe.run_with_timeout("systemctl", &["is-system-running"], Duration::from_secs(2));
    // `systemctl is-system-running` exits non-zero in degraded mode but
    // still indicates systemd is present. Both `success` and a
    // non-empty stdout (e.g. "degraded", "running") count as detected.
    out.success || !out.stdout.trim().is_empty()
}

fn detect_nvidia<P: EnvProbe>(probe: &P) -> (bool, Option<String>) {
    let out = probe.run_with_timeout("nvidia-smi", &["-L"], Duration::from_secs(5));
    if !out.success || out.stdout.trim().is_empty() {
        return (false, None);
    }
    let summary = out.stdout.lines().next().map(|s| s.to_string());
    (true, summary)
}

fn detect_ollama<P: EnvProbe>(probe: &P) -> OllamaState {
    let which = probe.run_with_timeout("ollama", &["--version"], Duration::from_secs(2));
    if !which.success {
        return OllamaState::NotFound;
    }
    // Installed — check if the daemon is listening.
    let body = probe.http_get("http://127.0.0.1:11434/api/tags", Duration::from_secs(1));
    match body {
        Some(json) => OllamaState::Running {
            models: parse_ollama_tags(&json),
        },
        None => OllamaState::InstalledNotRunning,
    }
}

/// Parse a tolerant subset of the `/api/tags` JSON. Returns model names
/// or an empty vec if anything is wrong.
fn parse_ollama_tags(body: &str) -> Vec<String> {
    let value: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    value
        .get("models")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

// ---------- Tests --------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// In-memory probe for deterministic testing. Each field overrides one
    /// probe call; defaults are "nothing found".
    #[derive(Default)]
    struct FakeProbe {
        os_release_body: Option<String>,
        is_root: bool,
        env: HashMap<String, String>,
        existing_paths: Vec<&'static str>,
        commands: HashMap<String, ProcessOutput>,
        http_responses: HashMap<String, String>,
        free_ports: Vec<u16>,
    }

    impl EnvProbe for FakeProbe {
        fn os_release(&self) -> Option<String> {
            self.os_release_body.clone()
        }
        fn is_root(&self) -> bool {
            self.is_root
        }
        fn env(&self, key: &str) -> Option<String> {
            self.env.get(key).cloned()
        }
        fn path_exists(&self, path: &Path) -> bool {
            let s = path.to_string_lossy().to_string();
            self.existing_paths.iter().any(|p| *p == s)
        }
        fn run_with_timeout(
            &self,
            program: &str,
            args: &[&str],
            _timeout: Duration,
        ) -> ProcessOutput {
            let key = format!("{} {}", program, args.join(" "));
            self.commands.get(&key).cloned().unwrap_or_default()
        }
        fn http_get(&self, url: &str, _timeout: Duration) -> Option<String> {
            self.http_responses.get(url).cloned()
        }
        fn port_status(&self, port: u16) -> PortStatus {
            if self.free_ports.contains(&port) {
                PortStatus::Free
            } else {
                PortStatus::InUse
            }
        }
    }

    fn ok_cmd(stdout: &str) -> ProcessOutput {
        ProcessOutput {
            success: true,
            stdout: stdout.to_string(),
        }
    }

    #[test]
    fn no_gpu_no_ollama_no_runpod_no_systemd_yields_minimal_snapshot() {
        let probe = FakeProbe::default();
        let snap = detect_with(&probe);
        assert!(!snap.has_nvidia);
        assert!(matches!(snap.ollama, OllamaState::NotFound));
        assert!(!snap.is_runpod);
        assert!(!snap.has_systemd);
        assert!(!snap.supports_local_stack());
        assert!(!snap.is_server_like());
    }

    #[test]
    fn nvidia_smi_zero_exit_with_summary_sets_gpu() {
        let mut probe = FakeProbe::default();
        probe.commands.insert(
            "nvidia-smi -L".into(),
            ok_cmd("GPU 0: NVIDIA A100 (UUID: GPU-abc123)\n"),
        );
        let snap = detect_with(&probe);
        assert!(snap.has_nvidia);
        assert_eq!(
            snap.gpu_summary.as_deref(),
            Some("GPU 0: NVIDIA A100 (UUID: GPU-abc123)")
        );
        assert!(snap.supports_local_stack());
    }

    #[test]
    fn ollama_installed_but_daemon_offline() {
        let mut probe = FakeProbe::default();
        probe
            .commands
            .insert("ollama --version".into(), ok_cmd("ollama version 0.5.6"));
        // No http_responses entry → http_get returns None → InstalledNotRunning.
        let snap = detect_with(&probe);
        assert_eq!(snap.ollama, OllamaState::InstalledNotRunning);
    }

    #[test]
    fn ollama_running_returns_model_names() {
        let mut probe = FakeProbe::default();
        probe
            .commands
            .insert("ollama --version".into(), ok_cmd("ollama version 0.5.6"));
        probe.http_responses.insert(
            "http://127.0.0.1:11434/api/tags".into(),
            r#"{"models":[{"name":"qwen3:latest"},{"name":"llama3:8b"}]}"#.into(),
        );
        let snap = detect_with(&probe);
        assert!(snap.ollama.is_running());
        match &snap.ollama {
            OllamaState::Running { models } => {
                assert_eq!(
                    models,
                    &vec!["qwen3:latest".to_string(), "llama3:8b".to_string()]
                );
            }
            other => panic!("expected Running, got {other:?}"),
        }
    }

    #[test]
    fn runpod_env_marks_server_like() {
        let mut probe = FakeProbe::default();
        probe.env.insert("RUNPOD_POD_ID".into(), "abc123".into());
        let snap = detect_with(&probe);
        assert!(snap.is_runpod);
        assert!(snap.is_server_like());
    }

    #[test]
    fn systemd_present_when_run_dir_exists_and_systemctl_responds() {
        let mut probe = FakeProbe::default();
        probe.existing_paths.push("/run/systemd/system");
        probe
            .commands
            .insert("systemctl is-system-running".into(), ok_cmd("running"));
        let snap = detect_with(&probe);
        assert!(snap.has_systemd);
    }

    #[test]
    fn systemd_absent_when_run_dir_missing_even_if_systemctl_exists() {
        let mut probe = FakeProbe::default();
        probe
            .commands
            .insert("systemctl is-system-running".into(), ok_cmd("running"));
        let snap = detect_with(&probe);
        assert!(!snap.has_systemd);
    }

    #[test]
    fn os_release_parsed_into_linux_branch() {
        let probe = FakeProbe {
            os_release_body: Some("NAME=\"Ubuntu\"\nID=ubuntu\nVERSION_ID=\"22.04\"\n".into()),
            ..FakeProbe::default()
        };
        let snap = detect_with(&probe);
        // On macOS / Windows runs of cargo test the cfg!() guard wins, so
        // gate the assertion to Linux test environments only.
        if cfg!(target_os = "linux") {
            match snap.os {
                OsId::Linux { distro, version } => {
                    assert_eq!(distro, "ubuntu");
                    assert_eq!(version, "22.04");
                }
                other => panic!("expected Linux, got {other:?}"),
            }
        }
    }

    #[test]
    fn port_report_routes_each_known_port() {
        let probe = FakeProbe {
            free_ports: vec![3888, 11434],
            ..FakeProbe::default()
        };
        let snap = detect_with(&probe);
        assert_eq!(snap.ports.gateway_3888, PortStatus::Free);
        assert_eq!(snap.ports.ollama_11434, PortStatus::Free);
        assert_eq!(snap.ports.admin_8080, PortStatus::InUse);
        assert_eq!(snap.ports.tts_7860, PortStatus::InUse);
        assert_eq!(snap.ports.stt_9090, PortStatus::InUse);
    }
}
