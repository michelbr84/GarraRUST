//! Smoke de `garraia doctor mcp` contra o binario de verdade (#1595).
//!
//! Molde: `tests/doctor_whatsapp_smoke.rs` — o contrato de superficie (o que
//! sai na tela, o exit code, a forma do `--json`) numa instalacao limpa, e o
//! health-check de subida com o fixture stdio que os testes de MCP do
//! `garraia-agents` ja usam. Arquivo proprio para nao empurrar o smoke do
//! whatsapp acima do teto do ratchet.

use std::path::Path;
use std::process::{Command, Stdio};

use tempfile::tempdir;

fn garra_bin() -> &'static str {
    env!("CARGO_BIN_EXE_garra")
}

/// Comando com o ambiente apontado para um diretorio limpo.
fn garra(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(garra_bin())
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

/// O fixture stdio que o `garraia-agents` ja usa nos testes de lifecycle MCP
/// (`tests/mcp_lifecycle.rs`) — fala so o suficiente de JSON-RPC para
/// `initialize`, `tools/list` e `tools/call`.
fn fixture_args() -> Vec<String> {
    vec![format!(
        "{}/../garraia-agents/tests/fixtures/fake_mcp_server.py",
        env!("CARGO_MANIFEST_DIR")
    )]
}

/// Instalacao limpa: nenhum servidor declarado nao e falha — nao ha o que
/// spawnar, e o exit e 0 com a nota de que o gateway provisiona no boot.
#[test]
fn clean_dir_exits_zero_and_says_nothing_to_check() {
    let dir = tempdir().expect("tempdir");
    let out = garra(dir.path(), &["doctor", "mcp", "--json"]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("json ({e}): {stdout}"));
    assert_eq!(v["ok"], true);
    assert_eq!(v["exit_code"], 0);
    assert_eq!(v["report"]["servers"].as_array().unwrap().len(), 0);
    assert!(
        v["report"]["note"]
            .as_str()
            .unwrap_or("")
            .contains("primeiro boot"),
        "{v}"
    );
}

/// O fixture declarado em `mcp:` do config.yml sobe de verdade: handshake
/// completo, tools contadas, exit 0. Este e o caso da issue — o doctor
/// confirma que o MCP server autoprovisionado NASCEU, nao so que existe.
#[cfg(not(windows))]
#[test]
fn configured_stdio_server_spawns_and_reports_tools() {
    let dir = tempdir().expect("tempdir");
    let fixture = fixture_args();
    let config = format!(
        "mcp:\n  fixture:\n    command: python3\n    args: [\"{}\"]\n    timeout: 15\n",
        fixture[0]
    );
    std::fs::write(dir.path().join("config.yml"), config).expect("write config");

    let out = garra(dir.path(), &["doctor", "mcp", "--json"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("json ({e}): {stdout}"));
    let servers = v["report"]["servers"].as_array().expect("servers");
    let fixture = servers
        .iter()
        .find(|s| s["name"] == "fixture")
        .expect("fixture server in report");
    assert_eq!(fixture["status"], "ok", "{fixture}");
    assert!(
        fixture["tools"].as_u64().unwrap_or(0) >= 1,
        "o fixture anuncia ao menos a tool echo: {fixture}"
    );
}

/// Um servidor declarado que nao subiu reprova o check com exit 2 — a
/// "saida nonzero quando o check falha (util em CI/scripts)" da issue.
#[cfg(not(windows))]
#[test]
fn server_that_cannot_start_fails_the_check_with_exit_two() {
    let dir = tempdir().expect("tempdir");
    let config = "mcp:\n  morto:\n    command: garra-servidor-que-nao-existe-1595\n    args: []\n    timeout: 10\n";
    std::fs::write(dir.path().join("config.yml"), config).expect("write config");

    let out = garra(dir.path(), &["doctor", "mcp", "--json"]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "stdout: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("json ({e}): {stdout}"));
    assert_eq!(v["ok"], false);
    let morto = v["report"]["servers"]
        .as_array()
        .expect("servers")
        .iter()
        .find(|s| s["name"] == "morto")
        .expect("morto in report");
    assert_eq!(morto["status"], "nao_subiu", "{morto}");
    assert!(
        morto["erro"].as_str().unwrap_or("").len() > 5,
        "o erro real tem de aparecer: {morto}"
    );
}

/// O `doctor` RAIZ ganhou o resumo CONFIG do MCP (`[5/5]`) sem spawnar nada:
/// numa instalacao limpa ele diz "nenhum servidor declarado" e segue vermelho
/// so se a config tiver erro proprio.
#[test]
fn root_doctor_prints_the_mcp_summary_line() {
    let dir = tempdir().expect("tempdir");
    let out = garra(dir.path(), &["doctor"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("[5/5] MCP"),
        "a linha de resumo MCP tem de existir no doctor raiz: {stdout}"
    );
    assert!(stdout.contains("nenhum servidor declarado"), "{stdout}");
}

/// `--json` do doctor raiz inclui o objeto `mcp` com a mesma vocabulario.
#[test]
fn root_doctor_json_carries_the_mcp_summary_object() {
    let dir = tempdir().expect("tempdir");
    let out = garra(dir.path(), &["doctor", "--json"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("json ({e}): {stdout}"));
    assert!(
        v["report"]["mcp"].get("servidores").is_some(),
        "o resumo MCP esta no JSON do doctor raiz: {v}"
    );
}
