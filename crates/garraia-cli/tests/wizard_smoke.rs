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
