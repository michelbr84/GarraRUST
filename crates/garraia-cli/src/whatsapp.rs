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
//! | 69 `EX_UNAVAILABLE` | falta Node/npm, o bridge nao sobe, nao ha sessao, ou `link`/`cloud` foram chamados sem terminal |
//! | 70 `EX_SOFTWARE` | erro interno (disco, config ilegivel) |

use std::io::{IsTerminal, Write};
use std::path::PathBuf;

use anyhow::Result;
use garraia_channels::whatsapp_linked::bridge::{
    self, BridgeError, BridgeLauncher, NodeLauncher, NodeRuntime,
};
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
    /// Traz de volta a sessao que ficou em `session.enc.prev`.
    Restore,
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

    /// Store da conta default.
    ///
    /// `for_data_dir` devolve `Result` porque recusa conta que nao seja um
    /// unico segmento `[A-Za-z0-9_-]{1,64}` — e a CLI passa `DEFAULT_ACCOUNT`,
    /// constante de compilacao que a regra aceita, entao o `Err` daqui e
    /// inalcancavel hoje. Ele e propagado assim mesmo: `unwrap()` em codigo de
    /// producao e proibido, e o dia em que a CLI aprender a escolher conta e
    /// exatamente o dia em que este erro passa a valer.
    fn store(&self) -> Result<SessionStore, garraia_channels::whatsapp_linked::SessionError> {
        SessionStore::for_data_dir(&self.data_dir, DEFAULT_ACCOUNT)
    }

    fn bridge_dir(&self) -> PathBuf {
        self.data_dir.join("whatsapp").join("bridge")
    }

    fn key(&self) -> Result<SessionKey, garraia_channels::whatsapp_linked::SessionError> {
        SessionKey::resolve(self.store()?.dir(), self.vault_passphrase.as_deref())
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
        Action::Restore => restore(ctx),
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
        "Ambiente não-interativo detectado. Escolha uma das duas opções e rode o comando:\n\n",
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
        "Também existem: garra whatsapp status | garra whatsapp restore | garra whatsapp logout",
        "Also available: garra whatsapp status | garra whatsapp restore | garra whatsapp logout",
    ));
    out
}

/// O que `link` e `cloud` dizem quando nao ha terminal.
///
/// # Por que NAO e o [`non_interactive_hint`]
///
/// O hint do menu termina mandando rodar `garra whatsapp link`. Quando o
/// proprio `link` respondia com esse mesmo texto — byte a byte, e saindo 0 —,
/// quem estava num pipe recebia como orientacao a repeticao do comando que
/// acabara de rodar. `ssh servidor 'garra whatsapp link'`, que e como se
/// conecta um GarraIA headless num VPS, nao tem TTY: o usuario ficava num
/// ciclo fechado, sem QR, sem erro e com exit 0 dizendo que deu certo.
///
/// Quem JA escolheu o fluxo precisa de outra coisa: o motivo (o QR se le
/// deste terminal, e o consentimento se da nele) e a saida (`ssh -t`). E de
/// um exit code que nao minta — 69 `EX_UNAVAILABLE`, o mesmo que o `status`
/// usa para "nao da para fazer isto aqui", e nao 0.
pub fn needs_a_terminal(lang: Lang, subcomando: &str) -> String {
    let mut out = String::new();
    out.push_str(t(
        lang,
        "Este fluxo precisa de um terminal de verdade.\n\n",
        "This flow needs a real terminal.\n\n",
    ));
    out.push_str(t(
        lang,
        "O QR code é desenhado neste terminal e você confirma o vínculo aqui, \
         então um pipe, um cron ou um `ssh` sem TTY não conseguem levar o \
         processo ate o fim.\n\n",
        "The QR code is drawn in this terminal and you confirm the link here, \
         so a pipe, a cron job or an `ssh` without a TTY cannot carry the \
         process through.\n\n",
    ));
    out.push_str(&format!(
        "  {}\n    ssh -t <usuario>@<maquina> garra whatsapp {subcomando}\n",
        t(
            lang,
            "Por ssh, peça um TTY com -t:",
            "Over ssh, ask for a TTY with -t:"
        )
    ));
    out.push_str(t(
        lang,
        "\nNum multiplexador (tmux, screen) ou num terminal local, basta rodar \
         o comando normalmente.",
        "\nInside a multiplexer (tmux, screen) or in a local terminal, just run \
         the command as usual.",
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
        "Como você quer usar o WhatsApp com o GarraIA?",
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
    let store = match ctx.store() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return EX_SOFTWARE;
        }
    };
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
        print_archive_warning(ctx, &store);
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
        t(ctx.lang, "Sessão:", "Session:"),
        store.blob_path().display()
    );
    print_archive_warning(ctx, &store);

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
            "O gateway ainda não consome este canal (chega no próximo slice).",
            "The gateway does not consume this channel yet (next slice)."
        )
    );
    0
}

