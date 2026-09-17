//! Testes do comando `garra whatsapp`.
//!
//! O que e coberto aqui: as decisoes puras (idioma, textos, ordem de gravacao
//! na config) e os caminhos que nao precisam de bridge. O ciclo de vida do
//! processo filho e testado em `garraia-channels` contra a fixture Python; o
//! smoke da linha de comando esta em `tests/whatsapp_smoke.rs`.

use super::*;
use crate::wizard::prompts::Prompter;
use std::cell::RefCell;
use std::rc::Rc;

/// Prompter roteirizado: cada chamada consome a proxima resposta.
#[derive(Default)]
struct ScriptedPrompter {
    selects: RefCell<Vec<usize>>,
    confirms: RefCell<Vec<bool>>,
    inputs: RefCell<Vec<String>>,
    passwords: RefCell<Vec<String>>,
    seen_prompts: RefCell<Vec<String>>,
}

impl ScriptedPrompter {
    fn with_confirms(answers: &[bool]) -> Self {
        let p = Self::default();
        *p.confirms.borrow_mut() = answers.iter().rev().copied().collect();
        p
    }
}

impl Prompter for ScriptedPrompter {
    fn select(&self, prompt: &str, _options: &[&str], default: usize) -> anyhow::Result<usize> {
        self.seen_prompts.borrow_mut().push(prompt.to_string());
        Ok(self.selects.borrow_mut().pop().unwrap_or(default))
    }
    fn confirm(&self, prompt: &str, default: bool) -> anyhow::Result<bool> {
        self.seen_prompts.borrow_mut().push(prompt.to_string());
        Ok(self.confirms.borrow_mut().pop().unwrap_or(default))
    }
    fn input(&self, prompt: &str, default: &str) -> anyhow::Result<String> {
        self.seen_prompts.borrow_mut().push(prompt.to_string());
        Ok(self
            .inputs
            .borrow_mut()
            .pop()
            .unwrap_or_else(|| default.to_string()))
    }
    fn password(&self, prompt: &str, _confirmation: Option<&str>) -> anyhow::Result<String> {
        self.seen_prompts.borrow_mut().push(prompt.to_string());
        Ok(self.passwords.borrow_mut().pop().unwrap_or_default())
    }
}

fn ctx_in(dir: &tempfile::TempDir, interactive: bool) -> Context {
    Context {
        data_dir: dir.path().join("data"),
        loader: Some(ConfigLoader::with_dir(dir.path())),
        vault_passphrase: Some("passphrase-de-teste".into()),
        interactive,
        columns: Some(120),
        unicode: true,
        lang: Lang::Pt,
    }
}

// ---------------------------------------------------------------------------
// Idioma
// ---------------------------------------------------------------------------

#[test]
fn the_two_menu_options_exist_in_both_languages() {
    // A doc (`docs/whatsapp.md` §Textos) espelha esta tabela; se ela mudar sem
    // a doc mudar junto, este teste e o lembrete.
    assert!(MENU_1_PT.contains("QR"));
    assert!(MENU_1_EN.contains("QR"));
    assert!(MENU_2_PT.contains("Business"));
    assert!(MENU_2_EN.contains("Business"));
    assert_ne!(MENU_1_PT, MENU_1_EN);
    assert_ne!(MENU_2_PT, MENU_2_EN);
}

#[test]
fn the_consent_screen_names_the_ban_risk_and_the_secondary_number() {
    for lang in [Lang::Pt, Lang::En] {
        let body = consent_body(lang).join(" ").to_lowercase();
        assert!(
            body.contains("bloquead") || body.contains("blocked"),
            "a tela precisa dizer que a conta pode ser bloqueada ({lang:?})"
        );
        assert!(
            body.contains("secundári") || body.contains("secundari") || body.contains("secondary"),
            "a tela precisa recomendar um numero secundario ({lang:?})"
        );
        assert!(
            body.contains("oficial") || body.contains("official"),
            "a tela precisa dizer que o cliente e nao-oficial ({lang:?})"
        );
    }
}

#[test]
fn the_instructions_name_the_exact_menu_path_on_the_phone() {
    assert!(
        instructions(Lang::Pt)
            .join(" ")
            .contains("Aparelhos conectados")
    );
    assert!(instructions(Lang::En).join(" ").contains("Linked devices"));
}

#[test]
fn the_non_interactive_hint_names_both_commands() {
    for lang in [Lang::Pt, Lang::En] {
        let hint = non_interactive_hint(lang);
        assert!(hint.contains("garra whatsapp link"), "{hint}");
        assert!(hint.contains("garra whatsapp cloud"), "{hint}");
    }
}

// ---------------------------------------------------------------------------
// Ordem de gravacao na config — a regra herdada do Hermes
// ---------------------------------------------------------------------------

#[test]
fn enabling_the_channel_creates_the_section_once_and_is_idempotent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let loader = ConfigLoader::with_dir(dir.path());
    loader.ensure_dirs().expect("dirs");

    assert!(set_linked_enabled(&loader, true).expect("enable"));
    let config = loader.load().expect("load");
    let entry = config
        .channels
        .get("whatsapp_linked")
        .expect("a secao precisa existir");
    assert_eq!(entry.enabled, Some(true));
    assert_eq!(entry.channel_type, "whatsapp_linked");

    assert!(
        !set_linked_enabled(&loader, true).expect("re-enable"),
        "ligar de novo nao reescreve o arquivo"
    );
}

