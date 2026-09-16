//! Canal **pull** `whatsapp_linked` — a peca que faz a mensagem escaneada pelo
//! QR chegar ao agente (#1238, fatia D).
//!
//! Irmao de [`super::whatsapp`] (Cloud API da Meta) e **nao** o substituto
//! dele: os dois podem estar ligados ao mesmo tempo, tem chaves de config
//! diferentes (`whatsapp_linked` × `whatsapp`) e nao compartilham uma linha de
//! transporte. O que este arquivo reusa do irmao e exatamente o que e do
//! *gateway* e nao do transporte: [`channel_gates`], o saneamento de entrada,
//! a hidratacao de historico, o `ExecContext` e a persistencia do turno.
//!
//! # O modelo de ameaca deste canal e outro
//!
//! No canal Cloud a mensagem chega por um webhook assinado pela Meta, de uma
//! conta Business que alguem aprovou. Aqui ela chega de **qualquer pessoa na
//! internet que conheca o numero pessoal do operador**, por um socket que a
//! propria conta abriu. E a maior superficie de injecao de prompt do projeto.
//! Tres decisoes saem disso, e nenhuma delas e configuravel para menos:
//!
//! 1. **Fail-closed de verdade.** O canal Cloud entrega a posse do bot a quem
//!    mandar a primeira mensagem (`needs_owner()` → `claim_owner`). Aqui isso
//!    seria entregar o agente ao primeiro estranho. Quem nao esta
//!    explicitamente na allowlist nao passa — nem com `AllowlistMode::Open`,
//!    que e uma decisao de ergonomia local e nao vale para um canal exposto ao
//!    mundo. Ver [`admitir`].
//! 2. **Ferramentas somente-leitura por padrao.** Sessao sem modo escolhido
//!    resolve para o perfil `search` (whitelist de leitura), e nao para "sem
//!    politica". Ver [`piso_somente_leitura`].
//! 3. **Guard de injecao indireta no texto recebido.** A issue #1243 propoe
//!    generalizar o guard que hoje so cobre `web_fetch`; ela **nao mergeou**,
//!    entao ele e aplicado localmente aqui. Ver [`preparar_entrada`].
//!
//! # PII
//!
//! `message` e `connected` carregam o JID inteiro de proposito — a allowlist
//! precisa dele. Esses valores **nunca** vao para log: o unico identificador
//! logavel e `phone_last4`. O teste `fonte_nao_loga_jid_cru` varre este arquivo
//! atras de regressao.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use garraia_agents::ChatMessage;
use garraia_agents::exec_context::ExecContext;
use garraia_channels::whatsapp_linked::health::{BridgeView, DiskFacts, LinkHealth, classify};
use garraia_channels::whatsapp_linked::{
    BridgeCommand, DEFAULT_ACCOUNT, InboundMessage, Jid, NodeLauncher, RunError, SessionKey,
    SessionStore, bridge, runner::InboundSink, runner::serve,
};
use garraia_config::AppConfig;
use garraia_security::{Allowlist, InputValidator, PairingManager};
use tokio::sync::{mpsc, watch};
use tracing::{info, warn};

use crate::state::SharedState;

use super::config::channel_gates;

/// Chave da secao de config. Vem do proprio canal para nao existir um segundo
/// literal capaz de divergir do que a CLI escreve.
pub const CONFIG_KEY: &str = garraia_channels::whatsapp_linked::CONFIG_KEY;

/// Modo default deste canal quando a sessao nao escolheu nenhum.
///
/// `search` e o unico perfil nativo com `whitelist_mode: true` e lista de
/// leitura — `ask` apenas *nega* tres ferramentas e libera o resto, o que num
/// canal aberto ao mundo e permissivo demais.
pub const DEFAULT_MODE: &str = "search";

/// Tamanho da fila de saida. Curta de proposito: comando enfileirado com a
/// ponte caida e descartado (decisao do `runner::serve`), entao acumular aqui
/// so atrasaria a descoberta de que a resposta nao saiu.
const OUTBOUND_CAPACITY: usize = 32;

