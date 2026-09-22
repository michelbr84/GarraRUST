//! Smoke de `garra whatsapp` contra o binario de verdade.
//!
//! Molde: `tests/wizard_smoke.rs`. O que importa aqui e o contrato de
//! superficie — o que aparece na tela e o exit code — nos caminhos que um
//! script, um Dockerfile ou um `curl … | sh` realmente encontram. O ciclo de
//! vida do bridge e testado em `garraia-channels` contra a fixture Python.

use std::process::{Command, Stdio};

use garraia_channels::whatsapp_linked::{DEFAULT_ACCOUNT, SessionBlob, SessionKey, SessionStore};
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

/// #1329: a instrucao nomeia o executavel que rodou, nao um literal.
///
/// O teste acima roda o bin `garra` e ve `garra whatsapp link`; este copia o
/// mesmo binario para um diretorio limpo com o nome `garraia` — o unico que
/// existe numa imagem Docker, num `cargo install` ou num `install.sh` sem o
/// alias — e tem de ver `garraia whatsapp link`. Os dois juntos sao a prova
/// de que o nome vem de `current_exe()`, e nao de um literal. E copia, nao
/// symlink: `current_exe()` resolve o link e devolveria `garra` de novo.
#[test]
fn the_hint_names_the_binary_that_actually_ran() {
    let dir = tempdir().expect("tempdir");
    let garraia = dir.path().join("garraia");
    std::fs::copy(garra_bin(), &garraia).expect("copiar o binario como `garraia`");
    let out = Command::new(&garraia)
        .arg("whatsapp")
        .env("XDG_CONFIG_HOME", dir.path())
        .env("GARRAIA_CONFIG_DIR", dir.path())
        .env("HOME", dir.path())
        .env("GARRAIA_LANG", "pt_BR.UTF-8")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn garraia");
    assert!(out.status.success(), "exit {:?}", out.status.code());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("garraia whatsapp link"), "{stdout}");
    assert!(
        !stdout.contains("garra whatsapp link"),
        "o alias nao pode aparecer quando quem rodou foi o `garraia`:\n{stdout}"
    );
}

