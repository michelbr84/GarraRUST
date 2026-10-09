//! `garra uninstall` contra o binario de verdade, com instalacao plantada em
//! tempdir. Regra de seguranca do proprio teste: o `garra` de build NUNCA roda
//! contra si mesmo — apagaria o proprio artefato e estaria testando destruicao
//! real da maquina. Cada cenario planta uma copia (`bin/garraia[.exe]` + alias)
//! e roda a copia.
//!
//! O happy-path de `--all-binaries` deliberadamente nao tem e2e aqui: a
//! varredura alcancaria `/usr/local/bin` e `/usr/bin` reais e apagaria a
//! instalacao do contributor. A semantica da varredura e coberta pelos testes
//! de unidade de `update_scan.rs`, e o alias que acompanha o binario alheio
//! pelo `alias_nosso` em `uninstall.rs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use tempfile::tempdir;

/// sysexits `EX_USAGE` — espelho da constante em `uninstall.rs`.
const EX_USAGE: i32 = 64;

fn garra_bin() -> &'static str {
    env!("CARGO_BIN_EXE_garra")
}

/// Planta uma instalacao typica do install.sh numa copia do binario de build:
/// `bin/garraia` + symlink `bin/garra -> garraia` (unix) ou `bin/garraia.exe` +
/// shim `bin/garra.cmd` (windows), mais um wrapper do Termux. Devolve o exe da
/// copia — e ele que os testes rodam, nunca o `garra_bin()`.
fn planta_instalacao(dir: &Path) -> PathBuf {
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).expect("mkdir bin");
    let exe = bin.join(format!("garraia{}", std::env::consts::EXE_SUFFIX));
    fs::copy(garra_bin(), &exe).expect("copia o binario");
    fs::write(bin.join("garra-mcp-server"), b"#!/bin/sh\nexit 0\n").expect("wrapper");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).expect("chmod exe");
        fs::set_permissions(
            bin.join("garra-mcp-server"),
            fs::Permissions::from_mode(0o755),
        )
        .expect("chmod wrapper");
        std::os::unix::fs::symlink(&exe, bin.join("garra")).expect("symlink garra");
    }
    #[cfg(windows)]
    fs::write(bin.join("garra.cmd"), "@echo off\r\ngarraia %*\r\n").expect("shim garra.cmd");
    exe
}