// ---------------------------------------------------------------------------
// Estado vivo
// ---------------------------------------------------------------------------

/// O que o supervisor sabe sobre a ponte, legivel pelas rotas de leitura.
///
/// `AtomicU8` e nao `Mutex<BridgeView>`: `/api/channels` e `/api/diagnostics`
/// leem isto por request e um lock envenenado por um panic do supervisor
/// derrubaria as duas rotas — ou obrigaria a um `unwrap()` em producao, que a
/// regra 4 do `CLAUDE.md` proibe.
#[derive(Debug)]
pub struct WhatsAppLinkedRuntime {
    bridge: AtomicU8,
}

impl Default for WhatsAppLinkedRuntime {
    fn default() -> Self {
        Self {
            bridge: AtomicU8::new(view_to_u8(BridgeView::Unknown)),
        }
    }
}

const V_UNKNOWN: u8 = 0;
const V_NOT_STARTED: u8 = 1;
const V_DOWN: u8 = 2;
const V_CONNECTED: u8 = 3;

fn view_to_u8(v: BridgeView) -> u8 {
    match v {
        BridgeView::Unknown => V_UNKNOWN,
        BridgeView::NotStarted => V_NOT_STARTED,
        BridgeView::Down => V_DOWN,
        BridgeView::Connected => V_CONNECTED,
    }
}

fn u8_to_view(raw: u8) -> BridgeView {
    match raw {
        V_NOT_STARTED => BridgeView::NotStarted,
        V_DOWN => BridgeView::Down,
        V_CONNECTED => BridgeView::Connected,
        // Inclui `V_UNKNOWN` e qualquer valor que uma variante futura esqueca
        // de mapear: "nao sei" e a resposta segura, nao "caiu".
        _ => BridgeView::Unknown,
    }
}

impl WhatsAppLinkedRuntime {
    /// O que o supervisor viu por ultimo.
    pub fn bridge(&self) -> BridgeView {
        u8_to_view(self.bridge.load(Ordering::Relaxed))
    }

    pub fn set_bridge(&self, view: BridgeView) {
        self.bridge.store(view_to_u8(view), Ordering::Relaxed);
    }
}

/// Onde a sessao e a ponte deste canal moram, derivados do data dir.
#[derive(Debug, Clone)]
pub struct LinkedPaths {
    pub store: SessionStore,
    pub bridge_dir: PathBuf,
}

impl LinkedPaths {
    pub fn from_config(config: &AppConfig) -> Self {
        let data_dir = config.resolved_data_dir();
        Self {
            store: SessionStore::for_data_dir(&data_dir, DEFAULT_ACCOUNT),
            bridge_dir: data_dir.join("whatsapp").join("bridge"),
        }
    }
}

/// O veredito que o `/api/channels` e o `/api/diagnostics` mostram.
///
/// **Uma** chamada, os dois consumidores: e o que impede o console de dizer
/// "ativo" enquanto a pagina de diagnostico diz "ponte caida".
pub fn health(config: &AppConfig, runtime: &WhatsAppLinkedRuntime) -> (LinkHealth, LinkedPaths) {
    let paths = LinkedPaths::from_config(config);
    let facts = DiskFacts::read(&paths.store, &paths.bridge_dir);
    (classify(&facts, runtime.bridge()), paths)
}

// ---------------------------------------------------------------------------
// Config do canal
// ---------------------------------------------------------------------------

/// Os poucos botoes que este canal tem. Nenhum deles afrouxa a allowlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkedSettings {
    pub enabled: bool,
    /// Identidades que o operador declarou na config. Somam-se a allowlist
    /// global; **nao** a substituem, e nao existe valor que signifique "todos".
    pub allow: Vec<String>,
    /// Responder em grupo? Falso por padrao: um agente que responde sozinho no
    /// grupo da familia do operador e um incidente, nao um recurso.
    pub reply_in_groups: bool,
    /// Modo que vale quando a sessao nao escolheu nenhum.
    pub default_mode: String,
}