/// Avisa sobre `session.enc.prev`, quando existir.
///
/// **F1 da auditoria R4.** O arquivado e uma credencial **viva**: quem o tem
/// fala como o usuario. Ele nasce quando alguem responde "sim" ao re-vincular
/// e sobrevive a qualquer pareamento que nao conclua — QR expirado, Ctrl+C,
/// Node ausente. Um `status` que nao o mostra deixa o estado invisivel, e foi
/// isso que permitiu o `logout` recusar o trabalho.
fn print_archive_warning(ctx: &Context, store: &SessionStore) {
    if !store.archive_path().is_file() {
        return;
    }
    println!();
    println!(
        "⚠ {}",
        t(
            ctx.lang,
            "Ha uma sessão ARQUIVADA neste aparelho, de um re-vínculo que não terminou:",
            "There is an ARCHIVED session on this machine, from a re-link that never finished:"
        )
    );
    println!("  {}", store.archive_path().display());
    println!(
        "  {}",
        t(
            ctx.lang,
            "Ela ainda é uma credencial válida.",
            "It is still a valid credential."
        )
    );
    // Ate a revisao R4 este aviso so mandava APAGAR. `restore_archive()` ja
    // existia e nenhuma superficie a expunha: a funcao que salva o usuario
    // estava escrita e o botao nao existia. Quem chegou aqui por um
    // re-vinculo que nao terminou quer, quase sempre, a sessao de volta — e
    // apagar e a unica das duas escolhas que nao tem volta, entao ela vem
    // depois.
    if store.exists() {
        // Ha sessao viva: restaurar por cima nao e o que `restore_archive`
        // faz, e prometer isso seria mentira. Ver o docstring dela.
        println!(
            "  {}",
            t(
                ctx.lang,
                "Há uma sessão em uso, então ela não será substituída. Para descartar a arquivada: garra whatsapp logout",
                "A session is in use, so it will not be replaced. To discard the archived one: garra whatsapp logout"
            )
        );
        return;
    }
    println!(
        "  {}",
        t(
            ctx.lang,
            "Para voltar a usá-la: garra whatsapp restore",
            "To use it again: garra whatsapp restore"
        )
    );
    println!(
        "  {}",
        t(
            ctx.lang,
            "Para apagá-la: garra whatsapp logout",
            "To delete it: garra whatsapp logout"
        )
    );
}

