//! `garra whatsapp` — menu de duas opcoes, pareamento por QR e Cloud API.
//!
//! # A forma vem do Hermes; a substancia, nao
//!
//! O que se copia do `hermes whatsapp` e a **simplicidade**: um comando, um
//! menu curto, um QR, uma confirmacao. O que nao se copia:
//!
//! - **TTY obrigatorio.** O Hermes sai com erro fora de terminal. Aqui, sem
//!   TTY, imprimimos as duas opcoes com o comando explicito de cada uma e
//!   saimos 0 — a mesma postura do `garra init`, e a unica que sobrevive a um
//!   `curl … | sh`, a um Dockerfile e a um systemd.
//! - **Sessao em claro.** Ver [`garraia_channels::whatsapp_linked::session`].
//! - **A mensagem errada de allowlist vazia.** O bridge do Hermes e
//!   fail-closed (allowlist vazia = ninguem) mas o wizard avisa o contrario;
//!   a allowlist deste canal e do slice do gateway e nao e prometida aqui.
//!
//! O que se copia inteiro e a **ordem de gravacao**: `session.enc` primeiro,
//! `enabled = true` depois. Um wizard abortado que deixou `enabled = true` faz
//! o gateway pagar timeout e retry a cada boot. O teste
//! `config_is_untouched_when_pairing_fails` fixa isso.
//!
//! # Exit codes (sysexits, como `garra desktop` e `garra config check`)
//!
//! | Codigo | Quando |
//! |---|---|
//! | 0 | tudo certo, inclusive o caminho sem TTY |
//! | 1 | o usuario cancelou (Ctrl+C, resposta "nao") |
//! | 69 `EX_UNAVAILABLE` | falta Node/npm, o bridge nao sobe, ou nao ha sessao |
//! | 70 `EX_SOFTWARE` | erro interno (disco, config ilegivel) |

use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use garraia_channels::whatsapp_linked::bridge::{self, BridgeError, NodeLauncher, NodeRuntime};
use garraia_channels::whatsapp_linked::runner::{self, PairUi, RunError};
use garraia_channels::whatsapp_linked::{DEFAULT_ACCOUNT, KeyOrigin, SessionKey, SessionStore, qr};
use garraia_config::{ChannelConfig, ConfigLoader};

use crate::wizard::prompts::Prompter;

const EX_UNAVAILABLE: i32 = 69;
const EX_SOFTWARE: i32 = 70;
const EX_CANCELLED: i32 = 1;

/// Chave da secao de config deste canal.
const CONFIG_KEY: &str = garraia_channels::whatsapp_linked::CONFIG_KEY;
/// Chave do canal Cloud API, que ja existia.
const CLOUD_CONFIG_KEY: &str = "whatsapp";

/// O que `garra whatsapp` deve fazer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Sem subcomando: mostra o menu.
    Menu,
    Link,
    Cloud,
    Status,
    Logout,
}

// ---------------------------------------------------------------------------
// Idioma
// ---------------------------------------------------------------------------

/// Idioma da interface deste comando.
///
/// A CLI **nao tem** camada de i18n: cada comando escreve em pt-BR direto no
/// fonte. Introduzir uma para este slice seria refatoracao grande e nao pedida.
/// O meio-termo adotado: as frases deste comando — as unicas que um usuario
/// estrangeiro le antes de decidir se confia o WhatsApp dele ao GarraIA —
/// existem nas duas linguas, numa tabela unica que a documentacao espelha
/// (`docs/whatsapp.md` §Textos). O resto da CLI segue em pt-BR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Pt,
    En,
}

impl Lang {
    /// `GARRAIA_LANG` vence; depois `LC_ALL`, `LC_MESSAGES`, `LANG`.
    /// Qualquer coisa que nao comece com `en` e pt-BR — o default do projeto.
    pub fn detect() -> Self {
        for key in ["GARRAIA_LANG", "LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(value) = std::env::var(key) {
                let value = value.trim().to_ascii_lowercase();
                if value.is_empty() {
                    continue;
                }
                return if value.starts_with("en") {
                    Lang::En
                } else {
                    Lang::Pt
                };
            }
        }
        Lang::Pt
    }
}