impl Default for LinkedSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            allow: Vec::new(),
            reply_in_groups: false,
            default_mode: DEFAULT_MODE.to_string(),
        }
    }
}

/// Le `channels.whatsapp_linked`. Pura: recebe o `AppConfig`, devolve a struct.
pub fn settings_from_config(config: &AppConfig) -> LinkedSettings {
    let Some(section) = config.channels.get(CONFIG_KEY) else {
        return LinkedSettings::default();
    };
    // Uma secao cuja `type` nao e `whatsapp_linked` nao e este canal. O
    // `ChannelConfig` guarda o tipo separado da chave, entao a checagem existe.
    if section.channel_type != CONFIG_KEY {
        return LinkedSettings::default();
    }
    let allow = section
        .settings
        .get("allow")
        .and_then(|v| v.as_array())
        .map(|itens| {
            itens
                .iter()
                .filter_map(|v| v.as_str())
                .map(normalizar_identidade)
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    LinkedSettings {
        // `enabled` ausente significa **desligado** neste canal, ao contrario
        // do default do `build_channels` (`unwrap_or(true)`): a secao so
        // aparece porque `garra whatsapp link` a escreveu, e ela e escrita
        // DEPOIS de a sessao existir. Um `true` implicito ligaria a supervisao
        // numa maquina onde o pareamento foi abortado no meio.
        enabled: section.enabled == Some(true),
        allow,
        reply_in_groups: section
            .settings
            .get("reply_in_groups")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        default_mode: section
            .settings
            .get("default_mode")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| DEFAULT_MODE.to_string()),
    }
}

/// A forma canonica de uma identidade neste canal.
///
/// Numero vira so digitos (`+55 11 99999-8888` → `5511999998888`), para casar
/// com o `from_number` que o canal Cloud ja usa na mesma allowlist. JID `@lid`
/// — que nao expoe numero — fica como veio.
pub fn normalizar_identidade(raw: &str) -> String {
    let raw = raw.trim();
    if raw.contains('@') {
        return raw.to_string();
    }
    raw.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// Quem mandou, do ponto de vista da allowlist e da sessao.
pub fn identidade_do_remetente(msg: &InboundMessage) -> String {
    match msg.sender_phone.as_deref() {
        Some(fone) => normalizar_identidade(fone),
        // Remetente `@lid` nao expoe numero; o JID e o unico identificador
        // estavel que sobra.
        None => msg.sender_jid.as_str().to_string(),
    }
}

/// Chave de sessao. Por **conversa** (`chat_jid`), nao por remetente: num
/// grupo o historico e do grupo.
///
/// Prefixo deliberadamente diferente do `whatsapp-` do canal Cloud — misturar
/// os dois faria a mesma pessoa continuar, no canal pessoal, uma conversa que
/// teve pela conta Business. O follow-up de migrar os dois para
/// `ChatSource::WhatsApp` + `resolve_session` (`garraia-db/src/chat_sync.rs`)
/// esta registrado e fora do escopo desta fatia.
pub fn session_id(msg: &InboundMessage) -> String {
    format!("whatsapp-linked-{}", msg.chat_jid.as_str())
}

// ---------------------------------------------------------------------------
// Gates
// ---------------------------------------------------------------------------

/// O veredito do gate de admissao.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admissao {
    /// Passa para o agente.
    Aceito,
    /// Codigo de pareamento resgatado agora: responde a boas-vindas e para.
    PareadoAgora,
    /// Nao passa.
    Recusado,
}

/// Admite (ou nao) um remetente.
///
/// # A diferenca para o canal Cloud, e por que ela existe
///
/// Duas coisas que o `bootstrap/whatsapp.rs` faz e que aqui seriam falhas:
///
/// - **Nao ha auto-claim de dono.** La, o primeiro remetente vira dono do bot.
///   Num canal onde o remetente e qualquer pessoa que tenha o numero pessoal do
///   operador, isso entrega o agente ao primeiro estranho que mandar "oi".
/// - **`AllowlistMode::Open` nao vale aqui.** `Allowlist::is_allowed` devolve
///   `true` para qualquer um em modo aberto; esse modo existe para ergonomia
///   local e nao pode valer para um canal exposto a internet. Por isso a
///   pergunta e `is_owner(..) || list_users().contains(..)` — presenca
///   explicita — e nao `is_allowed`.
///
/// Allowlist vazia significa **ninguem**. Quem precisa entrar entra com um
/// codigo de 6 digitos do `/pair`, ou pela lista `allow` da config.
pub fn admitir(
    list: &mut Allowlist,
    pairing: &mut PairingManager,
    remetente: &str,
    texto: &str,
) -> Admissao {
    if esta_explicitamente_liberado(list, remetente) {
        return Admissao::Aceito;
    }
    let candidato = texto.trim();
    if candidato.len() == 6 && candidato.chars().all(|c| c.is_ascii_digit()) {
        // `claim` e quem conta tentativa e queima o codigo (#1191).
        if pairing.claim(candidato, remetente).is_some() {
            list.add(remetente);
            return Admissao::PareadoAgora;
        }
    }
    Admissao::Recusado
}

/// Presenca **explicita** na allowlist — a pergunta que `is_allowed` nao faz
/// quando o modo e aberto.
pub fn esta_explicitamente_liberado(list: &Allowlist, remetente: &str) -> bool {
    list.is_owner(remetente) || list.list_users().iter().any(|u| *u == remetente)
}

/// O que fazer com uma mensagem antes de ela chegar perto do modelo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entrada {
    /// Entrega `texto` ao agente.
    Entregar { texto: String },
    /// Injecao direta detectada: nada vai ao modelo.
    Recusada,
}