/// `garra whatsapp restore` — devolve o `session.enc.prev` ao lugar.
///
/// # Por que este comando existe
///
/// Porque [`SessionStore::restore_archive`] existia desde a revisao anterior e
/// **nenhuma superficie a expunha**: a funcao que recupera o vinculo estava
/// escrita, testada, e o usuario nao tinha como chama-la. O `status` via o
/// arquivado e mandava apaga-lo.
///
/// O caminho que produz um arquivado orfao foi fechado nesta mesma rodada (o
/// handshake sem prazo do `runner`, que obrigava a SIGKILL e pulava o `Drop`
/// do `ArchiveGuard`), mas fechar a porta nao devolve a sessao de quem ja
/// passou por ela. Este comando devolve.
///
/// Ele **nao** sobrescreve uma sessao viva: quem decide isso e
/// `restore_archive`, que recusa quando ha `session.enc` — e recusar e o
/// certo, porque a viva e a que o servidor conhece. Nesse caso o comando diz
/// o que ha e sai 69, sem apagar nada.
fn restore(ctx: &Context) -> i32 {
    let store = match ctx.store() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return EX_SOFTWARE;
        }
    };
    print_header(ctx);

    if !store.archive_path().is_file() {
        println!(
            "{}",
            t(
                ctx.lang,
                "Não há sessão arquivada para restaurar.",
                "There is no archived session to restore."
            )
        );
        return EX_UNAVAILABLE;
    }
    if store.exists() {
        println!(
            "{}",
            t(
                ctx.lang,
                "Já há uma sessão em uso — a arquivada não pode substituí-la.",
                "A session is already in use — the archived one cannot replace it."
            )
        );
        println!(
            "{}",
            t(
                ctx.lang,
                "Se quiser descartar a arquivada: garra whatsapp logout",
                "To discard the archived one: garra whatsapp logout"
            )
        );
        return EX_UNAVAILABLE;
    }

    // **A PROVA VEM ANTES DO MOVIMENTO.** Ela ja existia, mas rodava depois
    // do `restore_archive()`, e entao uma passphrase do cofre apenas AUSENTE
    // do ambiente consumia o `.prev`: o comando saia 69, o arquivo ja tinha
    // saido do lugar, e a mensagem mandava ler um QR novo — conselho que
    // descartaria uma sessao intacta. O blob sobrevivia em `session.enc`, mas
    // nao sobrava comando que ligasse o canal: um segundo `restore`, ja com a
    // passphrase, respondia "nao ha arquivada".
    //
    // Provar primeiro custa a mesma leitura e devolve o erro com o arquivo
    // ainda no lugar. Mesmo principio do relogio de progresso do `runner.rs`:
    // nao destrua estado antes de saber que pode.
    if let Err(e) = ctx.key().and_then(|key| store.load_archive(&key)) {
        eprintln!("{e}");
        eprintln!(
            "{}",
            t(
                ctx.lang,
                "A sessão arquivada não abre com a chave atual — ela NÃO foi movida e continua onde está. Se a senha do cofre estava só faltando no ambiente, exporte-a e rode de novo.",
                "The archived session does not open with the current key — it was NOT moved and is still in place. If the vault passphrase was merely missing from the environment, export it and run again."
            )
        );
        return EX_UNAVAILABLE;
    }

    match store.restore_archive() {
        Ok(true) => {}
        // Inalcancavel depois dos dois guards acima, mas `restore_archive` e o
        // dono da regra e nao este comando: se ela recusar por um motivo que
        // ainda nao existe, dizer "restaurado" seria mentira.
        Ok(false) => {
            eprintln!(
                "{}",
                t(
                    ctx.lang,
                    "A sessão arquivada não pôde ser restaurada.",
                    "The archived session could not be restored."
                )
            );
            return EX_UNAVAILABLE;
        }
        Err(e) => {
            eprintln!("{e}");
            return EX_SOFTWARE;
        }
    }
    println!(
        "✓ {}",
        t(ctx.lang, "Sessão restaurada.", "Session restored.")
    );

    // ORDEM: o blob primeiro, o `enabled = true` depois — a mesma do `link`,
    // pelo mesmo motivo (um `enabled` sem sessao faz o gateway pagar timeout e
    // retry a cada boot).
    if let Err(e) = enable_channel(ctx) {
        eprintln!("{e}");
        return EX_SOFTWARE;
    }
    println!(
        "{}",
        t(
            ctx.lang,
            "Ela só volta a valer se o WhatsApp ainda aceitar este aparelho — rode `garra whatsapp status` e, se não aceitar, `garra whatsapp` para ler um QR novo.",
            "It only works again if WhatsApp still accepts this device — run `garra whatsapp status`, and if it does not, run `garra whatsapp` to scan a new QR."
        )
    );
    0
}

