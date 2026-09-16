//! Testes do comando `garra whatsapp`.
//!
//! O que e coberto aqui: as decisoes puras (idioma, textos, ordem de gravacao
//! na config) e os caminhos que nao precisam de bridge. O ciclo de vida do
//! processo filho e testado em `garraia-channels` contra a fixture Python; o
//! smoke da linha de comando esta em `tests/whatsapp_smoke.rs`.

use super::*;
use crate::wizard::prompts::Prompter;
use std::cell::RefCell;

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
    let code = temp_env_without_path(|| run(Action::Link, &ctx, &prompter));
    assert_eq!(code, 69, "faltando node, EX_UNAVAILABLE");

    let config = loader.load().expect("load");
    assert!(
        !config.channels.contains_key("whatsapp_linked"),
        "pareamento que falhou NAO pode ter escrito enabled = true"
    );
    assert!(
        !ctx.store().exists(),
        "e nao pode ter deixado sessao para tras"
    );
}

/// `PATH` vazio durante a closure. Serializado por um mutex proprio porque
/// mexer em env e global ao processo de teste.
fn temp_env_without_path<T>(f: impl FnOnce() -> T) -> T {
    use std::sync::Mutex;
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let previous = std::env::var_os("PATH");
    // SAFETY: o mutex acima serializa os testes que mexem em env neste binario.
    unsafe { std::env::set_var("PATH", "") };
    let out = f();
    // SAFETY: idem.
    unsafe {
        match previous {
            Some(v) => std::env::set_var("PATH", v),
            None => std::env::remove_var("PATH"),
        }
    }
    out
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
    let store = ctx.store();
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
    let store = ctx.store();
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

    let store = ctx.store();
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

#[test]
fn logout_answered_no_keeps_everything() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    let store = ctx.store();
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
    assert!(!ctx.store().exists());
}

#[test]
fn declining_the_consent_screen_cancels_without_touching_anything() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    let loader = ctx.loader.as_ref().expect("loader");
    loader.ensure_dirs().expect("dirs");

    let prompter = ScriptedPrompter::with_confirms(&[false]);
    assert_eq!(run(Action::Link, &ctx, &prompter), 1);

    assert!(!ctx.store().exists());
    let config = loader.load().expect("load");
    assert!(!config.channels.contains_key("whatsapp_linked"));
}

/// Com sessao existente e resposta "nao" ao re-vincular, o fluxo segue para a
/// validacao **sem** apagar nem arquivar — o caminho idempotente.
#[test]
fn declining_the_relink_prompt_never_archives_the_session() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    let store = ctx.store();
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
    let code = temp_env_without_path(|| run(Action::Link, &ctx, &prompter));

    assert_eq!(code, 69, "sem node");
    assert!(store.exists(), "a sessao continua la");
    assert!(
        !store.archive_path().exists(),
        "responder nao ao re-vincular nao arquiva nada"
    );
}
