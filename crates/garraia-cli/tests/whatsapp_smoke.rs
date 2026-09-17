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
fn whatsapp_help_lists_every_subcommand() {
    let dir = tempdir().expect("tempdir");
    let out = garra(dir.path(), &["whatsapp", "--help"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    for sub in ["link", "cloud", "status", "logout", "restore"] {
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

/// **F1 da auditoria R4, ponta a ponta.** O arquivado e uma credencial viva.
/// `status` tem de fala-lo em voz alta e `logout` tem de apaga-lo — os testes
/// unitarios afirmam o exit code e o disco, mas so este aqui le o que o
/// usuario de fato ve no terminal.
#[test]
fn status_announces_an_archived_session_and_logout_removes_it() {
    let dir = tempdir().expect("tempdir");
    // O `status` decide pelo arquivo existir; conteudo nao importa aqui.
    let account = dir.path().join("data").join("whatsapp").join("default");
    std::fs::create_dir_all(&account).expect("mkdir");
    let archived = account.join("session.enc.prev");
    std::fs::write(&archived, b"credencial-arquivada").expect("write");

    let status = garra(dir.path(), &["whatsapp", "status"]);
    let stdout = String::from_utf8_lossy(&status.stdout);
    assert!(
        stdout.contains("ARQUIVADA"),
        "o status precisa anunciar o arquivado, nao so dizer que nada esta \
vinculado:\n{stdout}"
    );
    assert!(
        stdout.contains("logout"),
        "e precisa dizer como limpar:\n{stdout}"
    );
    // E precisa OFERECER a volta, nao so mandar apagar: `restore_archive()`
    // existia e nenhuma superficie a expunha. Quem chegou aqui por um
    // re-vinculo interrompido quer, quase sempre, a sessao de volta.
    assert!(
        stdout.contains("restore"),
        "e precisa dizer como recuperar:\n{stdout}"
    );
    assert!(archived.is_file(), "status e leitura: nao apaga nada");

    // O `restore` devolve o arquivado ao lugar, sem TTY e sem perguntar.
    let restaurado = garra(dir.path(), &["whatsapp", "restore"]);
    assert!(
        restaurado.status.success(),
        "restore precisa aceitar o trabalho:\n{}",
        String::from_utf8_lossy(&restaurado.stderr)
    );
    assert!(!archived.exists(), "o .prev saiu do lugar");
    assert!(
        account.join("session.enc").is_file(),
        "e virou a sessao viva"
    );
    // Restaurado, ele volta a ser a sessao em uso — e um segundo `restore` ja
    // nao tem o que fazer.
    let de_novo = garra(dir.path(), &["whatsapp", "restore"]);
    assert_eq!(
        de_novo.status.code(),
        Some(69),
        "sem arquivada, `restore` recusa em vez de fingir que fez algo"
    );
    // E o `logout` continua limpando tudo: recoloca o arquivado ao lado da
    // sessao viva para conferir que nenhum dos dois sobrevive.
    std::fs::write(&archived, b"credencial-arquivada").expect("write");

    // Sem TTY o `logout` nao pergunta e vai direto ao ponto.
    let out = garra(dir.path(), &["whatsapp", "logout"]);
    assert!(out.status.success(), "logout precisa aceitar o trabalho");
    assert!(
        !archived.exists(),
        "a credencial arquivada tem de sumir:\n{}",
        String::from_utf8_lossy(&out.stdout)
    );
}

/// **`Context::from_env` ponta a ponta, com uma sessao VIVA.**
///
/// Os testes unitarios montam o `Context` a mao, entao `from_env` — e com ela
/// `resolved_data_dir()` — nao tinha nenhuma cobertura: apontar o
/// `data_dir` para o lugar errado passava com tudo verde. O custo do erro e
/// caro e silencioso: `status` diria "nenhum vinculado" numa maquina
/// vinculada, e `link` gravaria a sessao onde o gateway nao vai procurar.
///
/// Este teste grava a sessao **pelo mesmo `SessionStore`** que a CLI usa, num
/// diretorio que so o ambiente indica, e exige que o binario a encontre e
/// consiga ABRI-LA. O irmao acima cobre o mesmo caminho para o arquivado;
/// este cobre o vivo, que e o caso normal.
#[test]
fn status_finds_and_opens_a_live_session_written_where_from_env_resolves() {
    use garraia_channels::whatsapp_linked::{
        DEFAULT_ACCOUNT, SessionBlob, SessionKey, SessionStore,
    };

    let dir = tempdir().expect("tempdir");
    // `resolved_data_dir()` de uma config default: `<config_dir>/data`. Se a
    // CLI resolver outra coisa, ela nao acha o que gravamos aqui.
    let store = SessionStore::for_data_dir(&dir.path().join("data"), DEFAULT_ACCOUNT)
        .expect("DEFAULT_ACCOUNT e uma conta valida");
    let key = SessionKey::resolve(store.dir(), None).expect("chave");
    store
        .save(&SessionBlob::new("eyJhIjoxfQ=="), &key)
        .expect("save");

    let out = garra(dir.path(), &["whatsapp", "status"]);
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        stdout.contains("Vinculado: sim"),
        "o binario tem de achar a sessao no caminho que `from_env` resolve:\n{stdout}"
    );
    assert!(
        !stdout.contains("Nenhum WhatsApp pessoal vinculado"),
        "e nao pode dizer que nao ha nada:\n{stdout}"
    );
    // Prova que a chave tambem foi resolvida no mesmo lugar: `status` abre o
    // blob de verdade antes de dizer isto.
    assert!(
        stdout.contains("Leitura:  ok"),
        "a sessao tem de abrir, nao so existir:\n{stdout}"
    );
    assert_eq!(out.status.code(), Some(0), "sessao viva e legivel: exit 0");
}

/// O comando chama-se `whatsapp`, e nao `whats-app`.
///
/// Regressao real: o clap deriva o nome do subcomando em kebab-case a partir
/// do nome da variante, entao `Commands::WhatsApp` vira `whats-app` sozinho.
/// O comando ficou inteiramente inalcancavel por um `#[command(name = ...)]`
/// faltando, e os outros testes deste arquivo falharam todos de uma vez. Este
/// aqui pina os dois lados da moeda para o erro nao voltar em silencio.
#[test]
fn the_command_is_named_whatsapp_and_not_the_kebab_case_derivation() {
    let dir = tempdir().expect("tempdir");

    let good = garra(dir.path(), &["whatsapp", "--help"]);
    assert!(
        good.status.success(),
        "`garra whatsapp` precisa existir:\n{}",
        String::from_utf8_lossy(&good.stderr)
    );

    let bad = garra(dir.path(), &["whats-app", "--help"]);
    assert!(
        !bad.status.success(),
        "`whats-app` nao pode ser um nome valido — seria o derivado acidental"
    );
}