fn logout(ctx: &Context, prompter: &dyn Prompter) -> i32 {
    let store = match ctx.store() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return EX_SOFTWARE;
        }
    };
    print_header(ctx);

    // F1: `exists()` olha so o `session.enc`. O arquivado e igualmente uma
    // credencial viva, entao "nao ha nada" so e verdade quando os DOIS
    // sumiram — senao o comando que existe para limpar recusa o trabalho e
    // ainda responde 0, afirmando que limpou.
    let archived = store.archive_path().is_file();
    if !store.exists() && !archived {
        println!(
            "{}",
            t(ctx.lang, "Nada para desvincular.", "Nothing to unlink.")
        );
        return 0;
    }

    if ctx.interactive {
        let prompt = if store.exists() {
            t(
                ctx.lang,
                "Desvincular e apagar a sessão deste aparelho?",
                "Unlink and delete this device's session?",
            )
        } else {
            // So sobrou o arquivado: dizer "desvincular" seria impreciso.
            t(
                ctx.lang,
                "Apagar a sessão arquivada deste aparelho?",
                "Delete the archived session on this machine?",
            )
        };
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
    println!("✓ {}", t(ctx.lang, "Sessão apagada.", "Session deleted."));

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
            "O aparelho continua listado no celular até você removê-lo em \
Configurações → Aparelhos conectados.",
            "The device stays listed on your phone until you remove it under \
Settings → Linked devices.",
        )
    );
    0
}

// ---------------------------------------------------------------------------
// link
// ---------------------------------------------------------------------------

/// Arquiva a sessao atual e a traz de volta se o link nao gravar uma nova.
///
/// # Por que um guard, e nao duas chamadas nos bracos que falham
///
/// Porque os bracos que falham nao sao dois. Entre o arquivamento e o blob
/// novo cabem o Ctrl+C na tela do QR, os cinco QRs expirando, o bridge que
/// morre antes de conectar, o que conecta e nunca entrega a sessao, e o
/// proximo que alguem acrescentar. Listar os bracos e escolher quais
/// restauram; um guard restaura em todos, porque restaurar e o que acontece
/// quando **nao** se fez nada melhor.
///
/// O caminho de sucesso nao precisa de `commit()`: quando o `pair` grava o
/// blob novo ele mesmo chama `discard_archive()`, entao na hora do `drop` nao
/// ha mais arquivado — e [`SessionStore::restore_archive`] tambem se recusa a
/// passar por cima de um `session.enc` vivo. Nos dois sentidos, o guard vira
/// no-op silencioso exatamente quando deve.
///
/// # O braco que nao pode ser silencioso
///
/// `restore_archive` devolvendo `false` tem tres causas, e so duas sao o
/// caminho feliz: nao havia nada arquivado, ou ha sessao nova em disco. A
/// terceira e o arquivado ter SUMIDO entre o `archive()` e o `drop` — e ai o
/// usuario perdeu o vinculo anterior e precisa ouvir isso. Por isso o guard
/// guarda `archived`: sem esse bit os tres casos tem a mesma cara.
///
/// # Por que a saida e injetada
///
/// Porque `Drop` nao devolve valor, nao propaga erro e nao aparece em nenhuma
/// assinatura: com `println!`/`eprintln!` direto, a mensagem de MAIOR
/// consequencia deste arquivo — "o vinculo antigo foi perdido" — nao tinha
/// como ser afirmada por teste. A auditoria R4 mediu o custo disso: arrancar o
/// braco `Ok(false) if self.archived && !self.store.exists()` inteiro e
/// neutralizar o campo `archived` deixava os 28 testes do arquivo verdes,
/// identicos ao baseline. Em producao o destino e o terminal; no teste e um
/// vetor.
struct ArchiveGuard<'a> {
    store: &'a SessionStore,
    lang: Lang,
    /// Havia mesmo um blob para arquivar? `archive()` devolve `false` num
    /// store vazio, e nesse caso nao restaurar nada e o esperado.
    archived: bool,
    out: Box<dyn GuardOut + 'a>,
}

/// Para onde o [`ArchiveGuard`] fala. Ver o docstring dele.
trait GuardOut {
    /// Desfecho bom — em producao, stdout.
    fn ok(&mut self, line: &str);
    /// Desfecho ruim — em producao, stderr.
    fn warn(&mut self, line: &str);
}