/// O `link` e o `cloud` num pipe nao podem repetir o texto do menu.
///
/// A versao anterior deste teste exigia exit 0 de `whatsapp link` "para
/// orientar e nao travar esperando um QR". Travar era de fato o risco certo,
/// mas o remedio estava errado, e o teste passava **porque** o defeito
/// existia: os tres comandos imprimiam o MESMO texto, byte a byte, e o texto
/// termina mandando rodar `garra whatsapp link`. Medido no binario de
/// verdade: `diff` entre as saidas de `whatsapp`, `whatsapp link` e
/// `whatsapp cloud` era vazio, e os tres saiam 0.
///
/// Ou seja, `ssh servidor 'garra whatsapp link'` — a forma mais provavel de
/// alguem conectar um GarraIA headless — respondia ao usuario com o comando
/// que ele tinha acabado de rodar, sem QR, sem erro, e com um exit code
/// dizendo que tinha dado certo. Ciclo fechado.
///
/// O que este teste prende agora: os dois fluxos escolhidos saem 69, dizem o
/// motivo, ensinam o `ssh -t`, e o texto deles **difere** do texto do menu.
#[test]
fn link_and_cloud_without_a_tty_refuse_instead_of_repeating_the_menu() {
    let dir = tempdir().expect("tempdir");

    let menu = garra(dir.path(), &["whatsapp"]);
    assert!(
        menu.status.success(),
        "o menu sem escolha continua saindo 0: ali o hint E a resposta"
    );
    let menu_out = String::from_utf8_lossy(&menu.stdout).to_string();

    for sub in ["link", "cloud"] {
        let out = garra(dir.path(), &["whatsapp", sub]);
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();

        assert_eq!(
            out.status.code(),
            Some(69),
            "`{sub}` num pipe tem de sair 69 (EX_UNAVAILABLE), nao 0:\nstdout:\n{stdout}"
        );

        // A assercao que mata a mutacao: se alguem voltar os dois bracos para
        // `non_interactive_hint`, as saidas voltam a ser identicas.
        assert_ne!(
            stdout, menu_out,
            "`{sub}` esta repetindo o texto do menu — o usuario que escolheu \
             o fluxo recebe de volta o comando que acabou de rodar"
        );
        // E o motivo mais a saida, para a mensagem nao ser so uma recusa.
        assert!(
            stdout.contains("ssh -t"),
            "`{sub}` precisa ensinar o `ssh -t`:\n{stdout}"
        );
        assert!(
            stdout.contains(&format!("garra whatsapp {sub}")),
            "a linha do `ssh -t` precisa terminar no proprio subcomando:\n{stdout}"
        );
    }
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
    for sub in [
        "link", "cloud", "status", "logout", "restore", "allow", "users", "remove", "owner",
        "unowner",
    ] {
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
    // O arquivado precisa ser uma sessao DE VERDADE, e nao bytes quaisquer:
    // desde a rodada 5 o `restore` abre o blob antes de ligar o canal, porque
    // "restaurado" sem "abre" entregava ao gateway um `enabled` que ele paga
    // em timeout a cada boot. Sem passphrase, a chave vive em `session.key`
    // ao lado do ciphertext, e o subprocesso `garra` resolve a mesma.
    let data_dir = dir.path().join("data");
    let store = SessionStore::for_data_dir(&data_dir, DEFAULT_ACCOUNT).expect("conta valida");
    let key = SessionKey::resolve(store.dir(), None).expect("chave");
    store
        .save(&SessionBlob::new("eyJhcnF1aXZhZGEiOjF9"), &key)
        .expect("save");
    assert!(store.archive().expect("archive"), "havia o que arquivar");
    let account = data_dir.join("whatsapp").join("default");
    let archived = account.join("session.enc.prev");
    assert!(archived.is_file(), "o cenario comeca com um arquivado real");

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

/// **Uma passphrase que so FALTA no ambiente nao pode consumir o arquivado.**
///
/// A prova de que o blob abre existia, e rodava DEPOIS de
/// `restore_archive()`. Bastava rodar `garra whatsapp restore` num shell sem
/// a passphrase do cofre — a distracao mais comum de quem normalmente a
/// exporta — para o `.prev` sair do lugar: exit 69, arquivo consumido, e a
/// mensagem mandando ler um QR novo, conselho que descartaria uma sessao
/// intacta. Nao sobrava comando que ligasse o canal: um segundo `restore`,
/// ja com a passphrase, respondia "nao ha arquivada".
///
/// O que este teste fixa nao e a mensagem, e o DISCO: o arquivado continua
/// onde estava, e por isso a segunda tentativa ainda tem o que restaurar.
#[test]
fn a_missing_vault_passphrase_never_consumes_the_archived_session() {
    let dir = tempdir().expect("tempdir");
    let data_dir = dir.path().join("data");
    let store = SessionStore::for_data_dir(&data_dir, DEFAULT_ACCOUNT).expect("conta valida");

    // A sessao arquivada foi cifrada COM passphrase do cofre...
    let key = SessionKey::resolve(store.dir(), Some("senha-do-cofre")).expect("chave");
    store
        .save(&SessionBlob::new("eyJhcnF1aXZhZGEiOjF9"), &key)
        .expect("save");
    assert!(store.archive().expect("archive"), "havia o que arquivar");

    let account = data_dir.join("whatsapp").join("default");
    let archived = account.join("session.enc.prev");
    assert!(archived.is_file(), "o cenario comeca com um arquivado real");

    // ...e o `restore` roda SEM ela no ambiente (o helper nao a exporta).
    let out = garra(dir.path(), &["whatsapp", "restore"]);
    assert_eq!(
        out.status.code(),
        Some(69),
        "restore sem a chave certa precisa recusar:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // **A asserção que importa.** Antes da correcao o `.prev` ja tinha sido
    // movido para `session.enc` quando a prova falhou.
    assert!(
        archived.is_file(),
        "o arquivado foi CONSUMIDO por uma falha que nao destruiu nada de \
fato — ele tem de continuar onde estava"
    );
    assert!(
        !account.join("session.enc").exists(),
        "e nada pode ter chegado ao lugar da sessao viva"
    );

    // E a mensagem nao pode mandar jogar fora o que ainda serve.
    let tela = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !tela.contains("QR"),
        "mandar ler um QR novo aqui descartaria uma sessao intacta:\n{tela}"
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

// ---------------------------------------------------------------------------
// #1345: `whatsapp allow` sem terminal
// ---------------------------------------------------------------------------

/// O mesmo `garra()`, com env extra (o perfil de execucao, sobretudo).
fn garra_env(dir: &std::path::Path, args: &[&str], env: &[(&str, &str)]) -> std::process::Output {
    let mut cmd = Command::new(garra_bin());
    cmd.args(args)
        .env("XDG_CONFIG_HOME", dir)
        .env("GARRAIA_CONFIG_DIR", dir)
        .env("HOME", dir)
        .env("GARRAIA_LANG", "pt_BR.UTF-8")
        .env_remove("GARRAIA_EXECUTION_PROFILE");
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("spawn garra")
}

fn config_yml(dir: &std::path::Path) -> String {
    std::fs::read_to_string(dir.join("config.yml")).unwrap_or_default()
}

/// Num pipe, `allow` grava o numero normalizado e sai 0 — e na tela so
/// aparecem os quatro ultimos digitos.
#[test]
fn allow_in_a_pipe_writes_the_config_and_exits_zero() {
    let dir = tempdir().expect("tempdir");
    let out = garra_env(dir.path(), &["whatsapp", "allow", "+55 11 99999-8888"], &[]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(0),
        "stdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        config_yml(dir.path()).contains("5511999998888"),
        "{}",
        config_yml(dir.path())
    );
    assert!(stdout.contains("8888"), "{stdout}");
    assert!(
        !stdout.contains("5511999998888") && !stderr.contains("5511999998888"),
        "numero inteiro nunca vai para a tela:\n{stdout}\n{stderr}"
    );
}

#[test]
fn allow_with_an_invalid_number_exits_65() {
    let dir = tempdir().expect("tempdir");
    // Sem `+`, o DDD passaria por codigo do pais (#1345, review WHATSAPP-11).
    for numero in [
        "abc",
        "011999998888",
        "5511999998888@s.whatsapp.net",
        "11 99999-8888",
        "(11) 99999-8888",
        "5511999998888",
    ] {
        let out = garra_env(dir.path(), &["whatsapp", "allow", numero], &[]);
        assert_eq!(out.status.code(), Some(65), "{numero}");
    }
    assert!(!config_yml(dir.path()).contains("allow"));
}

/// Um LID (`<id>@lid`) e gravado como veio, e na tela so o final (#1345).
#[test]
fn allow_accepts_a_lid_and_prints_only_its_last_digits() {
    let dir = tempdir().expect("tempdir");
    let out = garra_env(
        dir.path(),
        &["whatsapp", "allow", "87654321098765@lid"],
        &[],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    assert!(config_yml(dir.path()).contains("87654321098765@lid"));
    assert!(
        stdout.contains("LID") && stdout.contains("8765"),
        "{stdout}"
    );
    assert!(!stdout.contains("87654321098765"), "{stdout}");
}

/// `--owner` em `standard` sai 64 (mesmo com `--yes`); em `isolated-pod`
/// sem `--yes` tambem, porque num pipe nao ha a quem perguntar.
#[test]
fn allow_owner_needs_isolated_pod_and_yes_in_a_pipe() {
    let dir = tempdir().expect("tempdir");
    let out = garra_env(
        dir.path(),
        &["whatsapp", "allow", "+5511999998888", "--owner", "--yes"],
        &[],
    );
    assert_eq!(out.status.code(), Some(64), "standard recusa --owner");
    assert!(!config_yml(dir.path()).contains("owners"));

    let pod = [("GARRAIA_EXECUTION_PROFILE", "isolated-pod")];
    let out = garra_env(
        dir.path(),
        &["whatsapp", "allow", "+5511999998888", "--owner"],
        &pod,
    );
    assert_eq!(out.status.code(), Some(64), "pod sem --yes num pipe");
    assert!(!config_yml(dir.path()).contains("owners"));

    let out = garra_env(
        dir.path(),
        &["whatsapp", "allow", "+5511999998888", "--owner", "--yes"],
        &pod,
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(config_yml(dir.path()).contains("owners"));
    assert!(
        !config_yml(dir.path()).contains("isolated-pod"),
        "o perfil da env nao vai para o arquivo"
    );
}

/// `link --allow` pre-responde a pergunta, mas o QR continua precisando de
/// terminal: num pipe e 69 com o texto do `ssh -t`, e nada e gravado.
#[test]
fn link_with_allow_in_a_pipe_still_needs_a_terminal() {
    let dir = tempdir().expect("tempdir");
    let out = garra_env(
        dir.path(),
        &["whatsapp", "link", "--allow", "+5511999998888"],
        &[],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(69), "{stdout}");
    assert!(stdout.contains("ssh -t"), "{stdout}");
    assert!(!config_yml(dir.path()).contains("5511999998888"));
}

// ---------------------------------------------------------------------------
// #1393/#1394/#1389: listar, remover e o curinga, no binario de verdade
// ---------------------------------------------------------------------------

/// `users` num pipe: papel e os quatro ultimos digitos, nunca o numero
/// inteiro — e o `--json` e um documento que um `jq` le direto.
#[test]
fn users_lists_the_gate_without_printing_a_full_number() {
    let dir = tempdir().expect("tempdir");
    let allow = garra_env(dir.path(), &["whatsapp", "allow", "+55 11 99999-8888"], &[]);
    assert_eq!(allow.status.code(), Some(0));

    let out = garra_env(dir.path(), &["whatsapp", "users"], &[]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{stdout}\n{stderr}");
    assert!(stdout.contains("Autorizados: 1"), "{stdout}");
    assert!(stdout.contains("8888"), "{stdout}");
    assert!(
        !stdout.contains("5511999998888") && !stderr.contains("5511999998888"),
        "numero inteiro nunca vai para a tela:\n{stdout}\n{stderr}"
    );

    let out = garra_env(dir.path(), &["whatsapp", "users", "--json"], &[]);
    assert_eq!(out.status.code(), Some(0));
    let doc: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("o --json e um documento so");
    assert_eq!(doc["authorized"], 1);
    assert_eq!(doc["users"][0]["role"], "allow");
    assert_eq!(doc["users"][0]["last4"], "8888");
    assert!(
        !doc.to_string().contains("5511999998888"),
        "nem o JSON leva a identidade inteira"
    );
}

/// `remove` num pipe tira o numero e e idempotente; remover um DONO exige
/// `--yes`, e sem ele a config fica intacta.
#[test]
fn remove_revokes_a_number_and_never_drops_an_owner_silently() {
    let dir = tempdir().expect("tempdir");
    assert_eq!(
        garra_env(dir.path(), &["whatsapp", "allow", "+5511999998888"], &[])
            .status
            .code(),
        Some(0)
    );
    let out = garra_env(
        dir.path(),
        &["whatsapp", "remove", "+55 11 99999-8888"],
        &[],
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{stdout}");
    assert!(stdout.contains("8888"), "{stdout}");
    assert!(!config_yml(dir.path()).contains("5511999998888"), "saiu");
    // De novo: quem nao estava na lista nao e erro.
    assert_eq!(
        garra_env(dir.path(), &["whatsapp", "remove", "+5511999998888"], &[])
            .status
            .code(),
        Some(0)
    );

    let pod = [("GARRAIA_EXECUTION_PROFILE", "isolated-pod")];
    assert_eq!(
        garra_env(
            dir.path(),
            &["whatsapp", "allow", "+5511999998888", "--owner", "--yes"],
            &pod,
        )
        .status
        .code(),
        Some(0)
    );
    let out = garra_env(dir.path(), &["whatsapp", "remove", "+5511999998888"], &pod);
    assert_eq!(
        out.status.code(),
        Some(64),
        "dono num pipe precisa de --yes"
    );
    assert!(config_yml(dir.path()).contains("5511999998888"), "intacto");

    let out = garra_env(
        dir.path(),
        &["whatsapp", "remove", "+5511999998888", "--yes"],
        &pod,
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!config_yml(dir.path()).contains("5511999998888"));
}

// ---------------------------------------------------------------------------
// #1395: promover e rebaixar, no binario de verdade
// ---------------------------------------------------------------------------

/// `owner` num pipe: recusado em `standard` (64), promove no pod com `--yes`
/// (0), e na tela so aparecem os quatro ultimos digitos.
#[test]
fn owner_is_refused_in_standard_and_promotes_inside_the_pod() {
    let dir = tempdir().expect("tempdir");
    let pod = [("GARRAIA_EXECUTION_PROFILE", "isolated-pod")];
    assert_eq!(
        garra_env(dir.path(), &["whatsapp", "allow", "+5511999998888"], &[])
            .status
            .code(),
        Some(0)
    );

    // `standard` (o default do `garra_env`): o perfil manda, mesmo com --yes.
    let out = garra_env(
        dir.path(),
        &["whatsapp", "owner", "+5511999998888", "--yes"],
        &[],
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(64), "{stderr}");
    assert!(stderr.contains("isolated-pod"), "{stderr}");
    assert!(!config_yml(dir.path()).contains("owners"), "nada gravado");

    // No pod, sem terminal e sem `--yes`: continua 64.
    let out = garra_env(dir.path(), &["whatsapp", "owner", "+5511999998888"], &pod);
    assert_eq!(
        out.status.code(),
        Some(64),
        "dono num pipe precisa de --yes"
    );
    assert!(!config_yml(dir.path()).contains("owners"));

    let out = garra_env(
        dir.path(),
        &["whatsapp", "owner", "+55 11 99999-8888", "--yes"],
        &pod,
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{stdout}\n{stderr}");
    assert!(stdout.contains("8888"), "{stdout}");
    assert!(
        !stdout.contains("5511999998888") && !stderr.contains("5511999998888"),
        "numero inteiro nunca vai para a tela:\n{stdout}\n{stderr}"
    );
    assert!(config_yml(dir.path()).contains("owners"), "gravou");

    // O `users` reflete o papel novo, e promover de novo sai 0.
    let out = garra_env(dir.path(), &["whatsapp", "users", "--json"], &pod);
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(doc["owners"], 1);
    assert_eq!(doc["users"][0]["role"], "owners");
    assert_eq!(
        garra_env(
            dir.path(),
            &["whatsapp", "owner", "+5511999998888", "--yes"],
            &pod
        )
        .status
        .code(),
        Some(0),
        "promover duas vezes nao e erro"
    );
}

/// `unowner` tira o papel e **nunca** o acesso — e o ultimo dono exige
/// `--yes` num pipe. Rebaixar funciona tambem em `standard`, onde promover
/// nao funciona: um dono esquecido ali e justamente o que se quer limpar.
#[test]
fn unowner_demotes_without_ever_dropping_access() {
    let dir = tempdir().expect("tempdir");
    let pod = [("GARRAIA_EXECUTION_PROFILE", "isolated-pod")];
    for numero in ["+5511999998888", "+5511977776666"] {
        assert_eq!(
            garra_env(dir.path(), &["whatsapp", "owner", numero, "--yes"], &pod)
                .status
                .code(),
            Some(0),
            "{numero}"
        );
    }

    // Dois donos: rebaixar um nao precisa de confirmacao.
    let out = garra_env(dir.path(), &["whatsapp", "unowner", "+5511999998888"], &pod);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("8888"), "{stdout}");
    assert!(
        !stdout.contains("5511999998888"),
        "identidade inteira nunca vai para a tela:\n{stdout}"
    );

    // O acesso sobreviveu: ele e um `allow` agora, e o portao continua com 2.
    let out = garra_env(dir.path(), &["whatsapp", "users", "--json"], &pod);
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(doc["authorized"], 2, "ninguem perdeu acesso: {doc}");
    assert_eq!(doc["owners"], 1);
    assert!(
        !doc.to_string().contains("5511999998888"),
        "nem o JSON leva a identidade inteira"
    );

    // Sobrou um dono: num pipe, sem `--yes`, o comando para.
    let out = garra_env(dir.path(), &["whatsapp", "unowner", "+5511977776666"], &pod);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(64), "{stderr}");
    assert!(config_yml(dir.path()).contains("owners"), "intacto");

    // Com `--yes` sai — e rodando em `standard`, onde `owner` seria recusado.
    let out = garra_env(
        dir.path(),
        &["whatsapp", "unowner", "+5511977776666", "--yes"],
        &[],
    );
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = garra_env(dir.path(), &["whatsapp", "users", "--json"], &[]);
    let doc: serde_json::Value = serde_json::from_slice(&out.stdout).expect("json");
    assert_eq!(doc["owners"], 0, "sem dono nenhum");
    assert_eq!(doc["authorized"], 2, "e com os dois ainda autorizados");
    for u in doc["users"].as_array().expect("array") {
        assert_eq!(u["role"], "allow", "{doc}");
    }

    // Idempotente: rebaixar quem nao e dono nao e erro.
    assert_eq!(
        garra_env(dir.path(), &["whatsapp", "unowner", "+5511977776666"], &[])
            .status
            .code(),
        Some(0)
    );
}

/// #1389: `allow '*'` continua recusado (65), mas a mensagem diz que "abrir
/// para todos" nao existe — e nao que o numero tem caractere invalido.
#[test]
fn allow_with_a_wildcard_says_the_feature_does_not_exist() {
    let dir = tempdir().expect("tempdir");
    let out = garra_env(dir.path(), &["whatsapp", "allow", "*"], &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(65), "{stderr}");
    assert!(stderr.contains("todo mundo"), "{stderr}");
    assert!(
        !stderr.contains("só pode ter dígitos"),
        "a frase generica de caractere nao serve aqui: {stderr}"
    );
    assert!(!config_yml(dir.path()).contains("allow"), "nada gravado");
}