#[test]
fn disabling_a_channel_that_was_never_created_writes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let loader = ConfigLoader::with_dir(dir.path());
    loader.ensure_dirs().expect("dirs");

    assert!(!set_linked_enabled(&loader, false).expect("disable"));
    let config = loader.load().expect("load");
    assert!(
        !config.channels.contains_key("whatsapp_linked"),
        "nao criamos a secao so para escrever false"
    );
}

#[test]
fn enabling_the_linked_channel_does_not_touch_the_cloud_channel() {
    let dir = tempfile::tempdir().expect("tempdir");
    let loader = ConfigLoader::with_dir(dir.path());
    loader.ensure_dirs().expect("dirs");
    write_cloud_channel(&loader, "tok", "123", "verify", "secret").expect("cloud");

    set_linked_enabled(&loader, true).expect("enable");

    let config = loader.load().expect("load");
    let cloud = config.channels.get("whatsapp").expect("cloud sobrevive");
    assert_eq!(
        cloud.settings.get("access_token").and_then(|v| v.as_str()),
        Some("tok"),
        "os dois canais coexistem sem se pisar"
    );
    assert!(config.channels.contains_key("whatsapp_linked"));
}

/// **A regra do Hermes.** Um `link` que falhou nao pode deixar `enabled = true`
/// para tras: o gateway pagaria timeout e retry a cada boot.
#[test]
fn config_is_untouched_when_pairing_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    let loader = ctx.loader.as_ref().expect("loader");
    loader.ensure_dirs().expect("dirs");

    // `link` sem Node instalado (PATH vazio) falha antes de qualquer bridge.
    let prompter = ScriptedPrompter::with_confirms(&[true]);
    let code = link_with(&ctx, &prompter, no_node);
    assert_eq!(code, 69, "faltando node, EX_UNAVAILABLE");

    let config = loader.load().expect("load");
    assert!(
        !config.channels.contains_key("whatsapp_linked"),
        "pareamento que falhou NAO pode ter escrito enabled = true"
    );
    assert!(
        !ctx.store()
            .expect("DEFAULT_ACCOUNT e conta valida")
            .exists(),
        "e nao pode ter deixado sessao para tras"
    );
}

/// "Este computador nao tem Node instalado", sem tocar no ambiente do
/// processo.
///
/// Substitui um `unsafe { std::env::set_var("PATH", "") }` que era invalido
/// nos proprios termos: `set_var` exige que nenhuma outra thread esteja no
/// ambiente, e cada `tempfile::tempdir()` deste binario le `TMPDIR` — os
/// outros testes rodam concorrentes. Ver o docstring de [`super::link_with`].
fn no_node() -> Result<NodeRuntime, BridgeError> {
    Err(BridgeError::ToolMissing("node"))
}

/// Launcher que nunca sobe: o programa nao existe, entao o `spawn` falha e o
/// `pair` devolve erro sem QR, sem Node e sem bridge. E o que permite levar o
/// fluxo do `link` ate um desfecho de FALHA *depois* de o guard estar
/// instalado — o trecho que nenhum teste alcancava.
struct DeadLauncher;

impl BridgeLauncher for DeadLauncher {
    fn command(&self) -> Result<tokio::process::Command, BridgeError> {
        Ok(tokio::process::Command::new(
            "/nao/existe/garraia-bridge-de-teste",
        ))
    }
    fn describe(&self) -> String {
        "/nao/existe/garraia-bridge-de-teste".into()
    }
    fn dir(&self) -> std::path::PathBuf {
        std::path::PathBuf::from("/nao/existe")
    }
}

// ---------------------------------------------------------------------------
// Cloud
// ---------------------------------------------------------------------------

#[test]
fn the_cloud_wizard_writes_the_four_keys_and_hardens_the_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let loader = ConfigLoader::with_dir(dir.path());
    loader.ensure_dirs().expect("dirs");

    write_cloud_channel(&loader, "EAAG-token", "1234567890", "meu-verify", "app-sec")
        .expect("write");

    let config = loader.load().expect("load");
    let entry = config.channels.get("whatsapp").expect("secao");
    assert_eq!(entry.enabled, Some(true));
    for key in [
        "access_token",
        "phone_number_id",
        "verify_token",
        "app_secret",
    ] {
        assert!(entry.settings.contains_key(key), "faltou {key}");
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.path().join("config.yml"))
            .expect("stat")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "config.yml acabou de receber quatro segredos"
        );
    }
}

