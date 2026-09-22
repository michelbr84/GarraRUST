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
        gateway_pid: None,
        perfil_da_env: None,
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
    // O nome do executavel vem de `binario::nome()` (#1329): no harness e o
    // canonico `garraia`; num alias instalado seria `garra`.
    let bin = crate::binario::nome();
    for lang in [Lang::Pt, Lang::En] {
        let hint = non_interactive_hint(lang);
        assert!(hint.contains(&format!("{bin} whatsapp link")), "{hint}");
        assert!(hint.contains(&format!("{bin} whatsapp cloud")), "{hint}");
    }
}

/// #1329: a instrucao pos-link dizia `garra start` como literal, e quem so
/// tem o `garraia` (Docker, `cargo install`, instalacao sem o alias) recebia
/// um comando inexistente. Nenhuma frase deste comando pode voltar a fixar o
/// nome — ele sai de [`crate::binario::nome`] via `{bin}`.
#[test]
fn nenhuma_instrucao_fixa_o_nome_do_executavel() {
    let fonte = include_str!("../whatsapp.rs");
    // Os testes vivem neste arquivo, nao no `whatsapp.rs`, mas o corte
    // continua aqui por simetria com `desktop.rs::a_cli_nao_encosta_em_tauri`.
    let ate_o_teste = fonte
        .split("fn nenhuma_instrucao_fixa_o_nome_do_executavel")
        .next()
        .unwrap_or(fonte);
    let corpo: String = ate_o_teste
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for proibido in ["`garra start`", "garra whatsapp", "`garra init`"] {
        assert!(
            !corpo.contains(proibido),
            "`{proibido}` voltou como literal em whatsapp.rs — use `{{bin}}` com `tb()`"
        );
    }
    // E o mecanismo que substitui o literal continua em uso.
    assert!(
        corpo.contains("{bin} start"),
        "a instrucao pos-link usa {{bin}}"
    );
    assert!(
        corpo.contains("{bin} whatsapp"),
        "as demais instrucoes usam {{bin}}"
    );
    // Review C9: `{bin}` so faz sentido dentro de `tb(` (que substitui) ou
    // de um `format!` com `bin` no escopo. Um `{bin}` cujo ultimo abridor
    // de chamada e um `t(` cru seria impresso literalmente — e a suite
    // continuaria verde, porque o literal `{bin} start` continua no fonte.
    let mut de = 0;
    while let Some(i) = corpo[de..].find("{bin}") {
        let ate = de + i;
        let antes = &corpo[..ate];
        // O proprio `tb()` e quem faz o `.replace("{bin}", ...)` — nao e uma
        // frase impressa.
        if antes.ends_with(".replace(\"") {
            de = ate + "{bin}".len();
            continue;
        }
        let ultimo_tb = antes.rfind("tb(");
        let ultimo_format = antes.rfind("format!(");
        let ultimo_t_cru = antes
            .rfind(" t(")
            .or_else(|| antes.rfind("(t("))
            .or_else(|| antes.rfind("\tt("));
        let substitui = ultimo_tb.max(ultimo_format);
        assert!(
            substitui.is_some_and(|s| ultimo_t_cru.is_none_or(|t| s > t)),
            "um `{{bin}}` em whatsapp.rs esta dentro de um `t(` cru (byte {ate}); use `tb()` ou `format!`"
        );
        de = ate + "{bin}".len();
    }
}

/// ADR 0024, teste 8: a instrucao pos-link diz `garraia start` quando o
/// executavel se chama `garraia` e `garra start` quando `garra` — provado
/// na linha RENDERIZADA, nao so na presenca do literal `{bin}` no fonte
/// (review C9).
#[test]
fn a_instrucao_pos_link_nomeia_o_executavel_que_rodou() {
    for (bin, esperado) in [("garraia", "`garraia start`"), ("garra", "`garra start`")] {
        for lang in [Lang::Pt, Lang::En] {
            let linha = instrucao_pos_link(lang, bin, None, false);
            assert!(linha.contains(esperado), "{bin}/{lang:?}: {linha}");
            assert!(!linha.contains("{bin}"), "{linha}");
            assert!(!linha.contains("{}"), "{linha}");
        }
    }
    // E o nome real que a CLI passa e um dos dois — no harness, `garraia`.
    let real = instrucao_pos_link(Lang::Pt, &crate::binario::nome(), None, false);
    assert!(
        real.contains("`garraia start`") || real.contains("`garra start`"),
        "{real}"
    );
}

// ---------------------------------------------------------------------------
// Perfil de execucao no `status` (ADR 0024, #1329)
// ---------------------------------------------------------------------------

fn config_com_linked(
    profile: Option<garraia_config::ExecutionProfile>,
    settings: serde_json::Value,
) -> garraia_config::AppConfig {
    let mut channels = std::collections::HashMap::new();
    let settings = settings
        .as_object()
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();
    channels.insert(
        CONFIG_KEY.to_string(),
        ChannelConfig {
            channel_type: CONFIG_KEY.to_string(),
            enabled: Some(true),
            settings,
        },
    );
    garraia_config::AppConfig {
        channels,
        execution: garraia_config::ExecutionConfig::new(profile, None),
        ..Default::default()
    }
}

/// Em `standard` o piso e `search` e a origem e `default`; a linha existe
/// nas duas linguas e conta os donos sem lista-los.
#[test]
fn a_linha_do_perfil_em_standard_diz_search_e_conta_donos() {
    let config = config_com_linked(
        None,
        serde_json::json!({ "owners": ["5511999998888", "abc@lid"] }),
    );
    let pt = execution_profile_line(Lang::Pt, &config);
    assert!(pt.contains("standard"), "{pt}");
    assert!(pt.contains("fonte default"), "{pt}");
    assert!(pt.contains("piso do dono: search"), "{pt}");
    assert!(pt.contains("donos: 2"), "{pt}");
    assert!(
        !pt.contains("5511999998888"),
        "identidades nunca aparecem: {pt}"
    );
    assert!(!pt.contains("@lid"), "identidades nunca aparecem: {pt}");

    let en = execution_profile_line(Lang::En, &config);
    assert!(en.contains("Execution profile: standard"), "{en}");
    assert!(en.contains("owner floor: search"), "{en}");
    assert!(en.contains("owners: 2"), "{en}");
}

/// A contagem e a do GATEWAY (review C3/C8/F-5): entradas vazias,
/// nao-string e sem digito nao sao donos em lugar nenhum, entao a CLI nao
/// pode dizer que sao. `["", "  ", 123, "x", "5511999998888"]` e UM dono.
#[test]
fn a_linha_do_perfil_conta_donos_como_o_gateway() {
    let config = config_com_linked(
        Some(garraia_config::ExecutionProfile::IsolatedPod),
        serde_json::json!({ "owners": ["", "  ", 123, "x", "5511999998888"] }),
    );
    let pt = execution_profile_line(Lang::Pt, &config);
    assert!(pt.contains("donos: 1"), "{pt}");

    // Formatacao de numero e normalizada como no `allow`: um dono, nao dois.
    let config = config_com_linked(None, serde_json::json!({ "owners": ["+55 11 99999-8888"] }));
    assert!(execution_profile_line(Lang::Pt, &config).contains("donos: 1"));
}