/// Saneia e avalia o texto recebido.
///
/// Tres camadas, nesta ordem, e a ordem importa:
///
/// 1. [`garraia_security::injection::sanitize_indirect`] tira caracteres
///    invisiveis **antes** de qualquer casamento de padrao — sem isso um
///    `i\u{200B}gnore previous instructions` passa por todo o resto.
/// 2. [`InputValidator::sanitize`] tira controle, como no canal Cloud.
/// 3. [`InputValidator::check_prompt_injection`] recusa injecao direta.
///
/// Quando o texto limpo ainda tem sinal de injecao (homoglifo, invisivel,
/// padrao de instrucao), um banner e **prefixado**, no mesmo formato que o
/// `web_fetch` usa: o modelo recebe o conteudo marcado como DADO. Este e o
/// guard da #1243 aplicado localmente, porque a issue nao mergeou.
pub fn preparar_entrada(cru: &str) -> Entrada {
    let (limpo, relatorio) = garraia_security::injection::sanitize_indirect(cru);
    let saneado = InputValidator::sanitize(&limpo);
    if InputValidator::check_prompt_injection(&saneado) {
        return Entrada::Recusada;
    }
    let texto = if relatorio.is_suspicious() {
        format!(
            "{}\n\n{saneado}",
            garraia_security::injection::warning_banner(&relatorio)
        )
    } else {
        saneado
    };
    Entrada::Entregar { texto }
}

/// Aplica o piso somente-leitura do canal.
///
/// `ExecContext` sem modo significa **sem politica de ferramenta** (ver o
/// docblock de `ExecContext::agent_mode`): todo o conjunto liberado, `bash`
/// incluso. Isso e o default certo para a CLI, onde quem digita e o dono da
/// maquina, e o errado aqui. Escolha explicita do usuario (`/mode`) continua
/// vencendo — e assim que o operador "sobe o nivel".
pub fn piso_somente_leitura(mut exec: ExecContext, modo_default: &str) -> ExecContext {
    if exec.agent_mode.is_none() && exec.custom_profile.is_none() {
        exec.agent_mode = Some(modo_default.to_string());
    }
    exec
}

