//! Smoke de `garra whatsapp` contra o binario de verdade.
//!
//! Molde: `tests/wizard_smoke.rs`. O que importa aqui e o contrato de
//! superficie — o que aparece na tela e o exit code — nos caminhos que um
//! script, um Dockerfile ou um `curl … | sh` realmente encontram. O ciclo de
//! vida do bridge e testado em `garraia-channels` contra a fixture Python.

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
        // pt-BR e o default do projeto; fixar evita que o locale da maquina de
        // CI troque o idioma e quebre as asserções de texto.
        .env("GARRAIA_LANG", "pt_BR.UTF-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn garra")
}

#[test]
fn whatsapp_without_a_tty_prints_both_options_and_exits_zero() {
    let dir = tempdir().expect("tempdir");
    let out = garra(dir.path(), &["whatsapp"]);

    assert!(
        out.status.success(),
        "exit {:?}\nstdout:\n{}\nstderr:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("WhatsApp — GarraIA"),
        "cabecalho:\n{stdout}"
    );
    // As duas opcoes, cada uma com o comando explicito: quem esta num pipe
    // precisa saber o que rodar num terminal.
    assert!(stdout.contains("garra whatsapp link"), "{stdout}");
    assert!(stdout.contains("garra whatsapp cloud"), "{stdout}");
    assert!(stdout.contains("QR"), "{stdout}");
    assert!(stdout.contains("Business"), "{stdout}");
}

#[test]
fn whatsapp_link_without_a_tty_also_exits_zero() {
    let dir = tempdir().expect("tempdir");
    let out = garra(dir.path(), &["whatsapp", "link"]);
    assert!(
        out.status.success(),
        "`link` num pipe precisa orientar e sair 0, nao travar esperando um QR"
    );
}

#[test]
fn whatsapp_status_without_a_session_exits_69_with_one_hint_line() {
    let dir = tempdir().expect("tempdir");
    let out = garra(dir.path(), &["whatsapp", "status"]);

    assert_eq!(
        out.status.code(),
        Some(69),
        "EX_UNAVAILABLE distingue 'nao vinculado' de 'deu erro'"
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Nenhum WhatsApp pessoal vinculado"),
        "{stdout}"
    );
    assert!(
        stdout.contains("garra whatsapp"),
        "a dica precisa dizer o que rodar:\n{stdout}"
    );
}

#[test]
fn whatsapp_logout_without_a_session_exits_zero() {
    let dir = tempdir().expect("tempdir");
    let out = garra(dir.path(), &["whatsapp", "logout"]);
    assert!(
        out.status.success(),
        "desvincular o que nao existe e no-op, nao erro"
    );
}

#[test]
fn whatsapp_help_lists_the_four_subcommands() {
    let dir = tempdir().expect("tempdir");
    let out = garra(dir.path(), &["whatsapp", "--help"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    for sub in ["link", "cloud", "status", "logout"] {
        assert!(stdout.contains(sub), "faltou `{sub}` no --help:\n{stdout}");
    }
}

/// O `--help` de topo precisa listar o comando novo: e assim que alguem o
/// descobre sem ler a doc.
#[test]
fn top_level_help_mentions_whatsapp() {
    let dir = tempdir().expect("tempdir");
    let out = garra(dir.path(), &["--help"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("whatsapp"), "{stdout}");
}

/// `garra whatsapp` nao acrescentou nenhuma flag com valor, entao o
/// rewriting de argv do `cli_args` nao pode ter mudado de comportamento.
#[test]
fn a_bare_flag_still_falls_through_to_chat_not_to_whatsapp() {
    let dir = tempdir().expect("tempdir");
    // `--model` pertence a `chat`; se a injecao de subcomando tivesse
    // quebrado, isto viraria erro de argumento inesperado.
    let out = garra(dir.path(), &["--model", "whatsapp", "--help"]);
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !combined.contains("unexpected argument"),
        "a injecao de subcomando regrediu:\n{combined}"
    );
}