/// Review C13: uma secao `whatsapp_linked` cuja `type` NAO e
/// `whatsapp_linked` nao e este canal — o gateway a ignora (zero donos,
/// nunca sobe), e a CLI tem de dizer o mesmo, nao `donos: 2` / `piso: code`.
#[test]
fn a_linha_do_perfil_ignora_secao_com_type_de_outro_canal() {
    let mut config = config_com_linked(
        Some(garraia_config::ExecutionProfile::IsolatedPod),
        serde_json::json!({ "owners": ["5511999998888", "5511888880000"], "default_mode": "code" }),
    );
    if let Some(ch) = config.channels.get_mut(CONFIG_KEY) {
        ch.channel_type = "whatsapp".to_string();
    }
    let pt = execution_profile_line(Lang::Pt, &config);
    assert!(pt.contains("donos: 0"), "{pt}");
    // Sem a secao, o piso e o default do perfil (`code` em isolated-pod),
    // porque o `default_mode` explicito da secao estranha tambem nao conta.
    assert!(pt.contains("piso do dono: code"), "{pt}");
}

/// Em `isolated-pod` (vindo do arquivo) o piso default do dono sobe para
/// `code`; um `default_mode` explicito vence nos dois perfis.
#[test]
fn a_linha_do_perfil_em_isolated_pod_diz_code_salvo_default_mode_explicito() {
    use garraia_config::ExecutionProfile;

    let pod = config_com_linked(Some(ExecutionProfile::IsolatedPod), serde_json::json!({}));
    let pt = execution_profile_line(Lang::Pt, &pod);
    assert!(pt.contains("isolated-pod (fonte file)"), "{pt}");
    assert!(pt.contains("piso do dono: code"), "{pt}");
    assert!(pt.contains("donos: 0"), "{pt}");

    let explicito = config_com_linked(
        Some(ExecutionProfile::IsolatedPod),
        serde_json::json!({ "default_mode": "search", "owners": ["5511999998888"] }),
    );
    let pt = execution_profile_line(Lang::Pt, &explicito);
    assert!(pt.contains("piso do dono: search"), "{pt}");
    assert!(pt.contains("donos: 1"), "{pt}");

    // O gemeo: `default_mode` explicito em standard tambem vence.
    let std_code = config_com_linked(None, serde_json::json!({ "default_mode": "code" }));
    assert!(execution_profile_line(Lang::Pt, &std_code).contains("piso do dono: code"));
}

/// A env aplicada pelo loader aparece como origem `env`.
#[test]
fn a_linha_do_perfil_mostra_a_origem_env() {
    use garraia_config::{ExecutionConfig, ExecutionProfile};
    let mut config = config_com_linked(None, serde_json::json!({}));
    config.execution =
        ExecutionConfig::new(None, None).com_env_aplicada(ExecutionProfile::IsolatedPod);
    let en = execution_profile_line(Lang::En, &config);
    assert!(en.contains("isolated-pod (source env)"), "{en}");
    assert!(en.contains("owner floor: code"), "{en}");
}