/// Esta mensagem merece um turno do agente?
///
/// Separada do sink para ser testavel sem runtime. Cada `false` aqui e um
/// incidente que nao acontece:
///
/// - `from_me`: a conta vinculada e a do proprio operador, entao **a resposta
///   do bot volta como mensagem recebida**. Sem este filtro o canal responde a
///   si mesmo para sempre, no primeiro "oi".
/// - `is_group` sem opt-in: ver [`LinkedSettings::reply_in_groups`].
/// - sem `text`: midia nao vai ao modelo nesta fatia.
pub fn deve_responder(msg: &InboundMessage, settings: &LinkedSettings) -> bool {
    if msg.from_me {
        return false;
    }
    if msg.is_group && !settings.reply_in_groups {
        return false;
    }
    msg.text.as_deref().is_some_and(|t| !t.trim().is_empty())
}

// ---------------------------------------------------------------------------
// Sink
// ---------------------------------------------------------------------------

/// O consumidor de mensagens: onde o transporte encosta no gateway.
pub struct GatewaySink {
    state: SharedState,
    runtime: Arc<WhatsAppLinkedRuntime>,
    settings: LinkedSettings,
    outbound: mpsc::Sender<BridgeCommand>,
}

impl GatewaySink {
    pub fn new(
        state: SharedState,
        runtime: Arc<WhatsAppLinkedRuntime>,
        settings: LinkedSettings,
        outbound: mpsc::Sender<BridgeCommand>,
    ) -> Self {
        Self {
            state,
            runtime,
            settings,
            outbound,
        }
    }

    /// Responde num chat. Erro de fila nao derruba nada: a ponte caida descarta
    /// de proposito (ver `runner::serve`).
    async fn responder(outbound: &mpsc::Sender<BridgeCommand>, chat: &Jid, texto: String) {
        let comando = BridgeCommand::Send {
            request_id: uuid::Uuid::new_v4().to_string(),
            chat_jid: chat.clone(),
            text: texto,
        };
        if outbound.send(comando).await.is_err() {
            warn!("whatsapp_linked: a fila de saida fechou; a resposta nao sera enviada");
        }
    }

    /// O turno inteiro de uma mensagem. `async` e fora do `deliver` porque
    /// `InboundSink::deliver` e sincrono e roda dentro do `select!` do driver —
    /// bloquear ali pararia de ler o stdout da ponte.
    async fn turno(
        state: SharedState,
        settings: LinkedSettings,
        outbound: mpsc::Sender<BridgeCommand>,
        msg: InboundMessage,
    ) {
        let remetente = identidade_do_remetente(&msg);
        let last4 = msg.sender_jid.last4();
        let bruto = msg.text.clone().unwrap_or_default();

        let (allowlist, pairing) = channel_gates(&state);
        let admissao = {
            // Os dois locks juntos, e soltos antes do `await`: `std::sync::
            // MutexGuard` nao e `Send`.
            let (Ok(mut list), Ok(mut pair)) = (allowlist.lock(), pairing.lock()) else {
                warn!("whatsapp_linked: gate envenenado; recusando por seguranca");
                return;
            };
            admitir(&mut list, &mut pair, &remetente, &bruto)
        };

        match admissao {
            Admissao::Recusado => {
                // Nao ha resposta: responder confirmaria ao estranho que o
                // numero roda um bot. Fica o log, com os 4 digitos apenas.
                warn!(
                    phone_last4 = %last4,
                    "whatsapp_linked: remetente fora da allowlist, mensagem descartada"
                );
                return;
            }
            Admissao::PareadoAgora => {
                info!(phone_last4 = %last4, "whatsapp_linked: remetente pareado por codigo");
                Self::responder(
                    &outbound,
                    &msg.chat_jid,
                    "Pronto! Voce tem acesso a este GarraIA. Mande sua pergunta.".to_string(),
                )
                .await;
                return;
            }
            Admissao::Aceito => {}
        }

        let Entrada::Entregar { texto } = preparar_entrada(&bruto) else {
            warn!(
                phone_last4 = %last4,
                "whatsapp_linked: entrada recusada por suspeita de injecao de prompt"
            );
            Self::responder(
                &outbound,
                &msg.chat_jid,
                "Nao consegui processar essa mensagem.".to_string(),
            )
            .await;
            return;
        };

        let sid = session_id(&msg);
        state
            .hydrate_session_history(&sid, Some(CONFIG_KEY), Some(&remetente))
            .await;
        let history: Vec<ChatMessage> = state.session_history(&sid);
        let continuity_key = state.continuity_key();
        let exec = piso_somente_leitura(
            state
                .exec_context_for_msg(&sid, Some(&remetente), Some(&texto))
                .await,
            &settings.default_mode,
        );

        let resposta = state
            .agents
            .process_message_with_agent_config(
                &sid,
                &texto,
                &history,
                continuity_key.as_deref(),
                Some(&remetente),
                None,
                None,
                None,
                None,
                &exec,
            )
            .await;

        match resposta {
            Ok(resposta) => {
                state
                    .persist_turn(&sid, Some(CONFIG_KEY), Some(&remetente), &texto, &resposta)
                    .await;
                Self::responder(&outbound, &msg.chat_jid, resposta).await;
            }
            Err(e) => {
                warn!(phone_last4 = %last4, "whatsapp_linked: o agente falhou: {e}");
                Self::responder(
                    &outbound,
                    &msg.chat_jid,
                    "Tive um problema para responder agora. Tente de novo em instantes."
                        .to_string(),
                )
                .await;
            }
        }
    }
}