/// Segundo pino da supressao CodeQL do alerta 173 (`rust/cleartext-logging`,
/// `whatsapp.rs`, sink `store.dir().display()` na mensagem de sucesso do
/// `link`).
///
/// O primeiro pino e estrutural e mora no store:
/// `SessionStore::for_data_dir` **recusa** conta que nao seja um segmento
/// `[A-Za-z0-9_-]{1,64}`, entao nenhum caminho impresso por este comando pode
/// sair do data dir. Isso fecha o path traversal, mas nao fecha o outro medo
/// do alerta: `5511999998888` casa a regra e seria PII no stdout.
///
/// E isto que este teste fecha — que o segmento de conta impresso pela CLI e
/// a **constante** `DEFAULT_ACCOUNT`, e nao algo escolhido em tempo de
/// execucao. Ele varre a arvore inteira de `src/` (nao um arquivo so) e exige
/// que o segundo argumento de cada `for_data_dir(` seja literalmente
/// `DEFAULT_ACCOUNT` — `account.unwrap_or(DEFAULT_ACCOUNT)` nao passa.
///
/// A varredura nao e a prova do traversal: essa e
/// `an_account_that_escapes_the_data_dir_is_refused_before_any_write`, em
/// `garraia-channels`. Aqui a pergunta e outra, e menor.
#[test]
fn every_session_store_in_the_cli_uses_the_constant_account() {
    let src_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = rust_sources_under(&src_root);
    assert!(
        files.len() > 10,
        "a varredura precisa enxergar a arvore inteira da CLI, achou {} arquivo(s) em {}",
        files.len(),
        src_root.display()
    );

    let mut calls = 0;
    let mut offenders = Vec::new();
    for (name, source) in &files {
        for account in account_arguments(source) {
            calls += 1;
            if account != "DEFAULT_ACCOUNT" {
                offenders.push(format!("{name}: for_data_dir(.., {account})"));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "conta dinamica num SessionStore da CLI: os `println!` do `link` \
imprimem esse segmento, e a supressao CodeQL 173 depende de ele ser a \
constante `DEFAULT_ACCOUNT`.\n{}",
        offenders.join("\n")
    );
    assert!(
        calls >= 1,
        "o scan precisa ter achado a construcao do store"
    );
}

/// Todos os `.rs` sob `dir`, recursivamente. Ler do disco em vez de listar
/// `include_str!` a mao e o que faz um arquivo NOVO da CLI entrar na varredura
/// sem ninguem se lembrar dele — era exatamente assim que um `for_data_dir`
/// em `doctor.rs` passaria batido.
fn rust_sources_under(dir: &std::path::Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current)
            .unwrap_or_else(|e| panic!("ler {}: {e}", current.display()));
        for entry in entries.flatten() {
            let path = entry.path();
            // `symlink_metadata`: um link nao e seguido, pelo mesmo motivo do
            // scanner de skills — a varredura nao pode sair da arvore.
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if meta.is_dir() {
                stack.push(path);
            } else if meta.is_file()
                && path.extension().is_some_and(|e| e == "rs")
                // Modulos de teste ficam de fora, e a exclusao e estreita de
                // proposito: o que este scan protege e o que vai ao **stdout
                // de producao**, e codigo de teste nao imprime para o usuario.
                // Incluir `tests.rs` faria o scan achar as proprias fixtures
                // deste arquivo e a si mesmo.
                && path.file_name().is_some_and(|n| n != "tests.rs")
            {
                let text = std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("ler {}: {e}", path.display()));
                out.push((path.display().to_string(), text));
            }
        }
    }
    out
}

/// Segundo argumento de cada chamada a `for_data_dir(`, ja normalizado.
///
/// Le ate o parentese que fecha a chamada (contando aninhamento) e corta na
/// virgula de topo, em vez de olhar uma janela de N caracteres: era a janela
/// que deixava `account.unwrap_or(DEFAULT_ACCOUNT)` passar, porque a
/// constante aparecia dentro dela como *fallback*.
fn account_arguments(source: &str) -> Vec<String> {
    const NEEDLE: &str = "for_data_dir(";
    let mut out = Vec::new();
    for (idx, _) in source.match_indices(NEEDLE) {
        let mut depth = 1i32;
        let mut split = None;
        let rest = &source[idx + NEEDLE.len()..];
        let mut end = rest.len();
        for (i, ch) in rest.char_indices() {
            match ch {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = i;
                        break;
                    }
                }
                ',' if depth == 1 && split.is_none() => split = Some(i),
                _ => {}
            }
        }
        let args = &rest[..end];
        let account = match split {
            Some(at) if at < args.len() => &args[at + 1..],
            // Um argumento so: nao e a assinatura que conhecemos, entao o
            // scan reporta o texto cru em vez de fingir que esta tudo bem.
            _ => args,
        };
        // A virgula final que o rustfmt deixa numa chamada quebrada em varias
        // linhas nao e parte do argumento.
        let account = account.trim().trim_end_matches(',').trim();
        out.push(account.split_whitespace().collect::<Vec<_>>().join(" "));
    }
    out
}

/// As duas mutacoes que derrubaram a versao anterior deste scan, agora como
/// asserção sobre o proprio parser — o unico jeito de provar que ele as pega
/// sem plantar codigo ruim na arvore de verdade.
#[test]
fn the_account_scan_catches_a_dynamic_account_with_a_constant_fallback() {
    // O literal e montado, e nao escrito inteiro, para a varredura de verdade
    // (que le este diretorio) nao achar a fixture como se fosse call site.
    let needle = ["for_data_dir", "("].concat();
    let dinamico = format!(
        "let account = std::env::var(\"GARRAIA_WA_ACCOUNT\").ok();\n\
         SessionStore::{needle}&self.data_dir, account.unwrap_or(DEFAULT_ACCOUNT))"
    );
    assert_eq!(
        account_arguments(&dinamico),
        vec!["account.unwrap_or(DEFAULT_ACCOUNT)".to_string()],
        "a janela de 160 chars aprovava isto porque DEFAULT_ACCOUNT aparecia \
dentro dela; o argumento inteiro nao aprova"
    );

    let constante = format!("SessionStore::{needle}&self.data_dir, DEFAULT_ACCOUNT)");
    assert_eq!(account_arguments(&constante), vec!["DEFAULT_ACCOUNT"]);

    // Quebra de linha do rustfmt entre os argumentos continua sendo aceita.
    let quebrado = format!("SessionStore::{needle}\n    &self.data_dir,\n    DEFAULT_ACCOUNT,\n)");
    assert_eq!(account_arguments(&quebrado), vec!["DEFAULT_ACCOUNT"]);
}