/// Sem config carregavel o `status` nao imprime a linha e nao muda de exit
/// code — a linha e um extra, nunca uma condicao.
#[test]
fn status_sem_config_nao_imprime_perfil_nem_falha() {
    let dir = tempfile::tempdir().expect("tmp");
    let mut ctx = ctx_in(&dir, false);
    ctx.loader = None;
    print_execution_profile(&ctx);
    // Sem sessao o `status` sai 69 exatamente como antes, com ou sem loader.
    assert_eq!(run(Action::Status, &ctx, &ScriptedPrompter::default()), 69);
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
    let code = link_with(&ctx, &prompter, &Pedido::default(), no_node);
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

/// Sem terminal, `link` e `cloud` recusam com 69 — e nao gravam nada.
///
/// Este teste exigia exit **0**, e era o segundo a codificar o defeito: os
/// tres comandos respondiam com o texto do menu, que termina mandando rodar
/// `garra whatsapp link`. O smoke
/// `link_and_cloud_without_a_tty_refuse_instead_of_repeating_the_menu` prova
/// no binario que as saidas hoje diferem; aqui a afirmacao e a do exit code
/// mais a de que a recusa acontece **antes** de qualquer escrita.
#[test]
fn link_and_cloud_without_a_tty_refuse_with_69_before_touching_disk() {
    for acao in [Action::Link, Action::Cloud] {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = ctx_in(&dir, false);
        assert_eq!(
            run(acao.clone(), &ctx, &ScriptedPrompter::default()),
            69,
            "{acao:?} num pipe tem de sair EX_UNAVAILABLE, nao 0"
        );
        assert!(
            !ctx.store()
                .expect("DEFAULT_ACCOUNT e conta valida")
                .exists(),
            "{acao:?} recusou, entao nao pode ter criado sessao"
        );
    }
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
    let code = link_with(&ctx, &prompter, &Pedido::default(), no_node);

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
    let code = link_paired(
        &ctx,
        &ScriptedPrompter::default(),
        &Pedido::default(),
        &store,
        &key,
        &DeadLauncher,
        &runtime,
        true,
    );

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
    // `{bin}` e o executavel em execucao (#1329); o subcomando e o que importa.
    assert!(
        corpo.contains("{bin} whatsapp restore"),
        "o aviso precisa nomear o comando que devolve a sessao"
    );
    assert!(
        corpo.contains("{bin} whatsapp logout"),
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
        warn[0].contains(&format!("{} whatsapp", crate::binario::nome())),
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
    let code = link_paired(
        &ctx,
        &ScriptedPrompter::default(),
        &Pedido::default(),
        &store,
        &key,
        &DeadLauncher,
        &runtime,
        false,
    );

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
    let code = link_with(&ctx, &prompter, &Pedido::default(), no_node);

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

// ---------------------------------------------------------------------------
// #1345: quem pode falar com o GarraIA depois do `link`
// ---------------------------------------------------------------------------

use acesso::{
    Acesso, Autorizado, Gravado, NumeroInvalido, Papel, acesso_da_config, autorizar,
    dica_do_gateway, final4, json_de_usuarios, linha_de_nao_estava, linha_de_removido,
    linhas_de_usuarios, listar, normalizar_numero, pos_link, remover,
};

const NUMERO: &str = "5511999998888";
/// Como o operador digita [`NUMERO`]: com `+` (obrigatorio, #1345).
const ENTRADA: &str = "+5511999998888";
/// Um LID sem numero, como a ponte o entrega.
const LID: &str = "87654321098765@lid";

impl ScriptedPrompter {
    /// Respostas de `input`, na ordem em que serao pedidas.
    fn with_inputs(self, answers: &[&str]) -> Self {
        *self.inputs.borrow_mut() = answers.iter().rev().map(|s| s.to_string()).collect();
        self
    }

    /// Respostas de `confirm`, na ordem em que serao pedidas.
    fn and_confirms(self, answers: &[bool]) -> Self {
        *self.confirms.borrow_mut() = answers.iter().rev().copied().collect();
        self
    }

    fn asked(&self, trecho: &str) -> bool {
        self.seen_prompts
            .borrow()
            .iter()
            .any(|p| p.contains(trecho))
    }
}

/// Grava uma config com o perfil pedido e, opcionalmente, a secao do canal.
fn grava_config(
    ctx: &Context,
    profile: Option<garraia_config::ExecutionProfile>,
    secao: Option<serde_json::Value>,
    enabled: Option<bool>,
) {
    let loader = ctx.loader.as_ref().expect("loader");
    loader.ensure_dirs().expect("dirs");
    let mut config = garraia_config::AppConfig {
        execution: garraia_config::ExecutionConfig::new(profile, None),
        ..Default::default()
    };
    if let Some(settings) = secao {
        config.channels.insert(
            CONFIG_KEY.to_string(),
            ChannelConfig {
                channel_type: CONFIG_KEY.to_string(),
                enabled,
                settings: settings
                    .as_object()
                    .expect("objeto")
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            },
        );
    }
    loader.save(&config).expect("save");
}

fn secao_de(ctx: &Context) -> Option<ChannelConfig> {
    ctx.loader
        .as_ref()
        .expect("loader")
        .load()
        .expect("load")
        .channels
        .get(CONFIG_KEY)
        .cloned()
}

fn lista(ctx: &Context, chave: &str) -> Vec<String> {
    secao_de(ctx)
        .and_then(|s| s.settings.get(chave).cloned())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| v.as_str().map(str::to_string))
        .collect()
}

fn pod() -> Option<garraia_config::ExecutionProfile> {
    Some(garraia_config::ExecutionProfile::IsolatedPod)
}

// --- normalizacao ----------------------------------------------------------

#[test]
fn o_numero_com_codigo_do_pais_vira_so_digitos_como_no_gateway() {
    for entrada in [
        "+55 11 99999-8888",
        "+5511999998888",
        "+55 (11) 99999.8888",
        "  +55 11 99999 8888  ",
    ] {
        let n = normalizar_numero(entrada).expect(entrada);
        assert_eq!(n, NUMERO, "{entrada}");
        assert_eq!(
            n,
            garraia_gateway::bootstrap::whatsapp_linked_normalizar_identidade(entrada),
            "a forma gravada e a que o portao do gateway compara: {entrada}"
        );
    }
    // Um numero dos EUA, com 11 digitos.
    assert_eq!(
        normalizar_numero("+1 555 123 4567").as_deref(),
        Ok("15551234567")
    );
    // E.164 curto, que a ponte entrega (6 a 15): Andorra (9) e Niue (7).
    assert_eq!(
        normalizar_numero("+376 312 345").as_deref(),
        Ok("376312345")
    );
    assert_eq!(normalizar_numero("+683 1234").as_deref(), Ok("6831234"));
}

/// #1345: um LID (`<digitos>@lid`) e aceito e gravado como veio — e e a
/// forma que o portao compara quando a ponte nao tem o numero.
#[test]
fn um_lid_e_aceito_como_veio_e_casa_com_o_gateway() {
    assert_eq!(normalizar_numero(LID).as_deref(), Ok(LID));
    assert_eq!(normalizar_numero(&format!("  {LID} ")).as_deref(), Ok(LID));
    assert_eq!(
        normalizar_numero(LID).ok(),
        Some(garraia_gateway::bootstrap::whatsapp_linked_normalizar_identidade(LID))
    );
    for invalido in [
        "abc@lid",
        "12@lid",
        "8765 4321@lid",
        "87654321098765@LID",
        "@lid",
    ] {
        assert_eq!(
            normalizar_numero(invalido),
            Err(NumeroInvalido::Jid),
            "{invalido}"
        );
    }
    assert_eq!(final4(LID), "8765", "o final e do id, nao de `@lid`");
}

#[test]
fn numero_sem_codigo_do_pais_letra_ou_jid_e_recusado() {
    for (entrada, esperado) in [
        ("", NumeroInvalido::Vazio),
        ("   ", NumeroInvalido::Vazio),
        ("+", NumeroInvalido::Vazio),
        ("+011 99999-8888", NumeroInvalido::ZeroInicial),
        ("+12345", NumeroInvalido::Tamanho(5)),
        ("+1234567890123456", NumeroInvalido::Tamanho(16)),
        // Sem `+`: DDD + numero passaria por codigo do pais (review
        // WHATSAPP-11), e o numero nunca casaria com o remetente.
        ("11 99999-8888", NumeroInvalido::SemMais),
        ("(11) 99999-8888", NumeroInvalido::SemMais),
        ("5511999998888", NumeroInvalido::SemMais),
        ("011 99999-8888", NumeroInvalido::SemMais),
        ("99999-8888", NumeroInvalido::SemMais),
        ("abc", NumeroInvalido::Caractere),
        ("55 11 9999a-8888", NumeroInvalido::Caractere),
        ("++5511999998888", NumeroInvalido::Caractere),
        ("5511999998888@s.whatsapp.net", NumeroInvalido::Jid),
    ] {
        assert_eq!(normalizar_numero(entrada), Err(esperado), "{entrada:?}");
    }
}

#[test]
fn a_mensagem_de_numero_invalido_nao_repete_a_entrada_e_existe_nas_duas_linguas() {
    for e in [
        NumeroInvalido::Vazio,
        NumeroInvalido::Jid,
        NumeroInvalido::SemMais,
        NumeroInvalido::Caractere,
        NumeroInvalido::ZeroInicial,
        NumeroInvalido::Curinga,
        NumeroInvalido::Tamanho(9),
    ] {
        let pt = e.mensagem(Lang::Pt);
        let en = e.mensagem(Lang::En);
        assert!(!pt.is_empty() && !en.is_empty() && pt != en, "{e:?}");
    }
    assert_eq!(final4(NUMERO), "8888");
    assert_eq!(final4("12"), "12");
}

// --- escrita na config -----------------------------------------------------

/// Instalacao nova: sem config nenhuma, `autorizar` cria a secao com o tipo
/// certo, grava o numero e NAO liga o canal.
#[test]
fn autorizar_numa_instalacao_nova_cria_a_secao_sem_ligar_o_canal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    let loader = ctx.loader.as_ref().expect("loader");

    assert_eq!(
        autorizar(loader, NUMERO, Papel::Autorizado).expect("grava"),
        Gravado::Novo
    );
    let secao = secao_de(&ctx).expect("secao criada");
    assert_eq!(secao.channel_type, CONFIG_KEY);
    assert_eq!(secao.enabled, None, "autorizar nunca liga o canal");
    assert_eq!(lista(&ctx, "allow"), vec![NUMERO.to_string()]);
    assert!(lista(&ctx, "owners").is_empty());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.path().join("config.yml"))
            .expect("config.yml")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600, "a config continua 0600");
    }
}

/// Upgrade: a secao existente e preservada chave a chave; o numero novo e
/// acrescentado, e o que ja estava (em qualquer grafia) nao duplica.
#[test]
fn autorizar_num_upgrade_preserva_tudo_e_nao_duplica() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    grava_config(
        &ctx,
        None,
        Some(serde_json::json!({
            "allow": ["+55 11 90000-0001"],
            "owners": ["5511900000002"],
            "default_mode": "search",
            "reply_in_groups": true,
        })),
        Some(true),
    );
    let loader = ctx.loader.as_ref().expect("loader");

    assert_eq!(
        autorizar(loader, NUMERO, Papel::Autorizado).expect("grava"),
        Gravado::Novo
    );
    assert_eq!(
        autorizar(loader, "5511900000001", Papel::Autorizado).expect("grava"),
        Gravado::JaEstava,
        "o que ja estava, em outra grafia, nao duplica"
    );

    let secao = secao_de(&ctx).expect("secao");
    assert_eq!(secao.enabled, Some(true), "enabled intocado");
    assert_eq!(
        lista(&ctx, "allow"),
        vec!["+55 11 90000-0001".to_string(), NUMERO.to_string()],
        "a entrada antiga fica como o operador escreveu"
    );
    assert_eq!(lista(&ctx, "owners"), vec!["5511900000002".to_string()]);
    assert_eq!(
        secao.settings.get("default_mode"),
        Some(&serde_json::json!("search"))
    );
    assert_eq!(
        secao.settings.get("reply_in_groups"),
        Some(&serde_json::json!(true))
    );
}