impl InboundSink for GatewaySink {
    fn deliver(&self, message: InboundMessage) {
        if !deve_responder(&message, &self.settings) {
            return;
        }
        let state = Arc::clone(&self.state);
        let settings = self.settings.clone();
        let outbound = self.outbound.clone();
        tokio::spawn(Self::turno(state, settings, outbound, message));
    }

    fn on_connection(&self, jid: Option<&Jid>, connected: bool) {
        self.runtime.set_bridge(if connected {
            BridgeView::Connected
        } else {
            BridgeView::Down
        });
        // `jid` inteiro nunca entra no log — so os 4 ultimos digitos.
        match (connected, jid) {
            (true, Some(j)) => info!(
                phone_last4 = %j.last4(),
                "whatsapp_linked: conectado"
            ),
            (true, None) => info!("whatsapp_linked: conectado"),
            (false, _) => warn!("whatsapp_linked: desconectado"),
        }
    }
}

// ---------------------------------------------------------------------------
// Supervisor
// ---------------------------------------------------------------------------

/// Por que o supervisor nao subiu. Serve ao log e ao teste.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NaoSubiu {
    /// `channels.whatsapp_linked.enabled` nao e `true`.
    Desabilitado,
    /// Nao ha `session.enc`: `garra whatsapp link` ainda nao rodou.
    SemSessao,
    /// `node` nao esta na PATH.
    SemNode,
}

/// Decide se ha o que supervisionar. Pura o bastante para ter teste proprio.
pub fn deve_supervisionar(
    settings: &LinkedSettings,
    sessao_existe: bool,
    node_presente: bool,
) -> Result<(), NaoSubiu> {
    if !settings.enabled {
        return Err(NaoSubiu::Desabilitado);
    }
    if !sessao_existe {
        return Err(NaoSubiu::SemSessao);
    }
    if !node_presente {
        return Err(NaoSubiu::SemNode);
    }
    Ok(())
}

