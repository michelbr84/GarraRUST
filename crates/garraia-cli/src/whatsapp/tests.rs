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

/// **Premissa da supressao CodeQL do alerta 173** (`rust/cleartext-logging`,
/// `whatsapp.rs`, sink `store.dir().display()` na mensagem de sucesso do
/// `link`).
///
/// O alerta e falso-positivo por **uma** razao, e so por ela: o segmento de
/// conta do caminho impresso e `DEFAULT_ACCOUNT`, constante de compilacao. O
/// que vai ao stdout e `<data_dir>/whatsapp/default/` — sem identificador de
/// usuario nenhum.
///
/// So que `SessionStore::for_data_dir` aceita **qualquer** string como conta, e
/// a fatia do gateway chama a mesma funcao. No dia em que alguem passar um
/// numero de telefone ali, tres `println!` deste comando passam a imprimi-lo e
/// a supressao vira mentira. Este teste e o que quebra nesse dia — sem ele, a
/// justificativa registrada no ledger nao tem nada que a sustente.
#[test]
fn every_session_store_in_the_cli_uses_the_constant_account() {
    let src = include_str!("../whatsapp.rs");
    let mut calls = 0;
    for (idx, _) in src.match_indices("for_data_dir(") {
        calls += 1;
        let tail = &src[idx..src.len().min(idx + 160)];
        assert!(
            tail.contains("DEFAULT_ACCOUNT"),
            "conta dinamica num SessionStore da CLI: a mensagem de sucesso do \
`link` imprime esse caminho, e a supressao CodeQL 173 depende de a conta ser \
constante. Trecho:\n{tail}"
        );
    }
    assert!(
        calls >= 1,
        "o scan precisa ter achado a construcao do store"
    );
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
    let store = ctx.store();
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
    let store = ctx.store();
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
    let store = ctx.store();
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
    let code = temp_env_without_path(|| run(Action::Link, &ctx, &prompter));

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