/// `garra whatsapp cloud` rodado so para trocar um token nao pode levar junto
/// o resto da secao. Gates, allowlists e overrides que o operador escreveu a
/// mao em `channels.whatsapp` sao invisiveis para este wizard — e era
/// exatamente por isso que reconstruir a secao do zero os apagava em silencio.
#[test]
fn the_cloud_wizard_keeps_settings_it_did_not_ask_about() {
    let dir = tempfile::tempdir().expect("tempdir");
    let loader = ConfigLoader::with_dir(dir.path());
    loader.ensure_dirs().expect("dirs");

    // O operador ja tinha a secao, com uma chave que o wizard nao conhece e um
    // token antigo.
    let mut config = loader.load().expect("load");
    let mut settings = std::collections::HashMap::new();
    settings.insert(
        "allowed_senders".to_string(),
        serde_json::json!(["5511999990000"]),
    );
    settings.insert(
        "access_token".to_string(),
        serde_json::Value::String("token-antigo".into()),
    );
    config.channels.insert(
        "whatsapp".to_string(),
        ChannelConfig {
            channel_type: "whatsapp".to_string(),
            enabled: Some(true),
            settings,
        },
    );
    loader.save(&config).expect("save");

    write_cloud_channel(&loader, "token-novo", "1234567890", "meu-verify", "app-sec")
        .expect("write");

    let config = loader.load().expect("load");
    let entry = config.channels.get("whatsapp").expect("secao");
    assert_eq!(
        entry.settings.get("allowed_senders"),
        Some(&serde_json::json!(["5511999990000"])),
        "a chave do operador tem de sobreviver a troca de token"
    );
    assert_eq!(
        entry.settings.get("access_token"),
        Some(&serde_json::Value::String("token-novo".into())),
        "e as quatro chaves do wizard tem de ser sobrescritas"
    );
    assert_eq!(entry.enabled, Some(true));
}

// ---------------------------------------------------------------------------
// status / logout
// ---------------------------------------------------------------------------

#[test]
fn status_without_a_session_is_unavailable_not_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    assert_eq!(run(Action::Status, &ctx, &ScriptedPrompter::default()), 69);
}

#[test]
fn status_with_a_session_succeeds() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    let store = ctx.store().expect("DEFAULT_ACCOUNT e conta valida");
    let key = ctx.key().expect("key");
    store
        .save(
            &garraia_channels::whatsapp_linked::SessionBlob::new("eyJhIjoxfQ=="),
            &key,
        )
        .expect("save");

    assert_eq!(run(Action::Status, &ctx, &ScriptedPrompter::default()), 0);
}

#[test]
fn status_reports_an_unreadable_session_instead_of_claiming_it_is_fine() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut ctx = ctx_in(&dir, false);
    let store = ctx.store().expect("DEFAULT_ACCOUNT e conta valida");
    let key = ctx.key().expect("key");
    store
        .save(
            &garraia_channels::whatsapp_linked::SessionBlob::new("eyJhIjoxfQ=="),
            &key,
        )
        .expect("save");

    // Passphrase trocada: o arquivo existe, mas nao abre.
    ctx.vault_passphrase = Some("outra".into());
    assert_eq!(
        run(Action::Status, &ctx, &ScriptedPrompter::default()),
        69,
        "status que so olha o nome do arquivo mentiria aqui"
    );
}

#[test]
fn logout_without_a_session_is_a_no_op_that_succeeds() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    assert_eq!(run(Action::Logout, &ctx, &ScriptedPrompter::default()), 0);
}

#[test]
fn logout_purges_the_material_and_disables_the_channel() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    let loader = ctx.loader.as_ref().expect("loader");
    loader.ensure_dirs().expect("dirs");
    set_linked_enabled(loader, true).expect("enable");

    let store = ctx.store().expect("DEFAULT_ACCOUNT e conta valida");
    let key = ctx.key().expect("key");
    store
        .save(
            &garraia_channels::whatsapp_linked::SessionBlob::new("eyJhIjoxfQ=="),
            &key,
        )
        .expect("save");

    let prompter = ScriptedPrompter::with_confirms(&[true]);
    assert_eq!(run(Action::Logout, &ctx, &prompter), 0);

    assert!(!store.exists(), "o blob some");
    assert!(!store.key_path().exists(), "a chave some");
    let config = loader.load().expect("load");
    assert_eq!(
        config
            .channels
            .get("whatsapp_linked")
            .and_then(|c| c.enabled),
        Some(false),
        "e o canal fica desligado"
    );
}

