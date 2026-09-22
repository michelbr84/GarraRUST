//! #1261 (decisao A) contra o binario de verdade: `start`, `start -d` e
//! `restart` recusam um bind nao-loopback sem credencial de gateway, com a
//! mensagem que diz como corrigir, ANTES de escutar ou de fazer fork.
//!
//! A config em disco diz `127.0.0.1` de proposito: a decisao tem de olhar o
//! bind resolvido pelo clap (`HOST` ou `--host`), nunca o arquivo. Voltar a
//! decidir por `config.gateway.host` do arquivo deixa estes testes vermelhos.

use std::net::TcpListener;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

use tempfile::tempdir;

fn garra_bin() -> &'static str {
    env!("CARGO_BIN_EXE_garra")
}

fn porta_livre() -> u16 {
    TcpListener::bind("0.0.0.0:0")
        .expect("porta efemera")
        .local_addr()
        .expect("local_addr")
        .port()
}

/// Um comando com o ambiente isolado num tempdir e sem nenhuma credencial
/// de gateway herdada de quem roda o teste.
fn comando(dir: &std::path::Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(garra_bin());
    cmd.args(args)
        .env("XDG_CONFIG_HOME", dir)
        .env("GARRAIA_CONFIG_DIR", dir)
        .env("HOME", dir)
        .env("GARRAIA_NO_SPINNER", "1")
        .env_remove("GARRAIA_GATEWAY_API_KEY")
        .env_remove("HOST")
        .env_remove("PORT")
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

/// Roda com um teto: se o gateway SUBIR (o bug), o teste nao pode pendurar.
fn roda_com_teto(mut cmd: Command) -> Output {
    let mut filho = cmd.spawn().expect("spawn garra");
    let inicio = std::time::Instant::now();
    loop {
        if filho.try_wait().expect("try_wait").is_some() {
            return filho.wait_with_output().expect("output");
        }
        if inicio.elapsed() > Duration::from_secs(20) {
            let _ = filho.kill();
            let out = filho.wait_with_output().expect("output");
            panic!(
                "o processo nao saiu — o gateway subiu em vez de recusar.\nstderr:\n{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn config_em_loopback_sem_chave(dir: &std::path::Path) {
    std::fs::write(
        dir.join("config.yml"),
        "gateway:\n  host: \"127.0.0.1\"\n  port: 3888\n",
    )
    .expect("config");
}

fn assert_recusou(out: &Output, porta: u16, contexto: &str) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(78),
        "{contexto}: exit EX_CONFIG esperado\nstderr:\n{stderr}"
    );
    for trecho in [
        "refusing to start",
        " init`",
        "gateway.api_key",
        "GARRAIA_GATEWAY_API_KEY",
        "--host 127.0.0.1",
    ] {
        assert!(
            stderr.contains(trecho),
            "{contexto}: falta `{trecho}` no stderr:\n{stderr}"
        );
    }
    TcpListener::bind(("0.0.0.0", porta))
        .unwrap_or_else(|e| panic!("{contexto}: ninguem podia ter escutado na porta {porta}: {e}"));
}

#[test]
fn start_com_host_da_env_recusa() {
    let dir = tempdir().expect("tempdir");
    config_em_loopback_sem_chave(dir.path());
    let porta = porta_livre();
    let mut cmd = comando(dir.path(), &["start"]);
    cmd.env("HOST", "0.0.0.0").env("PORT", porta.to_string());
    let out = roda_com_teto(cmd);
    assert_recusou(&out, porta, "HOST=0.0.0.0 garra start");
}

#[test]
fn start_com_flag_host_recusa() {
    let dir = tempdir().expect("tempdir");
    config_em_loopback_sem_chave(dir.path());
    let porta = porta_livre();
    let p = porta.to_string();
    let cmd = comando(dir.path(), &["start", "--host", "0.0.0.0", "--port", &p]);
    let out = roda_com_teto(cmd);
    assert_recusou(&out, porta, "garra start --host 0.0.0.0");
}

/// `start -d`: a recusa sai no stderr do PAI, antes do fork, e nenhum PID
/// file fica para tras.
#[test]
fn start_daemon_recusa_antes_do_fork() {
    let dir = tempdir().expect("tempdir");
    config_em_loopback_sem_chave(dir.path());
    let porta = porta_livre();
    let p = porta.to_string();
    let cmd = comando(
        dir.path(),
        &["start", "-d", "--host", "0.0.0.0", "--port", &p],
    );
    let out = roda_com_teto(cmd);
    assert_recusou(&out, porta, "garra start -d --host 0.0.0.0");
    assert!(
        !dir.path().join("garraia.pid").exists(),
        "a recusa vem antes do fork: nenhum PID file"
    );
}

/// `restart` le `HOST` como o `start` (paridade) — e recusa igual.
#[test]
fn restart_le_host_da_env_e_recusa() {
    let dir = tempdir().expect("tempdir");
    config_em_loopback_sem_chave(dir.path());
    let porta = porta_livre();
    let mut cmd = comando(dir.path(), &["restart"]);
    cmd.env("HOST", "0.0.0.0").env("PORT", porta.to_string());
    let out = roda_com_teto(cmd);
    assert_recusou(&out, porta, "HOST=0.0.0.0 garra restart");
}

/// Uma credencial de verdade nao e recusada: com GARRAIA_GATEWAY_API_KEY o
/// processo passa da checagem (ele entao sobe, e o teste o derruba).
#[test]
fn start_com_credencial_de_env_passa_da_recusa() {
    let dir = tempdir().expect("tempdir");
    config_em_loopback_sem_chave(dir.path());
    let porta = porta_livre();
    let mut cmd = comando(dir.path(), &["start"]);
    cmd.env("HOST", "0.0.0.0")
        .env("PORT", porta.to_string())
        .env("GARRAIA_GATEWAY_API_KEY", "credencial-de-teste-1261-longa");
    let mut filho = cmd.spawn().expect("spawn");
    std::thread::sleep(Duration::from_secs(3));
    let saiu = filho.try_wait().expect("try_wait");
    let _ = filho.kill();
    let out = filho.wait_with_output().expect("output");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("refusing to start"),
        "com credencial nao pode recusar:\n{stderr}"
    );
    if let Some(status) = saiu {
        assert_ne!(status.code(), Some(78), "exit 78 e a recusa:\n{stderr}");
    }
    assert!(
        !stderr.contains("credencial-de-teste-1261-longa")
            && !String::from_utf8_lossy(&out.stdout).contains("credencial-de-teste-1261-longa"),
        "o segredo nunca aparece na saida"
    );
}