#[test]
fn dono_vai_so_para_owners() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    let loader = ctx.loader.as_ref().expect("loader");
    autorizar(loader, NUMERO, Papel::Dono).expect("grava");
    assert_eq!(lista(&ctx, "owners"), vec![NUMERO.to_string()]);
    assert!(lista(&ctx, "allow").is_empty(), "dono nao entra no allow");
}

/// Secao com `type` de outro canal: o gateway a ignora, entao "autorizado"
/// seria mentira. Recusa, e o arquivo fica como estava.
#[test]
fn autorizar_recusa_secao_de_outro_tipo_sem_escrever() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    let loader = ctx.loader.as_ref().expect("loader");
    loader.ensure_dirs().expect("dirs");
    let mut config = garraia_config::AppConfig::default();
    config.channels.insert(
        CONFIG_KEY.to_string(),
        ChannelConfig {
            channel_type: "whatsapp".into(),
            enabled: Some(true),
            settings: Default::default(),
        },
    );
    loader.save(&config).expect("save");
    let antes = std::fs::read_to_string(dir.path().join("config.yml")).expect("ler");

    assert!(autorizar(loader, NUMERO, Papel::Autorizado).is_err());
    let depois = std::fs::read_to_string(dir.path().join("config.yml")).expect("ler");
    assert_eq!(antes, depois);
}

// --- `garraia whatsapp allow` ----------------------------------------------

fn pedido(numero: &str, owner: bool, yes: bool) -> Pedido {
    Pedido {
        numero: Some(numero.to_string()),
        owner,
        yes,
    }
}

#[test]
fn allow_sem_terminal_autoriza_e_sai_zero() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    assert_eq!(
        run(
            Action::Allow(pedido("+55 11 99999-8888", false, false)),
            &ctx,
            &ScriptedPrompter::default()
        ),
        0
    );
    assert_eq!(lista(&ctx, "allow"), vec![NUMERO.to_string()]);
}

#[test]
fn allow_com_numero_invalido_sai_65_sem_escrever() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    assert_eq!(
        run(
            Action::Allow(pedido("abc", false, false)),
            &ctx,
            &ScriptedPrompter::default()
        ),
        65
    );
    assert!(secao_de(&ctx).is_none());
}

/// `--owner` em `standard`: 64 e a config intocada — mesmo com `--yes`.
#[test]
fn allow_owner_em_standard_sai_64_sem_escrever() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    let p = ScriptedPrompter::default().and_confirms(&[true]);
    assert_eq!(
        run(Action::Allow(pedido(ENTRADA, true, true)), &ctx, &p),
        64
    );
    assert!(secao_de(&ctx).is_none());
    assert!(!p.asked("DONO"), "nem pergunta");
}

/// #1345 (review WHATSAPP-16): o perfil do `--owner` sai do arquivo e da env
/// **capturada no contexto**, nunca relida do processo — o teste fixa o
/// perfil sem depender do `GARRAIA_EXECUTION_PROFILE` da maquina.
#[test]
fn allow_owner_decide_pelo_perfil_da_env_do_contexto() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut ctx = ctx_in(&dir, false);
    ctx.perfil_da_env = Some("isolated-pod".into());
    assert_eq!(
        run(
            Action::Allow(pedido(ENTRADA, true, true)),
            &ctx,
            &ScriptedPrompter::default()
        ),
        0,
        "a env do contexto diz pod: o dono entra"
    );
    assert_eq!(lista(&ctx, "owners"), vec![NUMERO.to_string()]);

    let dir = tempfile::tempdir().expect("tempdir");
    let mut ctx = ctx_in(&dir, false);
    ctx.perfil_da_env = Some("standard".into());
    grava_config(&ctx, pod(), None, None);
    assert_eq!(
        run(
            Action::Allow(pedido(ENTRADA, true, true)),
            &ctx,
            &ScriptedPrompter::default()
        ),
        64,
        "a env vence o arquivo, como no gateway"
    );
}

/// #1345: `allow <id>@lid` grava o LID como veio, e a tela so mostra o final.
#[test]
fn allow_de_um_lid_grava_o_lid_como_veio() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    assert_eq!(
        run(
            Action::Allow(pedido(LID, false, false)),
            &ctx,
            &ScriptedPrompter::default()
        ),
        0
    );
    assert_eq!(lista(&ctx, "allow"), vec![LID.to_string()]);
}

/// #1345 (review WHATSAPP-2): o mesmo celular brasileiro com e sem o nono
/// digito nao duplica — e a mesma chave que o portao compara.
#[test]
fn autorizar_nao_duplica_o_celular_com_e_sem_o_nono_digito() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    let loader = ctx.loader.as_ref().expect("loader");
    assert_eq!(
        autorizar(loader, "5531999998888", Papel::Autorizado).expect("grava"),
        Gravado::Novo
    );
    assert_eq!(
        autorizar(loader, "553199998888", Papel::Autorizado).expect("grava"),
        Gravado::JaEstava
    );
    assert_eq!(lista(&ctx, "allow"), vec!["5531999998888".to_string()]);
}

/// `--owner` em `isolated-pod`: sem terminal exige `--yes` (64); com
/// `--yes` grava em `owners` e so la.
#[test]
fn allow_owner_no_pod_exige_yes_fora_de_terminal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    grava_config(&ctx, pod(), None, None);

    assert_eq!(
        run(
            Action::Allow(pedido(ENTRADA, true, false)),
            &ctx,
            &ScriptedPrompter::default()
        ),
        64
    );
    assert!(lista(&ctx, "owners").is_empty());

    assert_eq!(
        run(
            Action::Allow(pedido(ENTRADA, true, true)),
            &ctx,
            &ScriptedPrompter::default()
        ),
        0
    );
    assert_eq!(lista(&ctx, "owners"), vec![NUMERO.to_string()]);
    assert!(lista(&ctx, "allow").is_empty());
}

/// No terminal, `--owner` sem `--yes` pergunta — e o default e NAO.
#[test]
fn allow_owner_no_terminal_pergunta_com_default_nao() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    grava_config(&ctx, pod(), None, None);
    let p = ScriptedPrompter::default();
    assert_eq!(
        run(Action::Allow(pedido(ENTRADA, true, false)), &ctx, &p),
        1
    );
    assert!(p.asked("DONO"));
    assert!(
        lista(&ctx, "owners").is_empty(),
        "default nao: nada gravado"
    );
}

// --- #1389: `allow '*'` nao e erro de digitacao ------------------------------

