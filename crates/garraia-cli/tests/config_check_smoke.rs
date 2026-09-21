//! Smoke de `garra config check` contra o binario de verdade (ADR 0024).
//!
//! Molde: `tests/whatsapp_smoke.rs`. O que se prova aqui e o contrato de
//! superficie do perfil de execucao invalido — exit 2 com um `Error` em
//! `execution.profile` — pelos DOIS caminhos, env e arquivo, num
//! subprocesso. E o unico lugar em que um valor invalido entra em
//! `GARRAIA_EXECUTION_PROFILE` de verdade: os testes unitarios de
//! `garraia-config` injetam o valor por parametro, porque `load()` e
//! `validate()` sao lidos sem lock no mesmo binario e um valor invalido em
//! transito derrubaria qualquer um deles (review C7 da #1329).

use std::process::{Command, Stdio};

use tempfile::tempdir;

fn garra_bin() -> &'static str {
    env!("CARGO_BIN_EXE_garra")
}

/// `garra config check --json` com o ambiente apontado para um diretorio
/// limpo e, opcionalmente, a env do perfil.
fn config_check(dir: &std::path::Path, perfil_env: Option<&str>) -> std::process::Output {
    let mut cmd = Command::new(garra_bin());
    cmd.args(["config", "check", "--json"])
        .env("XDG_CONFIG_HOME", dir)
        .env("GARRAIA_CONFIG_DIR", dir)
        .env("HOME", dir)
        .env("GARRAIA_LANG", "pt_BR.UTF-8")
        .env_remove("GARRAIA_EXECUTION_PROFILE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(valor) = perfil_env {
        cmd.env("GARRAIA_EXECUTION_PROFILE", valor);
    }
    cmd.output().expect("spawn garra")
}

fn json_de(out: &std::process::Output) -> serde_json::Value {
    let stdout = String::from_utf8_lossy(&out.stdout);
    serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "stdout nao e JSON ({e}):\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&out.stderr)
        )
    })
}

fn erros_em_execution_profile(payload: &serde_json::Value) -> Vec<String> {
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

/// Env invalida: o relatorio existe (exit 2), com o `Error` nomeando o valor
/// e a env — nao um exit 65 "o arquivo nao parseia".
#[test]
fn env_invalida_e_error_em_execution_profile_com_exit_2() {
    let dir = tempdir().expect("tempdir");
    std::fs::write(dir.path().join("config.yml"), "gateway:\n  port: 3888\n").expect("config");

    let out = config_check(dir.path(), Some("pod"));
    assert_eq!(
        out.status.code(),
        Some(2),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let payload = json_de(&out);
    let erros = erros_em_execution_profile(&payload);
    assert_eq!(erros.len(), 1, "{payload}");
    assert!(erros[0].contains("\"pod\""), "{}", erros[0]);
    assert!(
        erros[0].contains("GARRAIA_EXECUTION_PROFILE"),
        "{}",
        erros[0]
    );
    // O sumario mostra o que o arquivo diz (nada), nao o valor invalido.
    assert_eq!(
        payload["summary"]["execution_profile"], "standard",
        "{payload}"
    );
}

/// Valor invalido NO ARQUIVO (review C10): o serde recusa o arquivo, mas o
/// check carrega o resto e reporta o mesmo `Error` em `execution.profile`,
/// exit 2 — como os docs prometem para "arquivo ou env".
#[test]
fn arquivo_invalido_e_error_em_execution_profile_com_exit_2() {
    let dir = tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("config.yml"),
        "gateway:\n  port: 3888\nexecution:\n  profile: isolated_pod\n",
    )
    .expect("config");

    let out = config_check(dir.path(), None);
    assert_eq!(
        out.status.code(),
        Some(2),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let payload = json_de(&out);
    let erros = erros_em_execution_profile(&payload);
    assert_eq!(erros.len(), 1, "{payload}");
    assert!(erros[0].contains("\"isolated_pod\""), "{}", erros[0]);
    assert!(erros[0].contains("config file"), "{}", erros[0]);
    // O resto do arquivo foi lido: a porta declarada esta no sumario.
    assert_eq!(payload["summary"]["gateway_port"], 3888, "{payload}");
}

/// O gemeo: valor valido pela env e pelo arquivo — sem `Error`, e o sumario
/// diz de onde o perfil veio.
#[test]
fn valor_valido_nao_gera_error_e_o_sumario_diz_a_origem() {
    let dir = tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("config.yml"),
        "execution:\n  profile: isolated-pod\n",
    )
    .expect("config");

    let do_arquivo = json_de(&config_check(dir.path(), None));
    assert!(
        erros_em_execution_profile(&do_arquivo).is_empty(),
        "{do_arquivo}"
    );
    assert_eq!(do_arquivo["summary"]["execution_profile"], "isolated-pod");
    assert_eq!(do_arquivo["summary"]["execution_profile_source"], "file");

    let da_env = json_de(&config_check(dir.path(), Some("standard")));
    assert!(erros_em_execution_profile(&da_env).is_empty(), "{da_env}");
    assert_eq!(da_env["summary"]["execution_profile"], "standard");
    assert_eq!(da_env["summary"]["execution_profile_source"], "env");
}