/// Escolhe entre as duas versoes de uma frase.
fn t(lang: Lang, pt: &'static str, en: &'static str) -> &'static str {
    match lang {
        Lang::Pt => pt,
        Lang::En => en,
    }
}

/// Cabecalho do comando.
pub const HEADER: &str = "WhatsApp — GarraIA";

/// Texto da opcao 1, nas duas linguas.
pub const MENU_1_PT: &str = "Conectar meu WhatsApp pessoal (ler um QR code)";
pub const MENU_1_EN: &str = "Link my personal WhatsApp (scan a QR code)";
/// Texto da opcao 2, nas duas linguas.
pub const MENU_2_PT: &str = "Conectar um WhatsApp Business (API oficial da Meta)";
pub const MENU_2_EN: &str = "Connect a WhatsApp Business account (official Meta Cloud API)";

// ---------------------------------------------------------------------------
// Contexto injetavel
// ---------------------------------------------------------------------------

/// Tudo que o comando precisa do mundo, injetado para o teste nao depender de
/// `$HOME`, de `node` instalado, nem de um terminal.
pub struct Context {
    /// Diretorio de dados efetivo (`AppConfig::resolved_data_dir`).
    pub data_dir: PathBuf,
    /// `None` quando a config nao pode ser lida/escrita (o comando ainda
    /// funciona: `status` e `logout` nao precisam dela).
    pub loader: Option<ConfigLoader>,
    /// Passphrase do cofre, ja resolvida pela CLI.
    pub vault_passphrase: Option<String>,
    /// stdin e um terminal?
    pub interactive: bool,
    /// Largura do terminal, quando conhecida.
    pub columns: Option<u16>,
    /// O locale afirma UTF-8?
    pub unicode: bool,
    pub lang: Lang,
}

impl Context {
    /// Contexto a partir do ambiente real.
    pub fn from_env() -> Self {
        let loader = ConfigLoader::new().ok();
        let data_dir = loader
            .as_ref()
            .and_then(|l| l.load().ok())
            .map(|c| c.resolved_data_dir())
            .unwrap_or_else(|| ConfigLoader::default_config_dir().join("data"));
        Self {
            data_dir,
            loader,
            vault_passphrase: garraia_security::vault_passphrase_from_env(),
            interactive: std::io::stdin().is_terminal(),
            columns: console::Term::stdout().size_checked().map(|(_, cols)| cols),
            unicode: crate::ui::spinner::locale_supports_unicode(),
            lang: Lang::detect(),
        }
    }

    fn store(&self) -> SessionStore {
        SessionStore::for_data_dir(&self.data_dir, DEFAULT_ACCOUNT)
    }

    fn bridge_dir(&self) -> PathBuf {
        self.data_dir.join("whatsapp").join("bridge")
    }

    fn key(&self) -> Result<SessionKey, garraia_channels::whatsapp_linked::SessionError> {
        SessionKey::resolve(self.store().dir(), self.vault_passphrase.as_deref())
    }
}

// ---------------------------------------------------------------------------
// Entrada
// ---------------------------------------------------------------------------

/// Roda o comando. Devolve o exit code; nunca chama `std::process::exit`.
pub fn run(action: Action, ctx: &Context, prompter: &dyn Prompter) -> i32 {
    match action {
        Action::Status => status(ctx),
        Action::Logout => logout(ctx, prompter),
        Action::Menu => menu(ctx, prompter),
        Action::Link => link(ctx, prompter),
        Action::Cloud => cloud(ctx, prompter),
    }
}

fn print_header(ctx: &Context) {
    println!();
    println!("{HEADER}");
    println!("{}", "─".repeat(HEADER.chars().count()));
    let _ = ctx;
}