/// `*` tem erro PROPRIO: o generico `Caractere` dizia "só pode ter dígitos",
/// o que faz um recurso inexistente parecer erro de digitacao. A semantica de
/// "abrir para todos" depende da #1388 e **nao** existe — a mensagem diz isso,
/// e o numero continua sendo recusado.
#[test]
fn o_curinga_tem_erro_proprio_e_continua_recusado() {
    for entrada in ["*", " * ", "**", "+*"] {
        assert_eq!(
            normalizar_numero(entrada),
            Err(NumeroInvalido::Curinga),
            "{entrada:?}"
        );
    }
    let pt = NumeroInvalido::Curinga.mensagem(Lang::Pt);
    let en = NumeroInvalido::Curinga.mensagem(Lang::En);
    assert_ne!(
        pt,
        NumeroInvalido::Caractere.mensagem(Lang::Pt),
        "o `*` nao pode cair de novo na frase generica de caractere"
    );
    assert!(pt.contains('*') && en.contains('*'), "{pt} / {en}");
    // A frase precisa dizer o que NAO existe, e nao so "invalido".
    assert!(pt.contains("todo mundo"), "{pt}");
    assert!(en.contains("everyone"), "{en}");

    // E um `*` que se parece com numero continua valendo como numero.
    assert_eq!(
        normalizar_numero("+55 11 99999-8888").as_deref(),
        Ok(NUMERO)
    );
}

/// Ponta a ponta do comando: exit 65 (EX_DATAERR), nada escrito na config.
#[test]
fn allow_com_curinga_sai_65_sem_escrever() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    assert_eq!(
        run(
            Action::Allow(pedido("*", false, false)),
            &ctx,
            &ScriptedPrompter::default()
        ),
        65
    );
    assert!(secao_de(&ctx).is_none(), "nenhuma secao nasce de um `*`");
}

// --- #1393: `garraia whatsapp users` -----------------------------------------

/// A lista traz papel, tipo e SO os quatro ultimos digitos — a mesma regra do
/// resto do comando. Donos primeiro.
#[test]
fn users_lista_o_papel_e_so_o_final_de_cada_identidade() {
    let config = config_com_linked(
        None,
        serde_json::json!({ "allow": [NUMERO, LID], "owners": ["5511977776666"] }),
    );
    let usuarios = listar(&config);
    assert_eq!(
        usuarios,
        vec![
            Autorizado {
                final4: "6666".into(),
                papel: Papel::Dono,
                lid: false,
            },
            Autorizado {
                final4: "8888".into(),
                papel: Papel::Autorizado,
                lid: false,
            },
            Autorizado {
                final4: "8765".into(),
                papel: Papel::Autorizado,
                lid: true,
            },
        ]
    );

    let acesso = acesso_da_config(&config);
    let texto = linhas_de_usuarios(Lang::Pt, acesso, &usuarios).join("\n");
    assert!(texto.contains("Autorizados: 3 · Donos: 1"), "{texto}");
    assert!(texto.contains("dono"), "{texto}");
    assert!(texto.contains("autorizado"), "{texto}");
    assert!(texto.contains("LID terminado em 8765"), "{texto}");
    assert!(
        !texto.contains(NUMERO) && !texto.contains("5511977776666") && !texto.contains(LID),
        "identidade inteira nunca vai para a tela:\n{texto}"
    );
    let ingles = linhas_de_usuarios(Lang::En, acesso, &usuarios).join("\n");
    assert!(ingles.contains("Authorized: 3 · Owners: 1"), "{ingles}");
    assert!(ingles.contains("owner"), "{ingles}");
}

/// A mesma identidade em `allow` e em `owners` (ou com e sem o nono digito) e
/// UMA linha, com o papel que o gateway honra — senao a lista discordaria das
/// contagens do `status`, que ja fazem a uniao.
#[test]
fn users_nao_repete_quem_esta_nas_duas_listas() {
    let config = config_com_linked(
        None,
        serde_json::json!({ "allow": ["5531999998888"], "owners": ["553199998888"] }),
    );
    let usuarios = listar(&config);
    assert_eq!(usuarios.len(), 1, "{usuarios:?}");
    assert_eq!(usuarios[0].papel, Papel::Dono, "o papel que o portao honra");
    let a = acesso_da_config(&config);
    assert_eq!((a.autorizados, a.donos), (1, 1), "bate com o `status`");
}

/// Portao vazio: a lista nao inventa linha nenhuma e diz o que fazer.
#[test]
fn users_com_o_portao_vazio_aponta_o_allow_e_sai_zero() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    grava_config(&ctx, None, Some(serde_json::json!({})), Some(true));

    let config = config_com_linked(None, serde_json::json!({}));
    let linhas = linhas_de_usuarios(Lang::Pt, acesso_da_config(&config), &[]).join("\n");
    assert!(linhas.contains("Autorizados: 0"), "{linhas}");
    assert!(linhas.contains("whatsapp allow <"), "{linhas}");

    assert_eq!(
        run(
            Action::Users { json: false },
            &ctx,
            &ScriptedPrompter::default()
        ),
        0
    );
}

/// O `--json` e contrato de script: chaves em ingles, papel com o nome da
/// config (`allow`/`owners`) e `last4` — nunca a identidade inteira.
#[test]
fn users_json_usa_as_chaves_da_config_e_so_o_final() {
    let config = config_com_linked(
        None,
        serde_json::json!({ "allow": [NUMERO], "owners": ["5511977776666"] }),
    );
    let doc = json_de_usuarios(acesso_da_config(&config), &listar(&config));
    assert_eq!(doc["authorized"], 2);
    assert_eq!(doc["owners"], 1);
    assert_eq!(doc["enabled"], true, "o `enabled` sai do proprio canal");
    let users = doc["users"].as_array().expect("array");
    assert_eq!(users.len(), 2);
    assert_eq!(users[0]["role"], "owners");
    assert_eq!(users[0]["kind"], "number");
    assert_eq!(users[0]["last4"], "6666");
    assert_eq!(users[1]["role"], "allow");
    assert_eq!(users[1]["last4"], "8888");
    let texto = doc.to_string();
    assert!(
        !texto.contains(NUMERO) && !texto.contains("5511977776666"),
        "o JSON tambem so leva o final: {texto}"
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    grava_config(
        &ctx,
        None,
        Some(serde_json::json!({ "allow": [NUMERO] })),
        None,
    );
    assert_eq!(
        run(
            Action::Users { json: true },
            &ctx,
            &ScriptedPrompter::default()
        ),
        0
    );
}

// --- #1394: `garraia whatsapp remove` ----------------------------------------

fn remocao(numero: &str, yes: bool) -> PedidoRemocao {
    PedidoRemocao {
        numero: numero.to_string(),
        yes,
    }
}

/// O espelho do `allow`: tira o numero da lista, preserva o resto da secao,
/// nao desliga o canal — e rodar de novo continua saindo 0.
#[test]
fn remove_tira_o_numero_e_e_idempotente() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    grava_config(
        &ctx,
        None,
        Some(serde_json::json!({
            "allow": [NUMERO, "5511977776666"],
            "reply_in_groups": true,
        })),
        Some(true),
    );

    assert_eq!(
        run(
            Action::Remove(remocao("+55 11 99999-8888", false)),
            &ctx,
            &ScriptedPrompter::default()
        ),
        0
    );
    assert_eq!(lista(&ctx, "allow"), vec!["5511977776666".to_string()]);
    let secao = secao_de(&ctx).expect("secao");
    assert_eq!(
        secao.enabled,
        Some(true),
        "remover nunca desliga o canal — isso e do `logout`"
    );
    assert_eq!(
        secao.settings.get("reply_in_groups"),
        Some(&serde_json::Value::Bool(true)),
        "as outras chaves ficam como estavam"
    );

    // De novo: nada a remover, e isso nao e erro.
    assert_eq!(
        run(
            Action::Remove(remocao(ENTRADA, false)),
            &ctx,
            &ScriptedPrompter::default()
        ),
        0
    );
    assert_eq!(lista(&ctx, "allow"), vec!["5511977776666".to_string()]);
}

