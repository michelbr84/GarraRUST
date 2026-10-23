//! Fixture do teste de orfao (#1636): uma arvore pai -> filho -> neto que o
//! teste de integracao `orphan_process` mata pelo pai, de forma abrupta, para
//! afirmar que nenhum descendente sobrevive.
//!
//! So e exercitado no Windows (onde o pai cria um Job Object `KILL_ON_JOB_CLOSE`
//! e assina o filho; a arvore inteira entra no job). No Unix a contencao e por
//! PDEATHSIG/grupo de processos no proprio bridge (ver `garraia-channels`) e
//! pelo watch de stdin do lado do filho, cobertos la e no `node --test` — este
//! fixture nao e usado fora do Windows, mas compila como um stub inerte.
//!
//! Papeis:
//!   parent <child_pidfile> <grand_pidfile>  cria o job, lanca o filho, assina
//!   child  <child_pidfile> <grand_pidfile>  grava o pid, lanca o neto, ocioso
//!   grand  <grand_pidfile>                  grava o pid, ocioso
//!
//! Sem `unwrap`/`expect`: erros viram exit 2. O ocioso e um loop de sleep — o
//! processo so morre quando o SO o mata (pelo job, no Windows).

use std::io;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let Err(e) = run(&args) {
        eprintln!("orphan_fixture: {e}");
        std::process::exit(2);
    }
}

fn run(args: &[String]) -> io::Result<()> {
    match args.get(1).map(String::as_str) {
        Some("parent") => parent(args.get(2), args.get(3)),
        Some("child") => child(args.get(2), args.get(3)),
        Some("grand") => grand(args.get(2)),
        other => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("role desconhecido: {other:?}"),
        )),
    }
}

fn arg<'a>(value: Option<&'a String>, nome: &str) -> io::Result<&'a str> {
    value
        .map(String::as_str)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("falta o arg {nome}")))
}

fn spawn_self(role: &str, pidfiles: &[&str]) -> io::Result<std::process::Child> {
    let exe = std::env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.arg(role);
    for p in pidfiles {
        cmd.arg(p);
    }
    cmd.spawn()
}

fn gravar_pid(pidfile: &str) -> io::Result<()> {
    std::fs::write(Path::new(pidfile), std::process::id().to_string())
}

fn ocioso() -> ! {
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn parent(child_pidfile: Option<&String>, grand_pidfile: Option<&String>) -> io::Result<()> {
    let child_pidfile = arg(child_pidfile, "child_pidfile")?;
    let grand_pidfile = arg(grand_pidfile, "grand_pidfile")?;

    // No Windows, o filho (e o neto, por heranca de job) entra num Job Object
    // com KILL_ON_JOB_CLOSE que ESTE processo segura. Quando o teste mata este
    // processo de forma abrupta, o SO fecha o handle do job e a arvore morre.
    #[cfg(windows)]
    let _job = {
        let job = garraia_desktop_core::KillOnCloseJob::new()?;
        let filho = spawn_self("child", &[child_pidfile, grand_pidfile])?;
        use std::os::windows::io::AsRawHandle;
        job.assign_handle(filho.as_raw_handle() as isize)?;
        // Ja assinado: o filho (e o neto, por heranca de job) seguem vivos
        // independentemente deste handle. O Drop de `std::process::Child` nao
        // mata o processo, entao deixar `filho` cair aqui apenas fecha o handle.
        job
    };
    #[cfg(not(windows))]
    {
        let _ = spawn_self("child", &[child_pidfile, grand_pidfile])?;
    }

    ocioso();
}

fn child(child_pidfile: Option<&String>, grand_pidfile: Option<&String>) -> io::Result<()> {
    let child_pidfile = arg(child_pidfile, "child_pidfile")?;
    let grand_pidfile = arg(grand_pidfile, "grand_pidfile")?;
    gravar_pid(child_pidfile)?;
    // O neto nasce aqui: no Windows ele herda o job do filho automaticamente.
    let _ = spawn_self("grand", &[grand_pidfile])?;
    ocioso();
}

fn grand(grand_pidfile: Option<&String>) -> io::Result<()> {
    let grand_pidfile = arg(grand_pidfile, "grand_pidfile")?;
    gravar_pid(grand_pidfile)?;
    ocioso();
}