/// Texto impresso quando nao ha terminal. Publico para o teste de smoke
/// afirmar a frase exata sem duplicar o literal.
pub fn non_interactive_hint(lang: Lang) -> String {
    let mut out = String::new();
    out.push_str(t(
        lang,
        "Ambiente nao-interativo detectado. Escolha uma das duas opcoes e rode o comando:\n\n",
        "Non-interactive environment detected. Pick one of the two options and run the command:\n\n",
    ));
    out.push_str(&format!(
        "  1) {}\n     garra whatsapp link\n\n",
        t(lang, MENU_1_PT, MENU_1_EN)
    ));
    out.push_str(&format!(
        "  2) {}\n     garra whatsapp cloud\n\n",
        t(lang, MENU_2_PT, MENU_2_EN)
    ));
    out.push_str(t(
        lang,
        "Tambem existem: garra whatsapp status | garra whatsapp logout",
        "Also available: garra whatsapp status | garra whatsapp logout",
    ));
    out
}

fn menu(ctx: &Context, prompter: &dyn Prompter) -> i32 {
    print_header(ctx);
    if !ctx.interactive {
        println!("{}", non_interactive_hint(ctx.lang));
        return 0;
    }

    let options = [
        t(ctx.lang, MENU_1_PT, MENU_1_EN),
        t(ctx.lang, MENU_2_PT, MENU_2_EN),
    ];
    let prompt = t(
        ctx.lang,
        "Como voce quer usar o WhatsApp com o GarraIA?",
        "How do you want to use WhatsApp with GarraIA?",
    );
    match prompter.select(prompt, &options, 0) {
        Ok(0) => link(ctx, prompter),
        Ok(_) => cloud(ctx, prompter),
        Err(_) => {
            println!();
            println!("{}", t(ctx.lang, "Cancelado.", "Cancelled."));
            EX_CANCELLED
        }
    }
}

// ---------------------------------------------------------------------------
// status / logout — funcionam antes do ConfigLoader
// ---------------------------------------------------------------------------

fn status(ctx: &Context) -> i32 {
    let store = ctx.store();
    print_header(ctx);

    if !store.exists() {
        println!(
            "{}",
            t(
                ctx.lang,
                "Nenhum WhatsApp pessoal vinculado.",
                "No personal WhatsApp linked."
            )
        );
        println!(
            "{}",
            t(
                ctx.lang,
                "Para vincular: garra whatsapp",
                "To link one: garra whatsapp"
            )
        );
        return EX_UNAVAILABLE;
    }

    println!("{}", t(ctx.lang, "Vinculado: sim", "Linked:  yes"));
    println!(
        "{} {}",
        t(ctx.lang, "Sessao:", "Session:"),
        store.blob_path().display()
    );

    match ctx.key() {
        Ok(key) => {
            let origin = match key.origin() {
                KeyOrigin::VaultPassphrase => t(
                    ctx.lang,
                    "derivada da passphrase do cofre",
                    "derived from the vault passphrase",
                ),
                KeyOrigin::RandomKeyFile => t(
                    ctx.lang,
                    "arquivo local session.key",
                    "local session.key file",
                ),
            };
            println!("{} {origin}", t(ctx.lang, "Chave:", "Key:    "));

            // Prova real de que a sessao abre — `status` que so olha o nome do
            // arquivo mente quando a passphrase mudou.
            match store.load(&key) {
                Ok(_) => println!("{}", t(ctx.lang, "Leitura:  ok", "Readable: yes")),
                Err(e) => {
                    println!(
                        "{} {e}",
                        t(ctx.lang, "Leitura:  FALHOU —", "Readable: NO —")
                    );
                    println!(
                        "{}",
                        t(
                            ctx.lang,
                            "Rode `garra whatsapp` para vincular de novo.",
                            "Run `garra whatsapp` to link again."
                        )
                    );
                    return EX_UNAVAILABLE;
                }
            }

            if let Some(warning) = key.origin().warning() {
                println!();
                println!("⚠ {warning}");
            }
        }
        Err(e) => {
            eprintln!("{e}");
            return EX_SOFTWARE;
        }
    }

    println!();
    println!(
        "{}",
        t(
            ctx.lang,
            "O gateway ainda nao consome este canal (chega no proximo slice).",
            "The gateway does not consume this channel yet (next slice)."
        )
    );
    0
}