/// A chave de comparacao e a do portao: o mesmo celular com e sem o nono
/// digito sai, senao `remove` nao acharia o que `allow` gravou.
#[test]
fn remove_casa_pela_chave_do_portao_e_sai_das_duas_listas() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    let loader = ctx.loader.as_ref().expect("loader");
    grava_config(
        &ctx,
        None,
        Some(serde_json::json!({
            "allow": ["5531999998888"],
            "owners": ["553199998888"],
        })),
        None,
    );

    let fora = remover(loader, "5531999998888").expect("remove");
    assert_eq!((fora.de_allow, fora.de_owners), (1, 1));
    assert!(fora.era_dono());
    assert!(lista(&ctx, "allow").is_empty());
    assert!(lista(&ctx, "owners").is_empty());
}

/// Numa config sem a secao, remover nao cria nada e nao escreve no disco.
#[test]
fn remover_numa_config_sem_a_secao_nao_cria_nem_escreve() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    let loader = ctx.loader.as_ref().expect("loader");
    loader.ensure_dirs().expect("dirs");

    let fora = remover(loader, NUMERO).expect("remove");
    assert_eq!(fora.total(), 0);
    assert!(secao_de(&ctx).is_none());
    assert!(
        !dir.path().join("config.yml").exists(),
        "sem nada a remover, o arquivo nem nasce"
    );
}

/// **A regra do #1394:** dono nunca sai em silencio. Num pipe, sem `--yes`,
/// o comando sai 64 e a config fica intacta.
#[test]
fn remove_de_dono_sem_yes_fora_de_terminal_sai_64_sem_escrever() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    grava_config(
        &ctx,
        pod(),
        Some(serde_json::json!({ "owners": [NUMERO] })),
        None,
    );

    assert_eq!(
        run(
            Action::Remove(remocao(ENTRADA, false)),
            &ctx,
            &ScriptedPrompter::default()
        ),
        64
    );
    assert_eq!(lista(&ctx, "owners"), vec![NUMERO.to_string()], "intacto");

    // Com `--yes`, sai.
    assert_eq!(
        run(
            Action::Remove(remocao(ENTRADA, true)),
            &ctx,
            &ScriptedPrompter::default()
        ),
        0
    );
    assert!(lista(&ctx, "owners").is_empty());
}

/// No terminal, a pergunta existe e o default e NAO: um Enter distraido nao
/// revoga o dono.
#[test]
fn remove_de_dono_no_terminal_pergunta_com_default_nao() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    grava_config(
        &ctx,
        pod(),
        Some(serde_json::json!({ "owners": [NUMERO] })),
        None,
    );

    let p = ScriptedPrompter::default();
    assert_eq!(run(Action::Remove(remocao(ENTRADA, false)), &ctx, &p), 1);
    assert!(p.asked("DONO"), "a pergunta precisa nomear o papel");
    assert_eq!(
        lista(&ctx, "owners"),
        vec![NUMERO.to_string()],
        "default nao: nada removido"
    );

    // Respondendo sim, sai — e a linha diz que era dono.
    let p = ScriptedPrompter::default().and_confirms(&[true]);
    assert_eq!(run(Action::Remove(remocao(ENTRADA, false)), &ctx, &p), 0);
    assert!(lista(&ctx, "owners").is_empty());
}

/// Quem nao e dono nao passa por confirmacao nenhuma.
#[test]
fn remove_de_autorizado_nao_pergunta_nada() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    grava_config(
        &ctx,
        None,
        Some(serde_json::json!({ "allow": [NUMERO] })),
        None,
    );

    let p = ScriptedPrompter::default();
    assert_eq!(run(Action::Remove(remocao(ENTRADA, false)), &ctx, &p), 0);
    assert!(!p.asked("DONO"), "so dono e confirmado");
    assert!(lista(&ctx, "allow").is_empty());
}

#[test]
fn remove_com_numero_invalido_sai_65_sem_escrever() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    grava_config(
        &ctx,
        None,
        Some(serde_json::json!({ "allow": [NUMERO] })),
        None,
    );

    for entrada in ["abc", "11 99999-8888", "*"] {
        assert_eq!(
            run(
                Action::Remove(remocao(entrada, false)),
                &ctx,
                &ScriptedPrompter::default()
            ),
            65,
            "{entrada:?}"
        );
    }
    assert_eq!(lista(&ctx, "allow"), vec![NUMERO.to_string()]);
}

/// As duas linhas de desfecho: so o final do numero, nas duas linguas, e a de
/// dono diz o que aconteceu.
#[test]
fn as_linhas_do_remove_nao_repetem_a_identidade() {
    let so_allow = acesso::Remocao {
        de_allow: 1,
        de_owners: 0,
    };
    let dono = acesso::Remocao {
        de_allow: 0,
        de_owners: 1,
    };
    for lang in [Lang::Pt, Lang::En] {
        for linha in [
            linha_de_removido(lang, NUMERO, so_allow),
            linha_de_removido(lang, NUMERO, dono),
            linha_de_nao_estava(lang, NUMERO),
            linha_de_removido(lang, LID, so_allow),
        ] {
            assert!(!linha.is_empty());
            assert!(
                !linha.contains(NUMERO) && !linha.contains("87654321098765"),
                "{linha}"
            );
        }
        assert!(
            linha_de_removido(lang, NUMERO, dono).contains("DONO")
                || linha_de_removido(lang, NUMERO, dono).contains("OWNER"),
            "a remocao de dono tem de dizer que era dono"
        );
        assert!(linha_de_removido(lang, LID, so_allow).contains("LID"));
    }
    assert!(linha_de_removido(Lang::Pt, NUMERO, so_allow).contains("8888"));
}

// --- o passo pos-link --------------------------------------------------------

/// Instalacao nova: o `link` pergunta o numero e o grava normalizado; so
/// entao a linha final e "pronto".
#[test]
fn pos_link_numa_instalacao_nova_grava_o_numero_e_diz_pronto() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    grava_config(&ctx, None, Some(serde_json::json!({})), Some(true));
    let p = ScriptedPrompter::default().with_inputs(&["+55 11 99999-8888"]);

    let pos = pos_link(&ctx, &p, Some("0000"), &Pedido::default()).expect("ok");
    assert_eq!(pos.autorizados, 1);
    assert_eq!(lista(&ctx, "allow"), vec![NUMERO.to_string()]);
    assert!(!p.asked("DONO"), "em standard dono nem e oferecido");
    let linha = final_line(Lang::Pt, pos.autorizados, None, false);
    assert!(linha.contains("pronto"), "{linha}");
}

