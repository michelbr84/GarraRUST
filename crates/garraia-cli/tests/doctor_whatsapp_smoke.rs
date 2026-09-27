//! Smoke de `garra doctor whatsapp` contra o binario de verdade (#1419).
//!
//! Molde: `tests/whatsapp_smoke.rs` — o contrato de superficie (o que sai na
//! tela, o exit code, a forma do `--json`) numa instalacao limpa. Arquivo
//! proprio para nao empurrar o `whatsapp_smoke.rs` acima do teto do ratchet.

use std::process::{Command, Stdio};

use tempfile::tempdir;

fn garra_bin() -> &'static str {
    env!("CARGO_BIN_EXE_garra")
}

/// Comando com o ambiente apontado para um diretorio limpo.
fn garra(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
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

/// Numa instalacao limpa — nada vinculado — e vermelho (69), diz por que,
/// aponta o passo, e o `--json` fala o vocabulario do `/api/diagnostics`.
/// Cada area do caminho tem a sua linha.
#[test]
fn in_a_clean_dir_it_exits_69_with_the_link_step_and_speaks_json() {
    let dir = tempdir().expect("tempdir");
    let out = garra(dir.path(), &["doctor", "whatsapp", "--json"]);
    assert_eq!(
        out.status.code(),
        Some(69),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let v: serde_json::Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("json ({e}): {stdout}"));
    assert_eq!(v["ok"], false);
    assert_eq!(v["exit_code"], 69);
    assert_eq!(v["report"]["status"], "error");
    let checks = v["report"]["checks"].as_array().expect("checks");
    let linked = checks
        .iter()
        .find(|c| c["id"] == "whatsapp.linked")
        .expect("whatsapp.linked");
    assert_eq!(linked["status"], "error", "{linked}");
    assert!(
        linked["next_step"]
            .as_str()
            .unwrap_or("")
            .contains("whatsapp"),
        "{linked}"
    );
    for id in [
        "whatsapp.gateway",
        "whatsapp.access",
        "execution.profile",
        "files.workspace",
        "mcp.visibility",
        "provider.default",
    ] {
        assert!(checks.iter().any(|c| c["id"] == id), "sem {id}: {stdout}");
    }
}

/// Humano: o mesmo exit e o resumo no fim; e as flags globais valem depois do
/// subcomando.
#[test]
fn the_human_output_ends_with_the_summary_and_global_flags_work_after_the_area() {
    let dir = tempdir().expect("tempdir");
    let out = garra(dir.path(), &["doctor", "whatsapp"]);
    assert_eq!(out.status.code(), Some(69));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("doctor whatsapp"), "{stdout}");
    assert!(stdout.contains("exit 69"), "{stdout}");

    let out = garra(dir.path(), &["doctor", "--json", "whatsapp"]);
    assert_eq!(out.status.code(), Some(69));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.trim_start().starts_with('{'), "{stdout}");
}
