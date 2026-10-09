//! O `.env` e lido APENAS do diretorio atual — nunca de um pai.
//!
//! O bug: `dotenvy::dotenv()` faz walk-up pelos diretorios pais ate achar um
//! `.env`. Um arquivo na raiz do repo (ou em qualquer ascendencia) injetava
//! `GARRAIA_CONFIG_DIR`, `GARRAIA_EXECUTION_PROFILE` e o que mais estivesse
//! nele em todo comando `garra` rodado de dentro de uma subpasta, sem o
//! operador pedir: envenenamento silencioso de ambiente por ascendencia.
//!
//! Contra o binario de verdade. O observavel e o contrato de superficie do
//! perfil de execucao invalido (ADR 0024): `GARRAIA_EXECUTION_PROFILE` com
//! valor invalido vira `Error` em `execution.profile` com exit 2 no
//! `config check --json` — o mesmo molde de `config_check_smoke.rs`. Se o
//! walk-up voltar, o `.env` do PAI passa a valer e o exit 2 denuncia.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use tempfile::tempdir;

fn garra_bin() -> &'static str {
    env!("CARGO_BIN_EXE_garra")
}

/// `garra config check --json` de dentro de `dir`, com ambiente limpo (sem
/// perfil herdado de quem roda o teste) e sem `.env` algum no proprio `dir`.
fn config_check(dir: &Path) -> std::process::Output {
    let mut cmd = Command::new(garra_bin());
    cmd.args(["config", "check", "--json"])
        .env("XDG_CONFIG_HOME", dir)
        .env("GARRAIA_CONFIG_DIR", dir)
        .env("HOME", dir)
        .env("GARRAIA_LANG", "pt_BR.UTF-8")
        .env_remove("GARRAIA_EXECUTION_PROFILE")
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.output().expect("spawn garra")
}

fn erros_em_execution_profile(out: &std::process::Output) -> Vec<String> {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&stdout) else {
        return vec![format!("stdout nao e JSON: {stdout}")];
    };
    payload["findings"]
        .as_array()
        .map(|fs| {
            fs.iter()
                .filter(|f| f["field"] == "execution.profile" && f["severity"] == "error")
                .filter_map(|f| f["message"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Raiz com `.env` envenenado (perfil invalido) e um subdir de trabalho limpo
/// por onde o comando roda — o cenario do walk-up.
fn raiz_com_env_no_pai() -> tempfile::TempDir {
    let raiz = tempdir().expect("tempdir");
    fs::write(raiz.path().join(".env"), "GARRAIA_EXECUTION_PROFILE=pod\n").expect("write .env");
    fs::create_dir(raiz.path().join("trabalho")).expect("mkdir trabalho");
    raiz
}

fn config_limpo(dir: &Path) {
    fs::write(dir.join("config.yml"), "gateway:\n  port: 3888\n").expect("config");
}

/// O `.env` do PAI nao pode valer: o comando roda de `raiz/trabalho`, o
/// `.env` esta em `raiz/`, e o perfil padrao (standard) e limpo — exit 0,
/// nenhum `Error` em `execution.profile`.
#[test]
fn env_do_pai_nao_e_carregado() {
    let raiz = raiz_com_env_no_pai();
    let trabalho = raiz.path().join("trabalho");
    config_limpo(&trabalho);

    let out = config_check(&trabalho);

    let erros = erros_em_execution_profile(&out);
    assert_eq!(
        out.status.code(),
        Some(0),
        "o .env do pai nao pode virar exit 2\nstderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        erros.is_empty(),
        "o .env do pai (walk-up) foi carregado: {erros:?}"
    );
}

/// Controle positivo: o `.env` do PROPRIO diretorio atual continua valendo —
/// o carregamento restrito ao CWD nao e o mesmo que nao carregar nada.
#[test]
fn env_do_diretorio_atual_continua_valendo() {
    let raiz = raiz_com_env_no_pai();
    let trabalho = raiz.path().join("trabalho");
    config_limpo(&trabalho);
    // Mesmo valor do pai: se o comando achar exit 2 com o Error de perfil,
    // foi do proprio CWD, nao do walk-up.
    fs::write(trabalho.join(".env"), "GARRAIA_EXECUTION_PROFILE=pod\n").expect("write .env");

    let out = config_check(&trabalho);

    assert_eq!(
        out.status.code(),
        Some(2),
        "o .env do CWD deve continuar valendo\nstderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !erros_em_execution_profile(&out).is_empty(),
        "esperado Error em execution.profile vindo do .env do CWD"
    );
}