/// A saida de producao.
struct TerminalOut;

impl GuardOut for TerminalOut {
    fn ok(&mut self, line: &str) {
        println!("{line}");
    }
    fn warn(&mut self, line: &str) {
        eprintln!("{line}");
    }
}

impl<'a> ArchiveGuard<'a> {
    fn archive(
        store: &'a SessionStore,
        lang: Lang,
    ) -> Result<Self, garraia_channels::whatsapp_linked::SessionError> {
        Self::archive_to(store, lang, Box::new(TerminalOut))
    }

    /// [`ArchiveGuard::archive`] com a saida injetada.
    fn archive_to(
        store: &'a SessionStore,
        lang: Lang,
        out: Box<dyn GuardOut + 'a>,
    ) -> Result<Self, garraia_channels::whatsapp_linked::SessionError> {
        let archived = store.archive()?;
        Ok(Self {
            store,
            lang,
            archived,
            out,
        })
    }
}

impl Drop for ArchiveGuard<'_> {
    fn drop(&mut self) {
        let lang = self.lang;
        let restored = self.store.restore_archive();
        let archived = self.archived;
        let live = self.store.exists();
        match restored {
            // Sai DEPOIS da mensagem do desfecho ("Cancelado.", "Nenhum QR foi
            // lido."), que e a ordem certa: primeiro o que aconteceu, depois o
            // que sobrou.
            Ok(true) => self.out.ok(&format!(
                "↩ {}",
                t(
                    lang,
                    "A sessão anterior foi restaurada — nada foi desvinculado.",
                    "Your previous session was restored — nothing was unlinked."
                )
            )),
            // Arquivamos, nao restauramos e nao ha sessao nova: o vinculo
            // anterior foi embora. Falar e o minimo — o usuario acabou de ler
            // "esta sessao nao vale mais" e sairia daqui achando que a antiga
            // continuava la.
            Ok(false) if archived && !live => self.out.warn(&format!(
                "! {}",
                t(
                    lang,
                    "A sessão anterior não pôde ser restaurada — o vínculo antigo foi perdido. Rode `garra whatsapp` e leia um QR novo.",
                    "The previous session could not be restored — the old link is gone. Run `garra whatsapp` and scan a new QR."
                )
            )),
            Ok(false) => {}
            // Sem `?` porque `Drop` nao propaga, e sem silencio porque uma
            // sessao boa presa no `.prev` e exatamente o que o usuario precisa
            // saber para recupera-la a mao.
            Err(e) => self.out.warn(&format!("{e}")),
        }
    }
}

fn link(ctx: &Context, prompter: &dyn Prompter) -> i32 {
    link_with(ctx, prompter, NodeRuntime::detect)
}