/// **F1 (auditoria R4, bloqueante).** Um re-vinculo abortado deixa
/// `session.enc.prev` — uma credencial VIVA — no disco, com a `session.key` ao
/// lado. Antes da correcao, `logout` respondia "Nada para desvincular." e saia
/// 0, porque `store.exists()` so olha o `session.enc`. Os dois comandos que o
/// usuario tem para limpar mentiam enquanto a credencial estava la.
///
/// O caminho nao e exotico: e o que acontece toda vez que o pareamento novo
/// nao conclui — QR expirado, Ctrl+C, Node ausente, ponte travada.
#[test]
fn an_aborted_relink_leaves_nothing_that_logout_refuses_to_clean() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    let store = ctx.store().expect("DEFAULT_ACCOUNT e conta valida");
    let key = ctx.key().expect("key");

    // 1. sessao funcionando.
    store
        .save(
            &garraia_channels::whatsapp_linked::SessionBlob::new("eyJhIjoxfQ=="),
            &key,
        )
        .expect("save");

    // 2. o usuario responde "sim" ao re-vincular: o `link` arquiva.
    assert!(store.archive().expect("archive"));
    assert!(!store.exists(), "o blob saiu do lugar");
    assert!(
        store.archive_path().is_file(),
        "e virou .prev, ainda valido"
    );

    // 3. o pareamento novo NAO conclui. Nada restaura, nada apaga.

    // 4. o usuario faz o obvio.
    let prompter = ScriptedPrompter::with_confirms(&[true]);
    assert_eq!(
        run(Action::Logout, &ctx, &prompter),
        0,
        "logout precisa aceitar o trabalho"
    );

    // 5. e o material tem de sumir — este e o ponto.
    assert!(
        !store.archive_path().exists(),
        "a sessao arquivada e uma credencial viva: logout tem de apaga-la"
    );
    assert!(!store.key_path().exists(), "a chave ao lado dela tambem");
    assert!(!store.salt_path().exists());
}

/// O que este teste pina: com **so** o arquivado em disco, `status` continua
/// respondendo 69 (nao ha sessao viva) e **nao apaga nada** — ler nao e
/// limpar.
///
/// O que ele **nao** pina, e o nome antigo prometia: o anuncio na tela. Um
/// teste unitario nao le `println!`, entao apagar os dois
/// `print_archive_warning` o deixava verde. Quem cobre o anuncio e o smoke
/// `whatsapp_smoke::status_announces_an_archived_session_and_logout_removes_it`,
/// que roda o binario e le o stdout de verdade — e que morre na mutacao.
#[test]
fn an_archived_session_alone_keeps_status_unavailable_and_survives_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    let store = ctx.store().expect("DEFAULT_ACCOUNT e conta valida");
    let key = ctx.key().expect("key");
    store
        .save(
            &garraia_channels::whatsapp_linked::SessionBlob::new("eyJhIjoxfQ=="),
            &key,
        )
        .expect("save");
    store.archive().expect("archive");

    // Continua "nao vinculado" — isso e verdade —, mas nao pode ser 0, e o
    // usuario precisa saber que sobrou credencial.
    assert_eq!(run(Action::Status, &ctx, &ScriptedPrompter::default()), 69);
    assert!(
        store.archive_path().is_file(),
        "status e leitura: nao apaga nada"
    );
}

/// `logout` num diretorio de verdade vazio continua sendo no-op de sucesso —
/// a correcao do F1 nao pode transformar isso em trabalho ou em erro.
#[test]
fn logout_with_neither_a_session_nor_an_archive_is_still_a_no_op() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    let prompter = ScriptedPrompter::with_confirms(&[true]);
    assert_eq!(run(Action::Logout, &ctx, &prompter), 0);
}

#[test]
fn logout_answered_no_keeps_everything() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    let store = ctx.store().expect("DEFAULT_ACCOUNT e conta valida");
    let key = ctx.key().expect("key");
    store
        .save(
            &garraia_channels::whatsapp_linked::SessionBlob::new("eyJhIjoxfQ=="),
            &key,
        )
        .expect("save");

    let prompter = ScriptedPrompter::with_confirms(&[false]);
    assert_eq!(run(Action::Logout, &ctx, &prompter), 1, "cancelamento");
    assert!(store.exists(), "responder nao nao pode apagar nada");
}

// ---------------------------------------------------------------------------
// Menu
// ---------------------------------------------------------------------------

#[test]
fn the_menu_without_a_tty_prints_both_options_and_exits_zero() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    assert_eq!(run(Action::Menu, &ctx, &ScriptedPrompter::default()), 0);
}

#[test]
fn link_without_a_tty_also_exits_zero_with_the_hint() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    assert_eq!(run(Action::Link, &ctx, &ScriptedPrompter::default()), 0);
    assert!(
        !ctx.store()
            .expect("DEFAULT_ACCOUNT e conta valida")
            .exists()
    );
}

#[test]
fn declining_the_consent_screen_cancels_without_touching_anything() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    let loader = ctx.loader.as_ref().expect("loader");
    loader.ensure_dirs().expect("dirs");

    let prompter = ScriptedPrompter::with_confirms(&[false]);
    assert_eq!(run(Action::Link, &ctx, &prompter), 1);

    assert!(
        !ctx.store()
            .expect("DEFAULT_ACCOUNT e conta valida")
            .exists()
    );
    let config = loader.load().expect("load");
    assert!(!config.channels.contains_key("whatsapp_linked"));
}

