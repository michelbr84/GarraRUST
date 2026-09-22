//! L1 do smoke de instalacao limpa da v0.4.4, contra o binario de verdade:
//! `start -d` abria o `garraia.log` com `File::create`, entao cada start do
//! daemon apagava o log da execucao anterior (e, sem `O_APPEND`, o que o
//! daemon escrevia cru pelo stdout/stderr caia no offset 0, por cima do
//! tracing). Dois `start -d` seguidos tem de deixar as duas execucoes no log.
#![cfg(unix)]

use std::cell::RefCell;
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use tempfile::tempdir;

fn garra_bin() -> &'static str {
    env!("CARGO_BIN_EXE_garra")
}

fn porta_livre() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("porta efemera")
        .local_addr()
        .expect("local_addr")
        .port()
}

fn comando(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(garra_bin());
    cmd.args(args)
        .env("XDG_CONFIG_HOME", dir)
        .env("GARRAIA_CONFIG_DIR", dir)
        .env("HOME", dir)
        .env("GARRAIA_NO_SPINNER", "1")
        .env_remove("GARRAIA_ALLOW_INVALID_CONFIG")
        .env_remove("GARRAIA_GATEWAY_API_KEY")
        .env_remove("HOST")
        .env_remove("PORT")
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    cmd
}

fn log(dir: &Path) -> String {
    std::fs::read_to_string(dir.join("garraia.log")).unwrap_or_default()
}

fn pid_do_daemon(dir: &Path) -> Option<i32> {
    std::fs::read_to_string(dir.join("garraia.pid"))
        .ok()
        .and_then(|s| s.trim().parse().ok())
}

fn vivo(pid: i32) -> bool {
    // SAFETY: sinal 0 so testa se o processo existe.
    unsafe { libc::kill(pid, 0) == 0 }
}

/// So mata o que ainda e este binario: um PID que morreu pode ter sido
/// reciclado por outro processo. Sem `/proc` (macOS), confia no registro.
fn ainda_e_o_daemon(pid: i32) -> bool {
    match std::fs::read(format!("/proc/{pid}/cmdline")) {
        Ok(cmdline) => cmdline.starts_with(garra_bin().as_bytes()),
        Err(_) => !Path::new("/proc/self").exists() && vivo(pid),
    }
}

/// Derruba os daemons deste teste se ele sair no meio, para nao deixar um
/// gateway orfao escutando na maquina de quem roda a suite.
///
/// Guarda todo PID que o teste viu, nao so o do PID file: um `stop` que
/// apagou o arquivo e nao matou o processo deixaria o guard cego. E o daemon
/// NAO lidera o proprio grupo — o `setsid` roda no filho do primeiro fork, o
/// daemon e o neto —, entao o grupo vem do `getpgid` e o PID leva o sinal
/// direto tambem.
struct DaemonGuard<'a> {
    dir: &'a Path,
    vistos: RefCell<Vec<i32>>,
}

impl DaemonGuard<'_> {
    fn registra(&self, pid: i32) {
        self.vistos.borrow_mut().push(pid);
    }
}

impl Drop for DaemonGuard<'_> {
    fn drop(&mut self) {
        let mut pids = self.vistos.borrow().clone();
        pids.extend(pid_do_daemon(self.dir));
        for pid in pids {
            if !vivo(pid) || !ainda_e_o_daemon(pid) {
                continue;
            }
            // SAFETY: kill/getpgid sobre um PID que este teste subiu e que
            // ainda roda o binario do teste; o grupo do proprio processo de
            // teste nunca e alvo.
            unsafe {
                let grupo = libc::getpgid(pid);
                if grupo > 0 && grupo != libc::getpgid(0) {
                    libc::kill(-grupo, libc::SIGKILL);
                }
                libc::kill(pid, libc::SIGKILL);
            }
        }
    }
}

/// Sobe o daemon, espera ele escutar na porta e o derruba com `garra stop`.
/// Devolve o PID dessa execucao.
///
/// A espera e pelo PID novo no `garraia.pid` e pela porta, nunca pelo log: o
/// log e justamente o que esta sob teste. O PID file e escrito antes do
/// `daemon started` e a porta so abre depois dele, entao com a porta aberta a
/// linha desta execucao ja esta no arquivo.
fn uma_execucao(guard: &DaemonGuard<'_>, porta: u16, anterior: Option<i32>) -> i32 {
    let dir = guard.dir;
    let p = porta.to_string();
    let out = comando(dir, &["start", "-d", "--port", &p])
        .output()
        .expect("start -d");
    assert!(
        out.status.success(),
        "start -d falhou: {:?}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );

    let inicio = Instant::now();
    let pid = loop {
        if let Some(pid) = pid_do_daemon(dir).filter(|p| Some(*p) != anterior) {
            guard.registra(pid);
            if TcpStream::connect(("127.0.0.1", porta)).is_ok() {
                break pid;
            }
        }
        assert!(
            inicio.elapsed() < Duration::from_secs(60),
            "o daemon nao subiu em 60s.\nlog:\n{}",
            log(dir)
        );
        std::thread::sleep(Duration::from_millis(100));
    };

    // `stop` cai para "quem escuta na porta" quando nao acha o PID file, e a
    // porta dele vem de `PORT` (default 3888, a do gateway real de quem roda
    // a suite). Com `PORT` na porta do teste, o fallback so alcanca o daemon
    // deste teste.
    let out = comando(dir, &["stop"])
        .env("PORT", &p)
        .output()
        .expect("stop");
    assert!(
        out.status.success(),
        "stop falhou: {:?}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    let inicio = Instant::now();
    while vivo(pid) {
        assert!(
            inicio.elapsed() < Duration::from_secs(20),
            "o daemon {pid} nao morreu depois do stop"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    pid
}

#[test]
fn dois_starts_do_daemon_mantem_as_duas_execucoes_no_log() {
    let dir = tempdir().expect("tempdir");
    // Sem servidor MCP: o daemon nao dispara `npx` (rede, cache) e o teste
    // fica hermetico. O `provision_filesystem_if_missing` respeita o arquivo.
    std::fs::write(dir.path().join("mcp.json"), r#"{"mcpServers":{}}"#).expect("mcp.json");
    // Declarado depois do `dir`, cai antes dele: le o PID file antes de o
    // tempdir sumir.
    let guard = DaemonGuard {
        dir: dir.path(),
        vistos: RefCell::new(Vec::new()),
    };
    let porta = porta_livre();

    let primeiro = uma_execucao(&guard, porta, None);
    uma_execucao(&guard, porta, Some(primeiro));

    let conteudo = log(dir.path());
    assert_eq!(
        conteudo.matches("daemon started").count(),
        2,
        "o segundo `start -d` apagou o log do primeiro:\n{conteudo}"
    );
    // A primeira linha e a da primeira execucao, inteira: nada escreveu no
    // offset 0 por cima dela.
    let primeira = conteudo.lines().next().unwrap_or_default();
    assert!(
        primeira.contains("daemon started"),
        "a primeira linha do log tem de ser a da primeira execucao, inteira: {primeira:?}"
    );
}