/// [`link`] com a deteccao do Node injetada.
///
/// # Por que um parametro, e nao `PATH=""` no teste
///
/// Porque a versao anterior destes testes zerava a `PATH` do processo inteiro
/// com um `unsafe { std::env::set_var }` cujo SAFETY dizia que "o mutex
/// serializa os testes que mexem em env neste binario". Essa nao e a condicao
/// de `set_var`: a condicao e que NENHUMA outra thread esteja no ambiente, e
/// `tempfile::tempdir()` le `TMPDIR`. Os outros ~530 testes deste binario
/// rodam concorrentes e chamam `tempdir()` o tempo todo. O raciocinio ja esta
/// escrito no docstring de `bridge.rs` que este mesmo PR acrescentou; deixa-lo
/// valer la e nao aqui era so escolher onde nao olhar.
///
/// Com a deteccao injetada, "nao ha Node" vira um `Err` que o teste passa —
/// nao um estado global que ele planta.
fn link_with(
    ctx: &Context,
    prompter: &dyn Prompter,
    detect_node: impl FnOnce() -> Result<NodeRuntime, BridgeError>,
) -> i32 {
    if !ctx.interactive {
        print_header(ctx);
        println!("{}", needs_a_terminal(ctx.lang, "link"));
        return EX_UNAVAILABLE;
    }

    let store = match ctx.store() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return EX_SOFTWARE;
        }
    };
    let key = match ctx.key() {
        Ok(k) => k,
        Err(e) => {
            eprintln!("{e}");
            return EX_SOFTWARE;
        }
    };

    // Sessao existente: pergunta, mas NAO mexe em disco ainda. Quem responde
    // "sim" aqui ainda vai ver a tela de consentimento, ainda pode recusa-la,
    // ainda pode dar Ctrl+C e ainda pode nao ter Node instalado — e em
    // qualquer um desses caminhos a sessao que funcionava tem de continuar
    // funcionando. Arquivar agora era desmontar o vinculo antes da ultima
    // confirmacao.
    let had_session = store.exists();
    let mut relink = false;
    if had_session {
        println!(
            "{}",
            t(
                ctx.lang,
                "Já existe uma sessão vinculada neste aparelho.",
                "This machine already has a linked session."
            )
        );
        // O texto diz o que de fato acontece: a sessao atual e ARQUIVADA
        // (`session.enc.prev`), so e descartada quando o vinculo novo conclui,
        // e volta sozinha se ele nao concluir (ver `ArchiveGuard`). "Apaga"
        // era impreciso nas duas pontas.
        let prompt = t(
            ctx.lang,
            "Re-vincular? A sessão atual sai de uso e só é descartada quando o novo vínculo concluir",
            "Re-link? The current session is set aside and only discarded once the new link completes",
        );
        match prompter.confirm(prompt, false) {
            Ok(true) => relink = true,
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
    // Vamos mostrar um QR quando nao ha sessao, ou quando o usuario pediu para
    // trocar a que existe.
    let will_show_qr = !had_session || relink;

    // Consentimento: so quando vamos de fato mostrar um QR.
    if will_show_qr && !consent(ctx, prompter) {
        println!("{}", t(ctx.lang, "Cancelado.", "Cancelled."));
        return EX_CANCELLED;
    }

    let node = match detect_node() {
        Ok(n) => n,
        Err(e) => {
            print_missing_node(ctx, &e);
            return EX_UNAVAILABLE;
        }
    };

    let bridge_dir = ctx.bridge_dir();
    // F3 da auditoria R4: o retorno NAO pode ser descartado. `Written`
    // significa que o `package.json`/`package-lock.json` embutidos mudaram —
    // tipicamente um `garra update` que bumpou o Baileys por causa de CVE. Se
    // a decisao de reinstalar olhasse so para `node_modules/` existir, o npm
    // nunca rodaria e a ponte subiria com a versao vulneravel indefinidamente,
    // reportando a versao velha em `started.baileys_version`.
    let materialized = match bridge::materialize(&bridge_dir, &bridge::EmbeddedAssets) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{e}");
            return EX_SOFTWARE;
        }
    };

    let runtime = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("{e}");
            return EX_SOFTWARE;
        }
    };

    if materialized == bridge::Materialized::Written || !bridge::deps_installed(&bridge_dir) {
        println!(
            "→ {}",
            t(
                ctx.lang,
                "Instalando as dependências do bridge (pode levar alguns minutos)…",
                "Installing bridge dependencies (this can take a few minutes)…"
            )
        );
        if let Err(e) = runtime.block_on(bridge::npm_ci(&node.npm, &bridge_dir)) {
            eprintln!();
            eprintln!("{e}");
            return EX_UNAVAILABLE;
        }
        println!(
            "✓ {}",
            t(ctx.lang, "Dependências prontas.", "Dependencies ready.")
        );
    }

    let launcher = NodeLauncher::new(&node.node, &bridge_dir);
    link_paired(ctx, &store, &key, &launcher, &runtime, relink)
}