/// **Nada destrutivo antes da ultima confirmacao.** Quem responde "sim" ao
/// re-vincular ainda vai ver a tela de consentimento, ainda pode recusa-la,
/// ainda pode dar Ctrl+C e ainda pode nao ter Node instalado. Em qualquer
/// desses caminhos o comando devolve erro — e a sessao que funcionava tem de
/// continuar no lugar, inteira.
///
/// Antes da correcao o `archive()` acontecia no `Ok(true)` do proprio prompt:
/// a credencial era desmontada antes de a pessoa sequer ver o que estava
/// aceitando.
#[test]
fn accepting_the_relink_but_failing_before_the_qr_leaves_the_session_untouched() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    let store = ctx.store().expect("DEFAULT_ACCOUNT e conta valida");
    let key = ctx.key().expect("key");
    store
        .save(
            &garraia_channels::whatsapp_linked::SessionBlob::new("eyJhIjoxfQ=="),
            &key,
        )
        .expect("save");

    // "sim" ao re-vincular, "sim" ao consentimento — e entao o fluxo morre na
    // deteccao do Node, que e o ponto de falha mais comum de todos.
    let prompter = ScriptedPrompter::with_confirms(&[true, true]);
    let code = link_with(&ctx, &prompter, no_node);

    assert_eq!(code, 69, "sem node");
    assert!(
        store.exists(),
        "a sessao que funcionava nao pode ter saido do lugar"
    );
    assert!(
        !store.archive_path().exists(),
        "e nada pode ter sido arquivado antes do QR aparecer"
    );
}

/// **A linha que instala o [`super::ArchiveGuard`] tem pino.**
///
/// Este e o teste que faltava. Os dois vizinhos acima morrem na deteccao do
/// Node, que acontece ANTES do guard — por construcao eles nunca o alcancam, e
/// por isso trocar o bloco inteiro do guard por um `store.archive()` cru, sem
/// inverso (o bug da rodada anterior, exatamente), deixava a crate verde.
///
/// Aqui o Node ja nao esta no caminho: entra-se direto pelo
/// [`super::link_paired`], com um launcher que nunca sobe. O fluxo percorre o
/// arquivamento, o QR que nunca aparece, o desfecho de erro e o `drop` do
/// guard. O que se afirma e o que o usuario ve em disco depois: a sessao
/// anterior de volta, legivel, e nada esquecido no `.prev`.
#[test]
fn a_relink_that_never_pairs_puts_the_previous_session_back() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    let loader = ctx.loader.as_ref().expect("loader");
    loader.ensure_dirs().expect("dirs");

    let store = ctx.store().expect("DEFAULT_ACCOUNT e conta valida");
    let key = ctx.key().expect("key");
    let anterior = garraia_channels::whatsapp_linked::SessionBlob::new("eyJhbnRlcmlvciI6MX0=");
    store.save(&anterior, &key).expect("save");

    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let code = link_paired(&ctx, &store, &key, &DeadLauncher, &runtime, true);

    assert_eq!(code, 69, "um bridge que nao sobe e EX_UNAVAILABLE");
    assert!(
        store.exists(),
        "o vinculo anterior tem de estar de volta em session.enc"
    );
    assert!(
        !store.archive_path().exists(),
        "e nada pode ter ficado para tras no .prev"
    );
    assert_eq!(
        store.load(&key).expect("a sessao restaurada abre"),
        anterior,
        "e tem de ser a MESMA sessao — restaurar um arquivo vazio nao restaura nada"
    );

    let config = loader.load().expect("load");
    assert!(
        !config.channels.contains_key("whatsapp_linked"),
        "um pareamento que falhou nao escreve enabled = true"
    );
}

/// **O botao que faltava.** `restore_archive()` existia desde a revisao
/// anterior e nenhuma superficie a expunha: o `status` via o arquivado e
/// mandava **apagar**.
#[test]
fn restore_brings_the_archived_session_back_and_enables_the_channel() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    let loader = ctx.loader.as_ref().expect("loader");
    loader.ensure_dirs().expect("dirs");
    let store = ctx.store().expect("DEFAULT_ACCOUNT e conta valida");
    let key = ctx.key().expect("key");
    let anterior = garraia_channels::whatsapp_linked::SessionBlob::new("eyJhbnRlcmlvciI6MX0=");
    store.save(&anterior, &key).expect("save");
    assert!(store.archive().expect("archive"), "havia o que arquivar");
    assert!(
        !store.exists(),
        "e o cenario e justamente o de ficar sem sessao viva"
    );

    assert_eq!(
        super::run(Action::Restore, &ctx, &ScriptedPrompter::default()),
        0
    );

    assert!(store.exists(), "a sessao voltou para session.enc");
    assert!(!store.archive_path().exists(), "e nao ficou copia no .prev");
    assert_eq!(
        store.load(&key).expect("a restaurada abre"),
        anterior,
        "e tem de ser a MESMA sessao"
    );
    let config = loader.load().expect("load");
    assert!(
        config
            .channels
            .get("whatsapp_linked")
            .is_some_and(|c| c.enabled == Some(true)),
        "com sessao em disco o canal volta a valer na config"
    );
}

