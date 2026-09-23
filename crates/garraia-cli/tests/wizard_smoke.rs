//! Plan 0126 §M1.8 — smoke test for `garraia init`.
//!
//! Drives the real `garra` binary in a tempdir to verify the
//! non-interactive guard still prints the expected hint and exits 0
//! when stdin is a closed pipe. This is the path CI / `curl … | sh` /
//! `docker build` hit.
//!
//! Full interactive-flow coverage (env detection, GPU prompts, local
//! stack install) is provided by the per-module unit tests in
//! `crates/garraia-cli/src/wizard/{env_detect,local_stack,config_writer}.rs`.

use std::process::{Command, Stdio};

use tempfile::tempdir;

/// `env!("CARGO_BIN_EXE_<name>")` is set by Cargo when running integration
/// tests so they don't have to know the build profile / target path.
fn garra_bin() -> &'static str {
    env!("CARGO_BIN_EXE_garra")
}

/// `garraia init` with stdin closed prints the non-interactive hint and
/// exits 0. This is the contract `install.sh` (PR-B) and any CI path
/// relies on.
#[test]
fn init_in_non_interactive_environment_prints_hint_and_exits_zero() {
    let dir = tempdir().expect("create tempdir");

    let output = Command::new(garra_bin())
        .arg("init")
        // GARRAIA_BOOTSTRAP_LOCAL=0 is a no-op when stdin is closed
        // (we never reach the GPU branch) but documenting it here so
        // a future reader knows the wizard's local-stack gate exists.
        .env("GARRAIA_BOOTSTRAP_LOCAL", "0")
        // Point the config loader at a fresh dir so the test doesn't
        // collide with the developer's real `~/.config/garraia/`.
        .env("XDG_CONFIG_HOME", dir.path())
        .env("GARRAIA_CONFIG_DIR", dir.path())
        // Inherit nothing; stdin is closed → IsTerminal returns false →
        // the non-interactive guard fires.
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn garra init");

    assert!(
        output.status.success(),
        "exit code: {:?}\n--- stdout ---\n{}\n--- stderr ---\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Non-interactive environment detected"),
        "expected non-interactive hint, got:\n{stdout}"
    );
    assert!(
        stdout.contains("config.yml"),
        "expected config.yml reference, got:\n{stdout}"
    );
}

/// #1430 — o passo de canal do wizard passou a oferecer WhatsApp junto com o
/// Telegram, e o vinculo do WhatsApp roda no fim do `init`. Numa instalacao
/// sem terminal (`docker build`, CI, `curl … | sh` sem TTY) nada disso pode
/// acontecer: o guarda de nao-interativo fica ANTES do primeiro prompt, e a
/// entrega ao WhatsApp e condicional a uma escolha que ninguem fez.
///
/// A unidade ja fixa a ordem no fonte (`run_wizard_entrega_o_whatsapp_depois_
/// de_salvar_a_config`); este teste prova a mesma coisa pelo binario de
/// verdade, que e onde um `docker build` quebraria.
#[test]
fn init_sem_terminal_nao_oferece_nem_vincula_canal() {
    let dir = tempdir().expect("create tempdir");

    let output = Command::new(garra_bin())
        .arg("init")
        .env("GARRAIA_BOOTSTRAP_LOCAL", "0")
        .env("XDG_CONFIG_HOME", dir.path())
        .env("GARRAIA_CONFIG_DIR", dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn garra init");

    assert!(
        output.status.success(),
        "exit code: {:?}",
        output.status.code()
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    for proibido in [
        "Channel Setup",
        "canal de mensagens",
        "WhatsApp",
        "@BotFather",
    ] {
        assert!(
            !stdout.contains(proibido),
            "instalacao sem terminal nao pode chegar ao passo de canal \
             (achei {proibido:?}):\n{stdout}"
        );
    }

    // O guarda retorna antes do `write_config`, entao nada foi gravado — e,
    // em particular, nenhuma secao de canal nasceu ligada por acidente.
    assert!(
        !dir.path().join("config.yml").exists(),
        "o guarda de nao-interativo nao pode escrever config"
    );
}