fn logout(ctx: &Context, prompter: &dyn Prompter) -> i32 {
    let store = ctx.store();
    print_header(ctx);

    if !store.exists() {
        println!(
            "{}",
            t(ctx.lang, "Nada para desvincular.", "Nothing to unlink.")
        );
        return 0;
    }

    if ctx.interactive {
        let prompt = t(
            ctx.lang,
            "Desvincular e apagar a sessao deste aparelho?",
            "Unlink and delete this device's session?",
        );
        match prompter.confirm(prompt, false) {
            Ok(true) => {}
            Ok(false) | Err(_) => {
                println!("{}", t(ctx.lang, "Cancelado.", "Cancelled."));
                return EX_CANCELLED;
            }
        }
    }

    if let Err(e) = store.purge() {
        eprintln!("{e}");
        return EX_SOFTWARE;
    }
    println!("✓ {}", t(ctx.lang, "Sessao apagada.", "Session deleted."));

    // `enabled = false` DEPOIS da remocao, espelhando a ordem do link.
    match disable_channel(ctx) {
        Ok(true) => println!(
            "✓ {}",
            t(
                ctx.lang,
                "Canal desligado na config.",
                "Channel disabled in the config."
            )
        ),
        Ok(false) => {}
        Err(e) => {
            eprintln!("{e}");
            return EX_SOFTWARE;
        }
    }
    println!();
    println!(
        "{}",
        t(
            ctx.lang,
            "O aparelho continua listado no celular ate voce remove-lo em \
Configuracoes → Aparelhos conectados.",
            "The device stays listed on your phone until you remove it under \
Settings → Linked devices.",
        )
    );
    0
}

// ---------------------------------------------------------------------------
// link
// ---------------------------------------------------------------------------