/// Comando isolado num tempdir, rodando uma copia plantada. CWD = tempdir e
/// `GARRAIA_CONFIG_DIR` = `<dir>/cfg`: a config fica separada do binario para
/// o `--purge` ter o que remover e para o teste ver que o default NAO mexe
/// nela.
fn comando(dir: &Path, exe: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .env("XDG_CONFIG_HOME", dir)
        .env("GARRAIA_CONFIG_DIR", dir.join("cfg"))
        .env("HOME", dir)
        .env("GARRAIA_NO_SPINNER", "1")
        .env_remove("HOST")
        .env_remove("PORT")
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

/// Roda com teto: desinstalacao travada (prompt, lock) nao pode segurar a
/// suite inteira.
fn roda_com_teto(mut cmd: Command) -> Output {
    let mut filho = cmd.spawn().expect("spawn garra");
    let inicio = std::time::Instant::now();
    loop {
        if filho.try_wait().expect("try_wait").is_some() {
            return filho.wait_with_output().expect("output");
        }
        if inicio.elapsed() > Duration::from_secs(60) {
            let _ = filho.kill();
            let out = filho.wait_with_output().expect("output");
            panic!(
                "o processo nao saiu em 60s.\nstderr:\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn existe(p: &Path) -> bool {
    fs::symlink_metadata(p).is_ok()
}

#[test]
fn uninstall_yes_remove_binario_alias_e_wrapper_preserva_config() {
    let dir = tempdir().expect("tempdir");
    let exe = planta_instalacao(dir.path());
    let cfg = dir.path().join("cfg");
    fs::create_dir_all(&cfg).expect("mkdir cfg");

    let out = roda_com_teto(comando(dir.path(), &exe, &["uninstall", "--yes"]));

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "uninstall --yes deve sair 0\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(!existe(&exe), "o binario deve sair: {stdout}");
    #[cfg(unix)]
    assert!(
        !existe(&dir.path().join("bin/garra")),
        "o symlink alias deve sair: {stdout}"
    );
    #[cfg(windows)]
    assert!(
        !existe(&dir.path().join("bin/garra.cmd")),
        "o shim deve sair: {stdout}"
    );
    assert!(
        !existe(&dir.path().join("bin/garra-mcp-server")),
        "o wrapper do Termux deve sair: {stdout}"
    );
    assert!(existe(&cfg), "sem --purge a config intocada fica: {stdout}");
}

#[test]
fn uninstall_purge_remove_config_dir() {
    let dir = tempdir().expect("tempdir");
    let exe = planta_instalacao(dir.path());
    let cfg = dir.path().join("cfg");
    fs::create_dir_all(&cfg).expect("mkdir cfg");
    fs::write(cfg.join("config.yml"), "gateway:\n  port: 3888\n").expect("config plantada");

    let out = roda_com_teto(comando(
        dir.path(),
        &exe,
        &["uninstall", "--yes", "--purge"],
    ));

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "uninstall --purge deve sair 0\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(!existe(&cfg), "--purge remove o config dir: {stdout}");
    assert!(!existe(&exe), "o binario tambem sai no purge: {stdout}");
}

#[test]
fn uninstall_config_quebrada_nao_bloqueia_o_purge() {
    // O contrato do intercept cedo: desinstalar porque a config quebrou e um
    // cenario real. Config ilegivel vira aviso, nunca exit != 0.
    let dir = tempdir().expect("tempdir");
    let exe = planta_instalacao(dir.path());
    let cfg = dir.path().join("cfg");
    fs::create_dir_all(&cfg).expect("mkdir cfg");
    fs::write(cfg.join("config.yml"), "::: nao e yaml :::\n").expect("config quebrada");

    let out = roda_com_teto(comando(
        dir.path(),
        &exe,
        &["uninstall", "--yes", "--purge"],
    ));

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "config quebrada nao pode impedir a desinstalacao\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(!existe(&cfg), "o purge cobre o diretorio padrao: {stdout}");
    assert!(
        !existe(&exe),
        "o binario sai mesmo com config quebrada: {stdout}"
    );
}

#[test]
fn uninstall_sem_yes_em_pipe_recusa_com_ex_usage() {
    let dir = tempdir().expect("tempdir");
    let exe = planta_instalacao(dir.path());

    // stdin null: nao e terminal, entao a recusa e obrigatoria — o pipe nao
    // pode confirmar uma remocao destrutiva por silencio.
    let out = roda_com_teto(comando(dir.path(), &exe, &["uninstall"]));

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(EX_USAGE),
        "sem terminal e sem --yes a desinstalacao e recusada\nstderr:\n{stderr}"
    );
    assert!(
        stderr.contains("--yes"),
        "a recusa aponta o caminho (--yes): {stderr}"
    );
    assert!(existe(&exe), "nada foi removido apos a recusa");
}

#[test]
fn uninstall_segunda_rodada_e_idempotente() {
    let dir = tempdir().expect("tempdir");
    let exe = planta_instalacao(dir.path());
    let cfg = dir.path().join("cfg");
    fs::create_dir_all(&cfg).expect("mkdir cfg");

    let primeira = roda_com_teto(comando(
        dir.path(),
        &exe,
        &["uninstall", "--yes", "--purge"],
    ));
    assert_eq!(
        primeira.status.code(),
        Some(0),
        "primeira rodada deve sair 0"
    );

    // Replanta a mesma instalacao e roda de novo: repetir o comando nao
    // acumula estado nem falha em item ja ausente.
    let exe = planta_instalacao(dir.path());
    let segunda = roda_com_teto(comando(
        dir.path(),
        &exe,
        &["uninstall", "--yes", "--purge"],
    ));

    let stdout = String::from_utf8_lossy(&segunda.stdout);
    let stderr = String::from_utf8_lossy(&segunda.stderr);
    assert_eq!(
        segunda.status.code(),
        Some(0),
        "segunda rodada deve sair 0\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !existe(&exe),
        "segunda rodada tambem remove o binario: {stdout}"
    );
}

#[test]
#[cfg(unix)]
fn uninstall_preserva_arquivo_real_chamado_garra() {
    // Um arquivo comum chamado `garra` ao lado nao e criacao do instalador
    // (que so faz symlink) e pode ser algo do usuario: o default mantem e
    // nota, em vez de destruir por homonimia.
    let dir = tempdir().expect("tempdir");
    let exe = planta_instalacao(dir.path());
    let bin = dir.path().join("bin");
    fs::remove_file(bin.join("garra")).expect("remove o symlink plantado");
    fs::write(bin.join("garra"), b"#!/bin/sh\necho outra coisa\n").expect("arquivo real");

    let out = roda_com_teto(comando(dir.path(), &exe, &["uninstall", "--yes"]));

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "manter arquivo alheio nao e falha\nstdout:\n{stdout}"
    );
    assert!(!existe(&exe), "o binario proprio sai: {stdout}");
    assert!(
        existe(&bin.join("garra")),
        "arquivo real homonimo e mantido: {stdout}"
    );
}

#[test]
#[cfg(unix)]
fn uninstall_remove_backup_old_do_update() {
    // O update deixa `garraia.old` ao lado do binario; e lixo de instalacao e
    // sai junto no escopo padrao.
    let dir = tempdir().expect("tempdir");
    let exe = planta_instalacao(dir.path());
    let backup = exe.with_extension("old");
    fs::write(&backup, b"binario antigo").expect("planta backup .old");

    let out = roda_com_teto(comando(dir.path(), &exe, &["uninstall", "--yes"]));

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "backup nao pode falhar a desinstalacao\nstdout:\n{stdout}"
    );
    assert!(
        !existe(&backup),
        "o backup .old sai no escopo padrao: {stdout}"
    );
    assert!(!existe(&exe), "e o binario tambem: {stdout}");
}

#[test]
#[cfg(windows)]
fn uninstall_no_windows_renomeia_exe_em_uso_sem_falhar() {
    // No Windows o exe em execucao nao pode ser apagado de uma vez: o rename
    // para `.old` (truque do update.rs) funciona com a imagem travada. O
    // residuo e inerte — aviso, nao erro.
    let dir = tempdir().expect("tempdir");
    let exe = planta_instalacao(dir.path());

    let out = roda_com_teto(comando(dir.path(), &exe, &["uninstall", "--yes"]));

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "residuo inerte nao e erro no Windows\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        !existe(&exe),
        "o exe some do lugar original (renomeado): {stdout}"
    );
}