/// Sobe o canal, quando ha o que subir.
///
/// Devolve o `watch::Sender` de cancelamento quando subiu — o desligamento
/// gracioso do gateway o usa — e `Err` com o motivo quando nao havia o que
/// fazer. **Nao e erro**: um gateway sem WhatsApp vinculado e o caso comum.
pub fn spawn_whatsapp_linked(state: &SharedState) -> Result<watch::Sender<bool>, NaoSubiu> {
    let settings = settings_from_config(&state.config);
    let paths = LinkedPaths::from_config(&state.config);
    let node = bridge::find_executable("node");

    deve_supervisionar(&settings, paths.store.exists(), node.is_some())?;
    // `deve_supervisionar` ja provou que ha `node`; o `else` existe porque o
    // compilador nao sabe disso, e um `unwrap()` em producao e proibido.
    let Some(node) = node else {
        return Err(NaoSubiu::SemNode);
    };

    // Seeds da config entram na allowlist compartilhada. Sao identidades
    // explicitas que o operador escreveu — o oposto de afrouxar o gate.
    if !settings.allow.is_empty() {
        let (allowlist, _) = channel_gates(state);
        match allowlist.lock() {
            Ok(mut list) => {
                for identidade in &settings.allow {
                    list.add(identidade.clone());
                }
                info!(
                    total = settings.allow.len(),
                    "whatsapp_linked: identidades da config adicionadas a allowlist"
                );
            }
            Err(_) => warn!("whatsapp_linked: allowlist envenenada; seeds da config ignorados"),
        }
    }

    let key = match SessionKey::resolve(
        paths.store.dir(),
        garraia_security::vault_passphrase_from_env().as_deref(),
    ) {
        Ok(key) => key,
        Err(e) => {
            // O valor nunca entra na mensagem: `SessionError` so carrega
            // caminho e categoria.
            warn!("whatsapp_linked: nao consegui abrir a chave da sessao: {e}");
            return Err(NaoSubiu::SemSessao);
        }
    };

    let runtime = Arc::clone(&state.whatsapp_linked);
    runtime.set_bridge(BridgeView::NotStarted);

    let (outbound_tx, outbound_rx) = mpsc::channel(OUTBOUND_CAPACITY);
    let sink: Arc<dyn InboundSink> = Arc::new(GatewaySink::new(
        Arc::clone(state),
        Arc::clone(&runtime),
        settings,
        outbound_tx,
    ));
    let launcher: Arc<dyn bridge::BridgeLauncher> =
        Arc::new(NodeLauncher::new(node, paths.bridge_dir.clone()));

    let (cancel_tx, cancel_rx) = watch::channel(false);
    let store = paths.store.clone();

    tokio::spawn(async move {
        let resultado = serve(launcher, store, key, sink, outbound_rx, cancel_rx, || {
            rand::random::<f64>()
        })
        .await;
        runtime.set_bridge(BridgeView::Down);
        match resultado {
            Ok(()) => info!("whatsapp_linked: supervisor encerrado"),
            Err(RunError::SessionDead { reason_code }) => {
                // O `serve` ja apagou o material. Deixar `enabled = true` faria
                // todo boot seguinte pagar spawn + handshake + falha por uma
                // credencial que nao existe mais.
                warn!(
                    reason_code = ?reason_code,
                    "whatsapp_linked: o servidor invalidou a sessao; desligando o canal"
                );
                desligar_na_config();
            }
            Err(e) => warn!("whatsapp_linked: supervisor parou: {e}"),
        }
    });

    Ok(cancel_tx)
}

/// Grava `channels.whatsapp_linked.enabled = false`.
///
/// Mesma escrita da CLI (`garra whatsapp logout`), pela mesma funcao —
/// [`garraia_config::ConfigLoader::set_channel_enabled`].
fn desligar_na_config() {
    match garraia_config::ConfigLoader::new() {
        Ok(loader) => match loader.set_channel_enabled(CONFIG_KEY, false) {
            Ok(true) => info!("whatsapp_linked: canal desligado na config"),
            Ok(false) => {}
            Err(e) => warn!("whatsapp_linked: nao consegui desligar o canal na config: {e}"),
        },
        Err(e) => warn!("whatsapp_linked: config indisponivel para desligar o canal: {e}"),
    }
}

#[cfg(test)]
mod tests;