fn link(ctx: &Context, prompter: &dyn Prompter) -> i32 {
    if !ctx.interactive {
        print_header(ctx);
        println!("{}", non_interactive_hint(ctx.lang));
        return 0;
    }

    let store = ctx.store();
    let key = match ctx.key() {
        Ok(k) => k,
        Err(e) => {
            eprintln!("{e}");
            return EX_SOFTWARE;
        }
    };

    // Sessao existente: pergunta antes de qualquer coisa destrutiva.
    let mut had_session = store.exists();
    if had_session {
        println!(
            "{}",
            t(
                ctx.lang,
                "Ja existe uma sessao vinculada neste aparelho.",
                "This machine already has a linked session."
            )
        );
        let prompt = t(
            ctx.lang,
            "Re-vincular? Isso apaga a sessao atual",
            "Re-link? This deletes the current session",
        );
        match prompter.confirm(prompt, false) {
            Ok(true) => {
                if let Err(e) = store.archive() {
                    eprintln!("{e}");
                    return EX_SOFTWARE;
                }
                had_session = false;
            }
            Ok(false) => {
                // Nao apaga nada: segue com a sessao atual, que e o caminho
                // idempotente (`session_found → validating → connected`).
            }
            Err(_) => {
                println!("{}", t(ctx.lang, "Cancelado.", "Cancelled."));
                return EX_CANCELLED;
            }
        }
    }

    // Consentimento: so quando vamos de fato mostrar um QR.
    if !had_session && !consent(ctx, prompter) {
        println!("{}", t(ctx.lang, "Cancelado.", "Cancelled."));
        return EX_CANCELLED;
    }

    let node = match NodeRuntime::detect() {
        Ok(n) => n,
        Err(e) => {
            print_missing_node(ctx, &e);
            return EX_UNAVAILABLE;
        }
    };

    let bridge_dir = ctx.bridge_dir();
    if let Err(e) = bridge::materialize(&bridge_dir, &bridge::EmbeddedAssets) {
        eprintln!("{e}");
        return EX_SOFTWARE;
    }

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("{e}");
            return EX_SOFTWARE;
        }
    };

    if !bridge::deps_installed(&bridge_dir) {
        println!(
            "→ {}",
            t(
                ctx.lang,
                "Instalando as dependencias do bridge (pode levar alguns minutos)…",
                "Installing bridge dependencies (this can take a few minutes)…"
            )
        );
        if let Err(e) = runtime.block_on(bridge::npm_install(&node.npm, &bridge_dir)) {
            eprintln!();
            eprintln!("{e}");
            return EX_UNAVAILABLE;
        }
        println!(
            "✓ {}",
            t(ctx.lang, "Dependencias prontas.", "Dependencies ready.")
        );
    }

    print_instructions(ctx);

    let launcher = NodeLauncher::new(&node.node, &bridge_dir);
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    let mut ui = TerminalUi::new(ctx);

    let outcome = runtime.block_on(async {
        // Ctrl+C vira um bit no canal: o driver manda `shutdown` ao bridge e
        // sai limpo, em vez de o processo morrer deixando um `node` orfao.
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                let _ = cancel_tx.send(true);
            }
        });
        runner::pair(&launcher, &store, &key, &mut ui, cancel_rx).await
    });

    match outcome {
        Ok(outcome) => {
            println!();
            if outcome.reused_existing_session {
                println!(
                    "✓ {}",
                    t(
                        ctx.lang,
                        "Sessao encontrada e valida",
                        "Session found and valid"
                    )
                );
            } else {
                println!(
                    "✓ {}",
                    t(
                        ctx.lang,
                        "WhatsApp conectado com sucesso.",
                        "WhatsApp connected successfully."
                    )
                );
            }
            println!(
                "✓ {} {}.",
                t(ctx.lang, "Sessao salva em", "Session saved in"),
                store.dir().display()
            );

            // ORDEM: o blob ja esta em disco (o runner gravou). So agora a
            // config aprende que o canal existe.
            match enable_channel(ctx) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!("{e}");
                    return EX_SOFTWARE;
                }
            }
            println!(
                "✓ {}",
                t(
                    ctx.lang,
                    "GarraIA esta pronto para receber mensagens (inicie o gateway: `garra start`)",
                    "GarraIA is ready to receive messages (start the gateway: `garra start`)"
                )
            );
            0
        }
        Err(RunError::Cancelled) => {
            println!();
            println!("{}", t(ctx.lang, "Cancelado.", "Cancelled."));
            EX_CANCELLED
        }
        Err(RunError::QrExpired) => {
            println!();
            println!(
                "{}",
                t(
                    ctx.lang,
                    "Nenhum QR foi lido. Rode `garra whatsapp` de novo.",
                    "No QR was scanned. Run `garra whatsapp` again."
                )
            );
            EX_UNAVAILABLE
        }
        Err(RunError::LoggedOut) => {
            println!();
            println!(
                "{}",
                t(
                    ctx.lang,
                    "A conta foi desvinculada no aparelho. Rode `garra whatsapp` de novo.",
                    "The account was unlinked on the phone. Run `garra whatsapp` again."
                )
            );
            let _ = disable_channel(ctx);
            EX_UNAVAILABLE
        }
        Err(e) => {
            eprintln!();
            eprintln!("{e}");
            EX_UNAVAILABLE
        }
    }
}

/// Tela de consentimento. `false` = o usuario recusou ou cancelou.
fn consent(ctx: &Context, prompter: &dyn Prompter) -> bool {
    println!();
    println!(
        "{}",
        t(
            ctx.lang,
            "Antes de continuar, leia:",
            "Before you continue, please read:"
        )
    );
    println!();
    for line in consent_body(ctx.lang) {
        println!("  {line}");
    }
    println!();
    let prompt = t(
        ctx.lang,
        "Entendi os riscos e quero continuar",
        "I understand the risks and want to continue",
    );
    // Default **No**: quem apertar Enter sem ler nao vincula a conta.
    prompter.confirm(prompt, false).unwrap_or(false)
}