/// Resposta vazia: ninguem por enquanto — sem "pronto", com o aviso e o
/// comando que resolve.
#[test]
fn pos_link_com_resposta_vazia_nao_diz_pronto_e_aponta_o_allow() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    grava_config(&ctx, None, Some(serde_json::json!({})), Some(true));
    let p = ScriptedPrompter::default().with_inputs(&[""]);

    let pos = pos_link(&ctx, &p, None, &Pedido::default()).expect("ok");
    assert_eq!(pos.autorizados, 0);
    for lang in [Lang::Pt, Lang::En] {
        let linha = final_line(lang, 0, None, false);
        assert!(
            !linha.contains("pronto") && !linha.contains("ready"),
            "{linha}"
        );
        assert!(linha.contains("whatsapp allow <"), "{linha}");
    }
}

#[test]
fn pos_link_desiste_depois_de_tres_numeros_invalidos() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    grava_config(&ctx, None, Some(serde_json::json!({})), Some(true));
    let p = ScriptedPrompter::default().with_inputs(&["abc", "0119", "123", ENTRADA]);
    let pos = pos_link(&ctx, &p, None, &Pedido::default()).expect("ok");
    assert_eq!(pos.autorizados, 0, "a quarta resposta nem e pedida");
    assert!(lista(&ctx, "allow").is_empty());
}

/// Em `isolated-pod` o dono e oferecido, com default NAO: nao responder
/// deixa `owners` vazio e o numero so no `allow`.
#[test]
fn pos_link_no_pod_oferece_dono_com_default_nao() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    grava_config(&ctx, pod(), Some(serde_json::json!({})), Some(true));
    let p = ScriptedPrompter::default().with_inputs(&[ENTRADA]);

    pos_link(&ctx, &p, None, &Pedido::default()).expect("ok");
    assert!(p.asked("DONO"), "no pod o dono e oferecido");
    assert!(lista(&ctx, "owners").is_empty());
    assert_eq!(lista(&ctx, "allow"), vec![NUMERO.to_string()]);
}

#[test]
fn pos_link_no_pod_com_sim_grava_so_em_owners() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    grava_config(&ctx, pod(), Some(serde_json::json!({})), Some(true));
    let p = ScriptedPrompter::default()
        .with_inputs(&[ENTRADA])
        .and_confirms(&[true]);

    let pos = pos_link(&ctx, &p, None, &Pedido::default()).expect("ok");
    assert_eq!(pos.autorizados, 1);
    assert_eq!(lista(&ctx, "owners"), vec![NUMERO.to_string()]);
    assert!(lista(&ctx, "allow").is_empty());
}

/// O final do numero bate com o do celular vinculado: avisa do `from_me` e o
/// default e NAO autorizar.
#[test]
fn pos_link_avisa_quando_o_numero_e_o_do_proprio_celular() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    grava_config(&ctx, None, Some(serde_json::json!({})), Some(true));
    let p = ScriptedPrompter::default().with_inputs(&[ENTRADA, ""]);

    let pos = pos_link(&ctx, &p, Some("8888"), &Pedido::default()).expect("ok");
    assert!(p.asked("mesmo assim"), "pede confirmacao explicita");
    assert_eq!(pos.autorizados, 0, "default nao");
    assert!(lista(&ctx, "allow").is_empty());
}

/// Re-vinculo / upgrade com gente autorizada: mostra as contagens, pergunta
/// se quer adicionar outro (default NAO) e nao mexe em nada.
#[test]
fn pos_link_com_allow_existente_nao_mexe_em_nada_por_default() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    grava_config(
        &ctx,
        None,
        Some(serde_json::json!({ "allow": ["5511900000001"], "default_mode": "search" })),
        Some(true),
    );
    let antes = std::fs::read_to_string(dir.path().join("config.yml")).expect("ler");
    let p = ScriptedPrompter::default().with_inputs(&[ENTRADA]);

    let pos = pos_link(&ctx, &p, None, &Pedido::default()).expect("ok");
    assert!(p.asked("Adicionar outro"));
    assert_eq!(pos.autorizados, 1);
    let depois = std::fs::read_to_string(dir.path().join("config.yml")).expect("ler");
    assert_eq!(antes, depois, "nada mudou");
}

/// `link --allow` pre-responde o numero: nada e perguntado sobre ele.
#[test]
fn pos_link_com_numero_pre_respondido_nao_pergunta() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    grava_config(
        &ctx,
        None,
        Some(serde_json::json!({ "allow": ["5511900000001"] })),
        Some(true),
    );
    let p = ScriptedPrompter::default();
    let pos = pos_link(&ctx, &p, None, &pedido("+55 11 99999-8888", false, false)).expect("ok");
    assert_eq!(pos.autorizados, 2);
    assert!(!p.asked("Número autorizado"));
    assert!(!p.asked("Adicionar outro"));
}

/// `link --owner` em `standard` e `link --allow abc` falham antes do QR.
#[test]
fn link_com_pre_respostas_invalidas_falha_antes_do_qr() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    let p = ScriptedPrompter::default();
    assert_eq!(
        link_with(&ctx, &p, &pedido("abc", false, false), no_node),
        65
    );
    assert_eq!(
        link_with(
            &ctx,
            &p,
            &Pedido {
                numero: None,
                owner: true,
                yes: false
            },
            no_node
        ),
        64
    );
    assert!(
        p.seen_prompts.borrow().is_empty(),
        "nem a tela de consentimento"
    );
    // E num pipe continua 69, antes de validar qualquer coisa.
    let pipe = ctx_in(&dir, false);
    assert_eq!(
        run(
            Action::LinkCom(pedido(ENTRADA, false, false)),
            &pipe,
            &ScriptedPrompter::default()
        ),
        69
    );
}

// --- textos ----------------------------------------------------------------

#[test]
fn a_dica_do_gateway_cobre_os_tres_casos_sem_reiniciar_nada() {
    for lang in [Lang::Pt, Lang::En] {
        let parado = dica_do_gateway(lang, false, None);
        assert!(parado.contains(" start`"), "{parado}");
        let novo = dica_do_gateway(lang, false, Some(42));
        assert!(novo.contains(" restart`"), "{novo}");
        // #1345 (review WHATSAPP-3/7/12): a CLI nao sabe se o gateway subiu
        // com o canal ligado, nem se ele vigia o config.yml. Nunca afirma
        // "sem reiniciar": diz a condicao e o comando do outro caso.
        let quente = dica_do_gateway(lang, true, Some(42));
        assert!(quente.contains(" restart`"), "{quente}");
        assert!(
            quente.starts_with("Se o gateway subiu")
                || quente.starts_with("If the gateway started"),
            "{quente}"
        );
    }
    let linha = instrucao_pos_link(Lang::Pt, "garraia", Some(42), false);
    assert!(
        linha.contains("pronto") && linha.contains("restart"),
        "{linha}"
    );
}

/// #1345: o `status` explica as recusas de `@lid` sem numero que o gateway
/// VIVO registrou — so a contagem e o final, nunca o LID inteiro.
#[test]
fn o_status_explica_as_recusas_de_lid_do_gateway_vivo() {
    let r = garraia_gateway::bootstrap::WhatsAppLinkedRecusasLid {
        pid: 42,
        recusas: 2,
        final4: "8765".into(),
    };
    for lang in [Lang::Pt, Lang::En] {
        let linha = recusas_lid_line(lang, Some(&r), Some(42)).expect("gateway vivo");
        assert!(linha.contains("@lid") && linha.contains("8765"), "{linha}");
        assert!(linha.contains("whatsapp allow <id>@lid"), "{linha}");
        assert!(linha.contains('2'), "{linha}");
    }
    assert_eq!(
        recusas_lid_line(Lang::Pt, Some(&r), Some(43)),
        None,
        "arquivo de outro gateway"
    );
    assert_eq!(
        recusas_lid_line(Lang::Pt, Some(&r), None),
        None,
        "gateway parado"
    );
    assert_eq!(recusas_lid_line(Lang::Pt, None, Some(42)), None);
    let zero = garraia_gateway::bootstrap::WhatsAppLinkedRecusasLid { recusas: 0, ..r };
    assert_eq!(recusas_lid_line(Lang::Pt, Some(&zero), Some(42)), None);
}

