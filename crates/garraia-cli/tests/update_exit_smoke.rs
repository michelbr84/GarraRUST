//! `garra update` e `garra rollback` nao podem sair com exit 0 quando a
//! operacao falha. O bug: os dois ramos de erro imprimiam `update failed:` /
//! `rollback failed:` no STDOUT e deixavam o processo cair no `Ok(())` do fim
//! do `main`, devolvendo 0 — scripts, CI e automacao chamadora viam sucesso.
//!
//! Contra o binario de verdade (`CARGO_BIN_EXE_garra`), com ambiente isolado
//! em tempdir. As falhas sao deterministicas e offline:
//!
//! - `rollback` sem backup `.old` ao lado do binario ja da `bail!` no proprio
//!   `run_rollback`, sem rede;
//! - `update` com `ALL_PROXY`/`HTTPS_PROXY` apontando para uma porta morta
//!   local faz o `fetch_latest_release` falhar na conexao com o proxy
//!   (reqwest lê as envs de proxy por default), tambem sem depender da rede
//!   externa nem de DNS.

use std::fs;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use tempfile::tempdir;

/// sysexits `EX_SOFTWARE` — espelho de `EX_UPDATE_FAILED` em `main.rs`. Se a
/// constante la mudar, este teste tem de mudar junto (e e bom que reclame).
const EX_UPDATE_FAILED: i32 = 70;

fn garra_bin() -> &'static str {
    env!("CARGO_BIN_EXE_garra")
}

/// Um comando com o ambiente isolado num tempdir — tambem impecavel contra o
/// dotenv walk-up: o CWD e o tempdir, nao o repo.
fn comando(dir: &std::path::Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(garra_bin());
    cmd.args(args)
        .env("XDG_CONFIG_HOME", dir)
        .env("GARRAIA_CONFIG_DIR", dir)
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

/// Roda com teto: se o processo pendurar (rede, dialog), o teste nao pode
/// segurar a suite inteira.
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

#[test]
fn rollback_sem_backup_sai_com_ex_update_failed() {
    // Um `.old` deixado por uma atualizacao local anterior nao pode tornar o
    // teste destrutivo (rename por cima do binario de teste) nem espurio: o
    // teste garante o cenario "sem backup".
    let atual = fs::canonicalize(garra_bin()).expect("canonicaliza o proprio garra");
    let backup = atual.with_extension("old");
    let _ = fs::remove_file(&backup);

    let dir = tempdir().expect("tempdir");
    let out = roda_com_teto(comando(dir.path(), &["rollback"]));

    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(EX_UPDATE_FAILED),
        "rollback falhou mas devolveu {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status.code()
    );
    assert!(
        stderr.contains("rollback failed"),
        "a mensagem de erro pertence ao stderr, nao ao stdout: {stderr}"
    );
    assert!(
        stderr.contains("no backup found"),
        "a causa raiz deve aparecer na cadeia de erro: {stderr}"
    );
    assert!(
        !stdout.contains("rollback failed"),
        "nada de mensagem de falha no stdout: {stdout}"
    );
}

#[test]
fn update_sem_rede_sai_com_ex_update_failed() {
    let dir = tempdir().expect("tempdir");
    let mut cmd = comando(dir.path(), &["update", "--yes"]);
    // Porta morta local: a conexao com o proxy recusa na hora, sem rede
    // externa e sem DNS (reqwest fala com o proxy, nao com o host final).
    for var in ["ALL_PROXY", "HTTPS_PROXY", "HTTP_PROXY"] {
        cmd.env(var, "http://127.0.0.1:1");
    }
    let out = roda_com_teto(cmd);

    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(EX_UPDATE_FAILED),
        "update falhou mas devolveu {:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status.code()
    );
    assert!(
        stderr.contains("update failed"),
        "a mensagem de erro pertence ao stderr, nao ao stdout: {stderr}"
    );
    assert!(
        !stdout.contains("update failed"),
        "nada de mensagem de falha no stdout: {stdout}"
    );
}

#[test]
fn update_check_binaries_sem_rede_continua_exit_0() {
    // O caminho de sucesso nao pode ser atingido pelo conserto: varredura de
    // PATH nao usa rede nem exit code de falha.
    let dir = tempdir().expect("tempdir");
    let out = roda_com_teto(comando(dir.path(), &["update", "--check-binaries"]));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "check-binaries deve continuar exit 0\nstdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !stdout.contains("update failed"),
        "check-binaries nao falhou: {stdout}"
    );
}