/// Corpo da tela de consentimento. Publico para a doc e o teste espelharem o
/// texto exato em vez de reescreve-lo.
pub fn consent_body(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::Pt => &[
            "Conectar pelo QR usa o recurso de \"aparelho conectado\" do WhatsApp",
            "por um cliente NAO oficial. Isso contraria os termos de uso da Meta.",
            "",
            "O risco real: a conta pode ser bloqueada, temporaria ou",
            "permanentemente. Nao existe recurso garantido.",
            "",
            "Recomendacao: use um numero secundario, e nao o seu numero",
            "principal. Um chip pre-pago ou um numero virtual resolve.",
            "",
            "Se voce precisa de suporte oficial, escolha a opcao 2 do menu",
            "(WhatsApp Business / Cloud API da Meta).",
        ],
        Lang::En => &[
            "Linking by QR uses WhatsApp's \"linked device\" feature through an",
            "UNOFFICIAL client. This goes against Meta's terms of service.",
            "",
            "The real risk: the account can be blocked, temporarily or",
            "permanently. There is no guaranteed appeal.",
            "",
            "Recommendation: use a secondary number, not your main one. A",
            "prepaid SIM or a virtual number is enough.",
            "",
            "If you need official support, pick option 2 in the menu",
            "(WhatsApp Business / Meta Cloud API).",
        ],
    }
}

fn print_instructions(ctx: &Context) {
    println!();
    for line in instructions(ctx.lang) {
        println!("{line}");
    }
    println!();
}

/// Bloco de instrucoes de leitura do QR.
pub fn instructions(lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::Pt => &[
            "Abra o WhatsApp no celular:",
            "  Configuracoes → Aparelhos conectados → Conectar um aparelho",
        ],
        Lang::En => &[
            "Open WhatsApp on your phone:",
            "  Settings → Linked devices → Link a device",
        ],
    }
}

fn print_missing_node(ctx: &Context, err: &BridgeError) {
    eprintln!();
    eprintln!("{err}");
    eprintln!();
    eprintln!(
        "{}",
        t(
            ctx.lang,
            "Vincular o WhatsApp pessoal precisa do Node.js 20 ou mais novo \
(so este caminho precisa; o resto do GarraIA nao).",
            "Linking a personal WhatsApp needs Node.js 20 or newer (only this \
path does; the rest of GarraIA does not)."
        )
    );
    eprintln!("  https://nodejs.org/en/download");
    eprintln!();
    eprintln!(
        "{}",
        t(
            ctx.lang,
            "Sem Node, a opcao 2 (WhatsApp Business / Cloud API) funciona: \
garra whatsapp cloud",
            "Without Node, option 2 (WhatsApp Business / Cloud API) works: \
garra whatsapp cloud"
        )
    );
}

// ---------------------------------------------------------------------------
// UI do pareamento
// ---------------------------------------------------------------------------

/// Implementacao de [`PairUi`] que escreve no terminal.
///
/// Regras herdadas do `ui/spinner.rs`: nunca esconde o cursor, nunca emite cor
/// e escreve num sink `io::Write` — o que permite ao teste afirmar a saida sem
/// capturar o stdout do processo.
struct TerminalUi<'a> {
    ctx: &'a Context,
    style: qr::Style,
    /// Ultimo segundo ja impresso, para o contador nao repetir a mesma linha.
    last_countdown: Option<u64>,
    sink: Box<dyn Write + Send>,
}

impl<'a> TerminalUi<'a> {
    fn new(ctx: &'a Context) -> Self {
        Self {
            ctx,
            style: qr::Style::for_terminal(ctx.unicode, ctx.interactive, ctx.columns),
            last_countdown: None,
            sink: Box::new(std::io::stdout()),
        }
    }

    fn say(&mut self, line: &str) {
        let _ = writeln!(self.sink, "{line}");
        let _ = self.sink.flush();
    }
}