#[test]
fn o_status_mostra_contagens_e_avisa_o_portao_vazio_sem_numeros() {
    let vazio = Acesso {
        enabled: true,
        autorizados: 0,
        donos: 0,
    };
    let linhas = access_lines(Lang::Pt, true, Some(7), Some(vazio)).join("\n");
    assert!(linhas.contains("Autorizados: 0"), "{linhas}");
    assert!(linhas.contains("Gateway:  rodando (pid 7)"), "{linhas}");
    assert!(linhas.contains("Canal:    ligado"), "{linhas}");
    assert!(linhas.contains("whatsapp allow <"), "{linhas}");

    let config = config_com_linked(
        None,
        serde_json::json!({ "allow": [NUMERO], "owners": [NUMERO] }),
    );
    let a = acesso_da_config(&config);
    assert_eq!((a.autorizados, a.donos), (1, 1), "uniao sem repeticao");
    let linhas = access_lines(Lang::En, false, None, Some(a)).join("\n");
    assert!(linhas.contains("Authorized: 1 · Owners: 1"), "{linhas}");
    assert!(linhas.contains("dependencies missing"), "{linhas}");
    assert!(!linhas.contains("whatsapp allow"), "{linhas}");
    assert!(
        !linhas.contains(NUMERO) && !linhas.contains("8888"),
        "{linhas}"
    );
}

/// O `status` continua saindo 0 com o portao vazio: scripts dependem do
/// exit code, que responde "ha vinculo utilizavel?".
#[test]
fn status_com_portao_vazio_nao_muda_o_exit_code() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, false);
    grava_config(&ctx, None, Some(serde_json::json!({})), Some(true));
    let store = ctx.store().expect("store");
    let key = ctx.key().expect("key");
    store
        .save(
            &garraia_channels::whatsapp_linked::SessionBlob::new("eyJhIjoxfQ=="),
            &key,
        )
        .expect("save");
    assert_eq!(run(Action::Status, &ctx, &ScriptedPrompter::default()), 0);
}

/// `restore` liga o canal; com ninguem autorizado, o estado que ele deixa e
/// o que dispara o aviso.
#[test]
fn restore_com_portao_vazio_deixa_o_estado_do_aviso() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    ctx.loader
        .as_ref()
        .expect("loader")
        .ensure_dirs()
        .expect("dirs");
    let store = ctx.store().expect("store");
    let key = ctx.key().expect("key");
    store
        .save(
            &garraia_channels::whatsapp_linked::SessionBlob::new("eyJhIjoxfQ=="),
            &key,
        )
        .expect("save");
    assert!(store.archive().expect("archive"));
    assert_eq!(run(Action::Restore, &ctx, &ScriptedPrompter::default()), 0);
    assert!(gate_is_empty(&ctx), "ligado e ninguem autorizado");
}

/// A varredura do `nenhuma_instrucao_fixa_o_nome_do_executavel`, estendida
/// ao modulo novo.
#[test]
fn o_modulo_de_acesso_nao_fixa_o_nome_do_executavel() {
    let corpo: String = include_str!("acesso.rs")
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    for proibido in [
        "`garra start`",
        "garra whatsapp",
        "garraia whatsapp",
        "`garraia start`",
    ] {
        assert!(
            !corpo.contains(proibido),
            "`{proibido}` literal em acesso.rs"
        );
    }
    assert!(corpo.contains("{bin} whatsapp allow"));
}

// --- ponta a ponta com a ponte falsa ----------------------------------------

/// A ponte falsa de `garraia-channels` no cenario `pair-ok`: QR, autentica,
/// conecta e entrega a sessao.
#[cfg(unix)]
struct PairOkLauncher;

#[cfg(unix)]
impl BridgeLauncher for PairOkLauncher {
    fn command(&self) -> Result<tokio::process::Command, BridgeError> {
        let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates/")
            .join("garraia-channels/tests/fixtures/fake_whatsapp_bridge.py");
        let mut cmd = tokio::process::Command::new("python3");
        cmd.arg(script)
            .arg("--scenario")
            .arg("pair-ok")
            .arg("--qr-expires")
            .arg("0.2")
            .arg("--handshake-timeout")
            .arg("5");
        Ok(cmd)
    }
    fn describe(&self) -> String {
        "python3 fake_whatsapp_bridge.py --scenario pair-ok".into()
    }
    fn dir(&self) -> std::path::PathBuf {
        std::env::temp_dir()
    }
}

/// **O defeito da #1345 pelo caminho inteiro do `link`.** QR lido, sessao
/// salva, canal ligado — e agora o numero perguntado vai para o `allow`.
#[cfg(unix)]
#[test]
fn o_link_de_verdade_pergunta_e_grava_quem_pode_falar() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    ctx.loader
        .as_ref()
        .expect("loader")
        .ensure_dirs()
        .expect("dirs");
    let store = ctx.store().expect("store");
    let key = ctx.key().expect("key");
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let p = ScriptedPrompter::default().with_inputs(&["+55 11 99999-8888"]);

    let code = link_paired(
        &ctx,
        &p,
        &Pedido::default(),
        &store,
        &key,
        &PairOkLauncher,
        &runtime,
        false,
    );
    assert_eq!(code, 0);
    assert!(store.exists(), "a sessao foi salva");
    let secao = secao_de(&ctx).expect("secao");
    assert_eq!(secao.enabled, Some(true), "o canal foi ligado");
    assert_eq!(lista(&ctx, "allow"), vec![NUMERO.to_string()]);
}

/// Upgrade pelo mesmo caminho: quem ja tinha `allow` nao perde nada, e o
/// default de "adicionar outro" e nao.
#[cfg(unix)]
#[test]
fn o_link_de_verdade_num_upgrade_preserva_o_allow() {
    let dir = tempfile::tempdir().expect("tempdir");
    let ctx = ctx_in(&dir, true);
    grava_config(
        &ctx,
        None,
        Some(serde_json::json!({ "allow": ["5511900000001"], "reply_in_groups": true })),
        Some(false),
    );
    let store = ctx.store().expect("store");
    let key = ctx.key().expect("key");
    let runtime = tokio::runtime::Runtime::new().expect("runtime");
    let p = ScriptedPrompter::default().with_inputs(&[ENTRADA]);

    let code = link_paired(
        &ctx,
        &p,
        &Pedido::default(),
        &store,
        &key,
        &PairOkLauncher,
        &runtime,
        false,
    );
    assert_eq!(code, 0);
    assert_eq!(lista(&ctx, "allow"), vec!["5511900000001".to_string()]);
    let secao = secao_de(&ctx).expect("secao");
    assert_eq!(secao.enabled, Some(true));
    assert_eq!(
        secao.settings.get("reply_in_groups"),
        Some(&serde_json::json!(true))
    );
}
