//! Smoke de `garra mcp list|inspect` contra o binario de verdade (#1793).
//!
//! Os subcomandos `garra mcp` usavam uma janela de 30s fixa para falar com o
//! servidor (spawn + handshake + `tools/list`), e o `list` mostrava esse 30
//! como se fosse o timeout do servidor. O gateway sempre usou o `timeout` da
//! entrada ou, sem ele, `timeouts.mcp.default_secs` — os dois tinham de dizer
//! a mesma coisa sobre o mesmo config. Molde: `tests/doctor_mcp_smoke.rs`.

use std::path::Path;
use std::process::{Command, Stdio};
// So o smoke de unix (o servidor MCP mudo e o `sleep`) mede tempo.
#[cfg(unix)]
use std::time::{Duration, Instant};

use tempfile::tempdir;

fn garra(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_garra"))
        .args(args)
        .env("XDG_CONFIG_HOME", dir)
        .env("GARRAIA_CONFIG_DIR", dir)
        .env("HOME", dir)
        .env("GARRAIA_LANG", "pt_BR.UTF-8")
        .env_remove("GARRAIA_EXECUTION_PROFILE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn garra")
}

/// `garra mcp list` mostra a janela que o servidor vai ter de fato: a da
/// entrada quando declarada, `timeouts.mcp.default_secs` quando nao.
#[test]
fn mcp_list_mostra_a_janela_efetiva_do_config() {
    let dir = tempdir().expect("tempdir");
    let config = "timeouts:\n  mcp:\n    default_secs: 77\n\
                  mcp:\n  sem_janela:\n    command: garra-nao-roda-1793\n    args: []\n  \
                  com_janela:\n    command: garra-nao-roda-1793\n    args: []\n    timeout: 9\n";
    std::fs::write(dir.path().join("config.yml"), config).expect("write config");

    let out = garra(dir.path(), &["mcp", "list"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "stdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let linha = |nome: &str| {
        stdout
            .lines()
            .find(|l| l.trim_start().starts_with(nome))
            .unwrap_or_else(|| panic!("{nome} fora da lista: {stdout}"))
            .to_string()
    };
    assert!(
        linha("sem_janela").contains("(timeout: 77s)"),
        "sem `timeout` na entrada vale timeouts.mcp.default_secs: {stdout}"
    );
    assert!(
        linha("com_janela").contains("(timeout: 9s)"),
        "o `timeout` da entrada vence: {stdout}"
    );
}

/// `garra mcp inspect` contra um servidor que sobe e nunca responde ao
/// handshake: a janela que mata a conexao e a do config (1s), nao a de 30s
/// que o comando aplicava por conta propria.
#[cfg(unix)]
#[test]
fn mcp_inspect_obedece_timeouts_mcp_do_config() {
    let dir = tempdir().expect("tempdir");
    let config = "timeouts:\n  mcp:\n    default_secs: 1\n\
                  mcp:\n  mudo:\n    command: sleep\n    args: [\"40\"]\n";
    std::fs::write(dir.path().join("config.yml"), config).expect("write config");

    let inicio = Instant::now();
    let out = garra(dir.path(), &["mcp", "inspect", "mudo"]);
    let gasto = inicio.elapsed();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("timed out after 1s"),
        "a janela aplicada tem de ser a de timeouts.mcp (1s): {stdout}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        gasto < Duration::from_secs(25),
        "o handshake mudo tem de cair na janela de 1s: levou {gasto:?}"
    );
}