impl PairUi for TerminalUi<'_> {
    fn status(&mut self, line: &str) {
        self.say(&format!("→ {line}"));
    }

    fn qr(&mut self, data: &str, attempt: u32, max: u32, previous_expired: bool) {
        self.last_countdown = None;
        if previous_expired {
            // Separador em vez de redesenho: limpar a tela apagaria as
            // instrucoes que o usuario ainda esta lendo.
            self.say("");
            self.say(&"─".repeat(40));
            let msg = match self.ctx.lang {
                Lang::Pt => format!("QR anterior expirou — novo QR (tentativa {attempt}/{max})"),
                Lang::En => format!("Previous QR expired — new QR (attempt {attempt}/{max})"),
            };
            self.say(&msg);
            self.say("");
        }
        match qr::render(data, self.style) {
            Ok(drawing) => {
                self.say(&drawing);
                if self.style == qr::Style::Raw {
                    self.say(t(self.ctx.lang, qr::RAW_HINT_PT, qr::RAW_HINT_EN));
                }
            }
            Err(e) => {
                // Falhar em desenhar nao pode matar o pareamento: a string
                // crua ainda serve.
                self.say(&format!("{e}"));
                self.say(data);
            }
        }
    }

    fn waiting(&mut self, attempt: u32, max: u32, seconds_left: u64) {
        if self.last_countdown == Some(seconds_left) {
            return;
        }
        self.last_countdown = Some(seconds_left);
        let line = match self.ctx.lang {
            Lang::Pt => {
                format!("   aguardando leitura… (tentativa {attempt}/{max}, {seconds_left}s)")
            }
            Lang::En => {
                format!("   waiting for the scan… (attempt {attempt}/{max}, {seconds_left}s)")
            }
        };
        self.say(&line);
    }

    fn authenticated(&mut self) {
        self.say("");
        self.say(t(
            self.ctx.lang,
            "✓ Autenticado. Sincronizando sessao…",
            "✓ Authenticated. Syncing the session…",
        ));
    }
}

// ---------------------------------------------------------------------------
// cloud — wizard da Cloud API
// ---------------------------------------------------------------------------

fn cloud(ctx: &Context, prompter: &dyn Prompter) -> i32 {
    if !ctx.interactive {
        print_header(ctx);
        println!("{}", non_interactive_hint(ctx.lang));
        return 0;
    }
    let Some(loader) = ctx.loader.as_ref() else {
        eprintln!(
            "{}",
            t(
                ctx.lang,
                "Nao consegui abrir a config. Rode `garra init` primeiro.",
                "Could not open the config. Run `garra init` first."
            )
        );
        return EX_SOFTWARE;
    };

    println!();
    println!(
        "{}",
        t(
            ctx.lang,
            "WhatsApp Business (Cloud API da Meta). Os quatro valores abaixo saem \
do painel em developers.facebook.com → seu app → WhatsApp.",
            "WhatsApp Business (Meta Cloud API). The four values below come from \
developers.facebook.com → your app → WhatsApp."
        )
    );
    println!();

    // `password` e nao `input` para o token nao ficar no scrollback nem no
    // historico do terminal.
    let access_token = match prompter.password(t(ctx.lang, "Access token", "Access token"), None) {
        Ok(v) => v,
        Err(_) => return EX_CANCELLED,
    };
    let phone_number_id =
        match prompter.input(t(ctx.lang, "Phone number ID", "Phone number ID"), "") {
            Ok(v) => v,
            Err(_) => return EX_CANCELLED,
        };
    let verify_token = match prompter.password(
        t(
            ctx.lang,
            "Verify token (o que voce escolheu no webhook)",
            "Verify token (the one you chose for the webhook)",
        ),
        None,
    ) {
        Ok(v) => v,
        Err(_) => return EX_CANCELLED,
    };
    let app_secret = match prompter.password(t(ctx.lang, "App secret", "App secret"), None) {
        Ok(v) => v,
        Err(_) => return EX_CANCELLED,
    };

    let missing: Vec<&str> = [
        ("access_token", &access_token),
        ("phone_number_id", &phone_number_id),
        ("verify_token", &verify_token),
        ("app_secret", &app_secret),
    ]
    .into_iter()
    .filter(|(_, v)| v.trim().is_empty())
    .map(|(k, _)| k)
    .collect();
    if !missing.is_empty() {
        eprintln!();
        eprintln!(
            "{} {}",
            t(ctx.lang, "Faltou preencher:", "Still missing:"),
            missing.join(", ")
        );
        return EX_CANCELLED;
    }

    if let Err(e) = write_cloud_channel(
        loader,
        &access_token,
        &phone_number_id,
        &verify_token,
        &app_secret,
    ) {
        eprintln!("{e}");
        return EX_SOFTWARE;
    }

    println!();
    // NUNCA reimprimir os valores — nem truncados. Um token parcial no
    // scrollback continua sendo material para quem le o terminal por cima do
    // ombro, e o `RedactingWriter` nao alcanca `println!`.
    println!(
        "✓ {}",
        t(
            ctx.lang,
            "Canal whatsapp (Cloud API) gravado na config.",
            "Channel whatsapp (Cloud API) written to the config."
        )
    );
    println!(
        "  {}",
        t(
            ctx.lang,
            "Os segredos ficam so no arquivo (modo 0600) — nao sao exibidos aqui.",
            "The secrets live only in the file (mode 0600) — they are never echoed."
        )
    );
    println!();
    println!(
        "{}",
        t(
            ctx.lang,
            "Falta apontar o webhook da Meta para:",
            "Point the Meta webhook at:"
        )
    );
    println!("  https://<seu-dominio>/webhooks/whatsapp");
    0
}