/// Sem arquivado, o comando recusa o trabalho em vez de responder 0 dizendo
/// que fez algo.
#[test]
fn restore_without_an_archive_refuses_instead_of_claiming_success() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    ctx.loader
        .as_ref()
        .expect("loader")
        .ensure_dirs()
        .expect("dirs");

    assert_eq!(
        super::run(Action::Restore, &ctx, &ScriptedPrompter::default()),
        69
    );
}

/// E ele **nunca** passa por cima de uma sessao viva: a viva e a que o
/// servidor conhece.
#[test]
fn restore_never_clobbers_a_live_session() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    ctx.loader
        .as_ref()
        .expect("loader")
        .ensure_dirs()
        .expect("dirs");
    let store = ctx.store().expect("DEFAULT_ACCOUNT e conta valida");
    let key = ctx.key().expect("key");
    store
        .save(
            &garraia_channels::whatsapp_linked::SessionBlob::new("eyJ2ZWxoYSI6MX0="),
            &key,
        )
        .expect("save velha");
    assert!(store.archive().expect("archive"));
    let viva = garraia_channels::whatsapp_linked::SessionBlob::new("eyJ2aXZhIjoxfQ==");
    store.save(&viva, &key).expect("save viva");

    assert_eq!(
        super::run(Action::Restore, &ctx, &ScriptedPrompter::default()),
        69
    );
    assert_eq!(
        store.load(&key).expect("load"),
        viva,
        "a sessao viva nao pode ter sido substituida"
    );
    assert!(
        store.archive_path().is_file(),
        "e a arquivada fica onde esta"
    );
}

/// O aviso de arquivado tem de **oferecer** a recuperacao, nao so mandar
/// apagar — e essa oferta e a razao de o comando existir.
#[test]
fn the_archive_warning_offers_the_way_back() {
    let fonte = include_str!("../whatsapp.rs");
    let i = fonte
        .find("fn print_archive_warning")
        .expect("print_archive_warning precisa existir");
    let corpo = &fonte[i..i + 3000];
    assert!(
        corpo.contains("garra whatsapp restore"),
        "o aviso precisa nomear o comando que devolve a sessao"
    );
    assert!(
        corpo.contains("garra whatsapp logout"),
        "e continuar oferecendo o descarte"
    );
}

/// Saida do [`super::ArchiveGuard`] gravada em memoria.
///
/// `Rc<RefCell<…>>` e nao um canal porque o guard cai no `drop`, que roda no
/// mesmo thread: o teste so precisa ler depois.
#[derive(Default)]
struct RecordedOut {
    ok: Rc<RefCell<Vec<String>>>,
    warn: Rc<RefCell<Vec<String>>>,
}

impl super::GuardOut for RecordedOut {
    fn ok(&mut self, line: &str) {
        self.ok.borrow_mut().push(line.to_string());
    }
    fn warn(&mut self, line: &str) {
        self.warn.borrow_mut().push(line.to_string());
    }
}

/// **A mensagem de perda do vinculo tem pino.**
///
/// Era a correcao inteira do ponto 5 da rodada anterior e nao tinha teste
/// nenhum: arrancar o braco `Ok(false) if self.archived && !self.store.exists()`
/// por completo e neutralizar o campo `archived` deixava os 28 testes deste
/// arquivo verdes, identicos ao baseline. Nenhum dos tres testes do guard
/// alcancava esse braco, e nenhum teste do arquivo capturava a saida.
///
/// O cenario e o unico em que o usuario de fato perdeu o vinculo anterior: o
/// `.prev` some entre o `archive()` e o `drop`.
#[test]
fn the_guard_says_so_when_the_archived_session_vanished() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    let store = ctx.store().expect("DEFAULT_ACCOUNT e conta valida");
    let key = ctx.key().expect("key");
    store
        .save(
            &garraia_channels::whatsapp_linked::SessionBlob::new("eyJhbnRlcmlvciI6MX0="),
            &key,
        )
        .expect("save");

    let out = RecordedOut::default();
    let (ok, warn) = (Rc::clone(&out.ok), Rc::clone(&out.warn));
    {
        let _guard =
            super::ArchiveGuard::archive_to(&store, ctx.lang, Box::new(out)).expect("archive");
        // O arquivado some debaixo do guard — disco cheio, antivirus,
        // `rm` de alguem. E o unico dos tres desfechos de `Ok(false)` que
        // significa perda.
        std::fs::remove_file(store.archive_path()).expect("apagar o .prev");
    }

    assert!(
        ok.borrow().is_empty(),
        "nada foi restaurado, entao nada pode ter sido anunciado como restaurado: {:?}",
        ok.borrow()
    );
    let warn = warn.borrow();
    assert_eq!(warn.len(), 1, "esperava exatamente um aviso: {warn:?}");
    assert!(
        warn[0].contains("vínculo antigo foi perdido"),
        "o usuario precisa ouvir que perdeu o vinculo: {:?}",
        warn[0]
    );
    assert!(
        warn[0].contains("garra whatsapp"),
        "e precisa ouvir o que fazer a seguir: {:?}",
        warn[0]
    );
}