/// A parte do [`link`] que comeca depois de o Node estar resolvido: arquivar a
/// sessao atual sob guard, mostrar o QR, parear e traduzir o desfecho em
/// codigo de saida.
///
/// # Por que uma funcao, e nao o resto de `link`
///
/// Porque a linha que INSTALA o [`ArchiveGuard`] e a correcao inteira desta
/// rodada, e dentro de `link` ela era inalcancavel por teste: tudo o que vem
/// antes — deteccao do Node, `npm ci` — falha primeiro num ambiente de teste,
/// e os testes existentes morriam na deteccao do Node sem nunca chegar aqui.
/// Trocar este bloco por um `store.archive()` cru, sem inverso — que e
/// exatamente o bug da rodada anterior —, deixava os 549 testes da crate
/// verdes.
///
/// Com o `launcher` como `&dyn BridgeLauncher` o teste entra por cima: um
/// launcher que sempre falha leva o fluxo ate o desfecho de erro, o guard cai,
/// e o que se afirma e o que o usuario ve em disco — sessao de volta, nada
/// arquivado.
fn link_paired(
    ctx: &Context,
    store: &SessionStore,
    key: &SessionKey,
    launcher: &dyn BridgeLauncher,
    runtime: &tokio::runtime::Runtime,
    relink: bool,
) -> i32 {
    // AGORA: consentimento dado, Node encontrado, dependencias prontas. Este
    // e o ultimo ponto antes de o QR aparecer.
    //
    // O comentario anterior dizia que era tambem "o primeiro ponto em que
    // arquivar deixa de poder desmontar um vinculo que continuaria valendo", e
    // isso era falso: depois daqui ainda vem o QR, e um Ctrl+C na tela dele ou
    // cinco QRs expirando deixavam o usuario **sem sessao viva**, com a boa
    // parada num `.prev` que nenhum caminho de codigo reabria. O guard abaixo
    // e o que faltava — ele desfaz o arquivamento em QUALQUER saida que nao
    // tenha gravado sessao nova, inclusive as que ninguem lembrou de listar.
    let _archive_guard = if relink {
        match ArchiveGuard::archive(store, ctx.lang) {
            Ok(g) => Some(g),
            Err(e) => {
                eprintln!("{e}");
                return EX_SOFTWARE;
            }
        }
    } else {
        None
    };

    print_instructions(ctx);

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
        runner::pair(launcher, store, key, &mut ui, cancel_rx).await
    });

    match outcome {
        Ok(outcome) => {
            println!();
            if outcome.reused_existing_session {
                println!(
                    "✓ {}",
                    t(
                        ctx.lang,
                        "Sessão encontrada e válida",
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
            // O que sai daqui e um DIRETORIO — `<data_dir>/whatsapp/default/`
            // —, nao credencial: o blob cifrado e a chave ficam dentro dele e
            // nenhum dos dois e lido aqui. O segmento de conta e
            // `DEFAULT_ACCOUNT`, constante de compilacao, e e exatamente disso
            // que depende a supressao do alerta CodeQL 173
            // (`rust/cleartext-logging`) registrada em
            // `docs/security/codeql-suppressions.md`. No dia em que a conta
            // virar dinamica esta linha passa a imprimi-la, e quem avisa e o
            // teste `every_session_store_in_the_cli_uses_the_constant_account`.
            println!(
                "✓ {} {}.",
                t(ctx.lang, "Sessão salva em", "Session saved in"),
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
                    "GarraIA está pronto para receber mensagens (inicie o gateway: `garra start`)",
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
        Err(RunError::SessionDead { reason_code }) => {
            println!();
            println!(
                "{}",
                t(
                    ctx.lang,
                    "Esta sessão não vale mais. Rode `garra whatsapp` de novo e leia um QR novo.",
                    "This session is no longer valid. Run `garra whatsapp` again and scan a new QR."
                )
            );
            // O codigo cru do Baileys e o que distingue "voce removeu o
            // aparelho no celular" (401) de "a Meta recusou esta sessao"
            // (403/419). Esconde-lo deixaria os dois com a mesma cara.
            if let Some(code) = reason_code {
                println!(
                    "  {} {code}",
                    t(ctx.lang, "Código do WhatsApp:", "WhatsApp code:")
                );
            }
            // Sem `?` e sem `eprintln!`, ao contrario dos outros call sites:
            // aqui o runner JA apagou a sessao, e o processo ja vai sair com
            // erro por causa disso. Falhar tambem a escrita da config nao muda
            // o que o usuario precisa fazer (rodar `garra whatsapp` de novo) e
            // uma segunda mensagem de erro so esconderia a primeira, que e a
            // que importa. O `enabled` remanescente e corrigido no proximo
            // link ou logout.
            let _ = disable_channel(ctx);
            EX_UNAVAILABLE
        }
        // `MissingDependencies` cai aqui de proposito: o `Display` dele ja
        // diz o comando (`npm ci`) e o diretorio exato, que e tudo o que o
        // usuario precisa. Reescrever a mensagem aqui a faria divergir.
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
    // F5: ate aqui o aviso do modo sem cofre so existia no `status` — depois
    // de a conta ja estar vinculada. Ele pertence ao momento em que a pessoa
    // decide, e precisa dizer a exposicao, nao so a variavel a definir.
    if let Ok(key) = ctx.key() {
        let warning = match ctx.lang {
            Lang::Pt => key.origin().warning(),
            Lang::En => key.origin().warning_en(),
        };
        if let Some(warning) = warning {
            println!();
            println!("  ⚠ {warning}");
        }
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
            "por um cliente NÃO oficial. Isso contraria os termos de uso da Meta.",
            "",
            "O risco real: a conta pode ser bloqueada, temporária ou",
            "permanentemente. Não existe recurso garantido.",
            "",
            "Recomendação: use um número secundário, e não o seu número",
            "principal. Um chip pré-pago ou um número virtual resolve.",
            "",
            "Se você precisa de suporte oficial, escolha a opção 2 do menu",
            "(WhatsApp Business / Cloud API da Meta).",
            "",
            "A sessão fica cifrada no seu computador e É a conta: quem tiver o",
            "arquivo fala como você, sem precisar do seu telefone. Trate-o como",
            "senha, inclusive nos backups.",
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
            "",
            "The session is stored encrypted on your computer and IS the account:",
            "whoever holds that file speaks as you, without needing your phone.",
            "Treat it like a password, backups included.",
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
            "  Configurações → Aparelhos conectados → Conectar um aparelho",
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
(só este caminho precisa; o resto do GarraIA não).",
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
            "Sem Node, a opção 2 (WhatsApp Business / Cloud API) funciona: \
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
            "✓ Autenticado. Sincronizando sessão…",
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
        println!("{}", needs_a_terminal(ctx.lang, "cloud"));
        return EX_UNAVAILABLE;
    }
    let Some(loader) = ctx.loader.as_ref() else {
        eprintln!(
            "{}",
            t(
                ctx.lang,
                "Não consegui abrir a config. Rode `garra init` primeiro.",
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
            "Verify token (o que você escolheu no webhook)",
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
            "Os segredos ficam só no arquivo (modo 0600) — não são exibidos aqui.",
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
    // `entry`, e nao `insert`: uma secao `channels.whatsapp` que ja existe
    // carrega escolhas do operador — gates, allowlists, overrides — que este
    // comando nao conhece e nao tem por que saber. Substituir a secao inteira
    // apagava tudo isso em silencio para quem so queria trocar um token.
    // Mexemos exatamente nas quatro chaves que o wizard pergunta.
    let entry = config
        .channels
        .entry(CLOUD_CONFIG_KEY.to_string())
        .or_insert_with(|| ChannelConfig {
            channel_type: CLOUD_CONFIG_KEY.to_string(),
            enabled: Some(true),
            settings: Default::default(),
        });
    entry.channel_type = CLOUD_CONFIG_KEY.to_string();
    entry.enabled = Some(true);
    for (key, value) in [
        ("access_token", access_token),
        ("phone_number_id", phone_number_id),
        ("verify_token", verify_token),
        ("app_secret", app_secret),
    ] {
        entry.settings.insert(
            key.to_string(),
            serde_json::Value::String(value.to_string()),
        );
    }
    // `ConfigLoader::save` ja aperta o arquivo para 0600 — o que importa
    // porque acabamos de escrever quatro segredos nele.
    loader.save(&config)?;
    Ok(())
}

#[cfg(test)]
mod tests;