// ---------------------------------------------------------------------------
// Escrita de config
// ---------------------------------------------------------------------------

/// Grava `channels.whatsapp_linked.enabled = true`.
///
/// So e chamada DEPOIS de `session.enc` existir. Ver o topo do modulo.
fn enable_channel(ctx: &Context) -> Result<()> {
    let Some(loader) = ctx.loader.as_ref() else {
        return Ok(()); // sem config, nada a gravar: o link em si ja valeu
    };
    set_linked_enabled(loader, true).map(|_| ())
}

/// Grava `enabled = false`. Devolve `false` quando nao havia o que desligar.
fn disable_channel(ctx: &Context) -> Result<bool> {
    let Some(loader) = ctx.loader.as_ref() else {
        return Ok(false);
    };
    set_linked_enabled(loader, false)
}

/// Nucleo testavel: recebe o loader, mexe so em `channels.whatsapp_linked`.
pub fn set_linked_enabled(loader: &ConfigLoader, enabled: bool) -> Result<bool> {
    // `save` escreve `<config_dir>/config.yml`; numa maquina que nunca rodou
    // `garra init` o diretorio ainda nao existe.
    loader.ensure_dirs()?;
    let mut config = loader.load()?;
    match config.channels.get_mut(CONFIG_KEY) {
        Some(existing) => {
            if existing.enabled == Some(enabled) {
                return Ok(false);
            }
            existing.enabled = Some(enabled);
        }
        None => {
            if !enabled {
                // Nao criamos a secao so para escrever `false`.
                return Ok(false);
            }
            config.channels.insert(
                CONFIG_KEY.to_string(),
                ChannelConfig {
                    channel_type: CONFIG_KEY.to_string(),
                    enabled: Some(true),
                    settings: Default::default(),
                },
            );
        }
    }
    loader.save(&config)?;
    Ok(true)
}

/// Grava `channels.whatsapp` (Cloud API) com os quatro segredos.
pub fn write_cloud_channel(
    loader: &ConfigLoader,
    access_token: &str,
    phone_number_id: &str,
    verify_token: &str,
    app_secret: &str,
) -> Result<()> {
    loader.ensure_dirs()?;
    let mut config = loader.load()?;
    let mut settings = std::collections::HashMap::new();
    for (key, value) in [
        ("access_token", access_token),
        ("phone_number_id", phone_number_id),
        ("verify_token", verify_token),
        ("app_secret", app_secret),
    ] {
        settings.insert(
            key.to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }
    config.channels.insert(
        CLOUD_CONFIG_KEY.to_string(),
        ChannelConfig {
            channel_type: CLOUD_CONFIG_KEY.to_string(),
            enabled: Some(true),
            settings,
        },
    );
    // `ConfigLoader::save` ja aperta o arquivo para 0600 — o que importa
    // porque acabamos de escrever quatro segredos nele.
    loader.save(&config)?;
    Ok(())
}

/// Caminho do diretorio de sessao, para mensagem e teste.
pub fn session_dir(data_dir: &Path) -> PathBuf {
    SessionStore::for_data_dir(data_dir, DEFAULT_ACCOUNT)
        .dir()
        .to_path_buf()
}

#[cfg(test)]
mod tests;