/// E o desfecho bom tambem fala — pelo outro canal.
///
/// O par do teste acima: sem ele, trocar `self.out.ok(…)` por silencio no
/// braco `Ok(true)` passaria despercebido, porque as asserções de disco dos
/// vizinhos nao olham a saida.
#[test]
fn the_guard_announces_the_session_it_put_back() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    let store = ctx.store().expect("DEFAULT_ACCOUNT e conta valida");
    let key = ctx.key().expect("key");
    store
        .save(
            &garraia_channels::whatsapp_linked::SessionBlob::new("eyJhbnRlcmlvciI6MX0="),
            &key,
        )
        .expect("save");

    let out = RecordedOut::default();
    let (ok, warn) = (Rc::clone(&out.ok), Rc::clone(&out.warn));
    drop(super::ArchiveGuard::archive_to(&store, ctx.lang, Box::new(out)).expect("archive"));

    assert!(
        warn.borrow().is_empty(),
        "nada deu errado: {:?}",
        warn.borrow()
    );
    let ok = ok.borrow();
    assert_eq!(ok.len(), 1, "esperava exatamente um anuncio: {ok:?}");
    assert!(
        ok[0].contains("restaurada"),
        "o usuario precisa ouvir que a sessao voltou: {:?}",
        ok[0]
    );
    assert!(store.exists(), "e ela precisa estar de volta em disco");
}

/// Sem `relink` o guard nao fala nada — nao havia o que arquivar.
#[test]
fn a_guard_with_nothing_archived_stays_quiet() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    let store = ctx.store().expect("DEFAULT_ACCOUNT e conta valida");

    let out = RecordedOut::default();
    let (ok, warn) = (Rc::clone(&out.ok), Rc::clone(&out.warn));
    drop(super::ArchiveGuard::archive_to(&store, ctx.lang, Box::new(out)).expect("archive"));

    assert!(ok.borrow().is_empty(), "{:?}", ok.borrow());
    assert!(
        warn.borrow().is_empty(),
        "um store vazio nao perdeu vinculo nenhum: {:?}",
        warn.borrow()
    );
}

/// O guard e no-op quando nao houve re-vinculo: sem `relink` nada e arquivado,
/// e uma falha do bridge nao pode inventar um `.prev`.
#[test]
fn a_first_link_that_never_pairs_archives_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    let store = ctx.store().expect("DEFAULT_ACCOUNT e conta valida");
    let key = ctx.key().expect("key");

    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let code = link_paired(&ctx, &store, &key, &DeadLauncher, &runtime, false);

    assert_eq!(code, 69);
    assert!(!store.exists(), "nao havia sessao e continua nao havendo");
    assert!(!store.archive_path().exists(), "nem arquivado");
}

/// Com sessao existente e resposta "nao" ao re-vincular, o fluxo segue para a
/// validacao **sem** apagar nem arquivar — o caminho idempotente.
#[test]
fn declining_the_relink_prompt_never_archives_the_session() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    let store = ctx.store().expect("DEFAULT_ACCOUNT e conta valida");
    let key = ctx.key().expect("key");
    store
        .save(
            &garraia_channels::whatsapp_linked::SessionBlob::new("eyJhIjoxfQ=="),
            &key,
        )
        .expect("save");

    // Responde "nao" ao re-vincular; o fluxo entao falha por falta de Node,
    // que e o que queremos — a asserção e sobre o estado do disco.
    let prompter = ScriptedPrompter::with_confirms(&[false]);
    let code = link_with(&ctx, &prompter, no_node);

    assert_eq!(code, 69, "sem node");
    assert!(store.exists(), "a sessao continua la");
    assert!(
        !store.archive_path().exists(),
        "responder nao ao re-vincular nao arquiva nada"
    );
}

/// **`restore` nao pode ligar o canal sem provar que o blob abre.**
///
/// `restore_archive()` move bytes; ela nao decifra nada. Se a `session.key`
/// se perdeu ou a passphrase do cofre mudou desde o arquivamento, a versao
/// anterior imprimia "✓ Sessão restaurada", gravava `enabled = true` e saia 0
/// — entregando ao gateway exatamente o `enabled` sem sessao utilizavel que a
/// ordem blob→enabled existe para evitar. O gateway paga timeout e retry a
/// cada boot, e ninguem fica sabendo.
#[test]
fn restore_refuses_to_enable_the_channel_when_the_blob_no_longer_opens() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    let loader = ctx.loader.as_ref().expect("loader");
    loader.ensure_dirs().expect("dirs");
    let store = ctx.store().expect("DEFAULT_ACCOUNT e conta valida");
    let key = ctx.key().expect("key");
    let anterior = garraia_channels::whatsapp_linked::SessionBlob::new("eyJhbnRlcmlvciI6MX0=");
    store.save(&anterior, &key).expect("save");
    assert!(store.archive().expect("archive"), "havia o que arquivar");

    // A passphrase do cofre mudou entre o arquivamento e o `restore`. O
    // ciphertext continua la; a chave que o abre, nao.
    let outra = Context {
        vault_passphrase: Some("outra-passphrase-completamente-diferente".into()),
        ..ctx_in(&dir, true)
    };

    assert_eq!(
        super::run(Action::Restore, &outra, &ScriptedPrompter::default()),
        69,
        "restaurar um blob que nao abre e EX_UNAVAILABLE, nao sucesso"
    );

    let config = loader.load().expect("load");
    assert!(
        config
            .channels
            .get("whatsapp_linked")
            .is_none_or(|c| c.enabled != Some(true)),
        "o canal NAO pode ter sido ligado com uma sessao que nao abre"
    );
}
