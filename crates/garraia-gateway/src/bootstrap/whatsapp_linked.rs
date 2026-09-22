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
//!    politica". `default_mode` so aceita modo **nativo** e diferente de
//!    `auto`: nome desconhecido nao vira portao aberto — o canal nao sobe.
//!    Ver [`piso_somente_leitura`] e [`modo_padrao`].
//! 3. **Guard de injecao indireta no texto recebido.** A issue #1243 propoe
//!    generalizar o guard que hoje so cobre `web_fetch`; ela **nao mergeou**,
//!    entao ele e aplicado localmente aqui. Ver [`preparar_entrada`].
//!
//! # O dono num pod isolado (ADR 0024, #1329)
//!
//! A unica coisa que afrouxa o piso e uma decisao **explicita** em dois
//! lugares ao mesmo tempo: `execution.profile = isolated-pod` (o operador
//! declara que o pod, e nao o Garra, e a fronteira) **e** a identidade do
//! remetente em `channels.whatsapp_linked.owners`. Com os dois, e so em
//! conversa 1:1, o piso do turno passa a ser `code` (sem whitelist:
//! filesystem, `bash`, MCP, subagentes). Grupo nunca herda, contato so
//! pareado por codigo nunca herda, e `owners` em perfil `standard` nao muda
//! nada. Ver [`perfil_do_turno`] e [`modo_do_piso`].
//!
//! # PII
//!
//! `message` e `connected` carregam o JID inteiro de proposito — a allowlist
//! precisa dele. Esses valores **nunca** vao para log: o unico identificador
//! logavel e `phone_last4`. O teste `fonte_nao_loga_jid_cru` varre este arquivo
//! atras de regressao.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use garraia_agents::ChatMessage;
use garraia_agents::exec_context::ExecContext;
use garraia_agents::modes::{AgentMode, ToolGate};
use garraia_channels::whatsapp_linked::health::{BridgeView, DiskFacts, LinkHealth, classify};
use garraia_channels::whatsapp_linked::{
    BridgeCommand, DEFAULT_ACCOUNT, InboundMessage, Jid, NodeLauncher, RunError, SessionError,
    SessionKey, SessionStore, bridge, runner::InboundSink, runner::serve,
};
use garraia_config::{AppConfig, ExecutionProfile};
use garraia_security::{InputValidator, PairingManager};
use tokio::sync::{mpsc, watch};
use tracing::{info, warn};

use crate::state::SharedState;

use super::config::channel_gates;
use super::execution::politica_de_execucao;

/// Chave da secao de config. Vem do proprio canal para nao existir um segundo
/// literal capaz de divergir do que a CLI escreve.
pub const CONFIG_KEY: &str = garraia_channels::whatsapp_linked::CONFIG_KEY;

/// Modo default deste canal quando a sessao nao escolheu nenhum.
///
/// `search` e o unico perfil nativo com `whitelist_mode: true` e lista de
/// leitura — `ask` apenas *nega* tres ferramentas e libera o resto, o que num
/// canal aberto ao mundo e permissivo demais.
///
/// E o piso de **todo** remetente em `standard`, e de todo remetente que nao
/// e dono (ou que fala por grupo) em `isolated-pod`. Ver [`modo_do_piso`].
pub const DEFAULT_MODE: &str = "search";

/// Modo default do **dono** em conversa 1:1 quando `execution.profile =
/// isolated-pod` e `default_mode` nao foi declarado (ADR 0024).
///
/// `code` e nativo, sem whitelist e sem `denied`: passa filesystem, `bash`,
/// toda ferramenta MCP e subagente. E o "poder total dentro do pod" da
/// decisao do dono — e por isso so vale para quem esta em `owners`, em
/// conversa 1:1, com o perfil declarado. Ver [`perfil_do_turno`].
pub const DEFAULT_MODE_DO_DONO_NO_POD: &str = "code";

/// Tamanho da fila de saida. Curta de proposito: comando enfileirado com a
/// ponte caida e descartado (decisao do `runner::serve`), entao acumular aqui
/// so atrasaria a descoberta de que a resposta nao saiu.
const OUTBOUND_CAPACITY: usize = 32;

// ---------------------------------------------------------------------------
// Estado vivo
// ---------------------------------------------------------------------------

/// O que o supervisor sabe sobre a ponte, legivel pelas rotas de leitura, e
/// **onde o cancelamento dele mora**.
///
/// `AtomicU8` e nao `Mutex<BridgeView>` para o `bridge`: `/api/channels` e
/// `/api/diagnostics` leem isto por request e um lock envenenado por um panic
/// do supervisor derrubaria as duas rotas — ou obrigaria a um `unwrap()` em
/// producao, que a regra 4 do `CLAUDE.md` proibe.
///
/// # Por que o `watch::Sender` do cancelamento mora aqui
///
/// Ele precisa viver enquanto o servidor viver. `watch::Receiver::changed()`
/// devolve `Err` quando o **ultimo** `Sender` cai, e o primeiro braco do
/// `select!` `biased` de `serve_once` trata `changed.is_err()` como
/// cancelamento — entao um `Sender` solto nao "vaza": ele **mata o canal** no
/// ciclo seguinte, depois de spawn, handshake, `session_load` e `start`, em
/// decimos de segundo, sem nenhum erro.
///
/// Devolver o `Sender` para o chamador era um convite a isso: o boot escrevia
/// `Ok(_cancel) => info!(...)` e o `_cancel` morria no fim do braco do `match`.
/// Por isso [`spawn_whatsapp_linked`] nao devolve mais o handle — ele e
/// estacionado aqui, num `AppState` que vive o processo inteiro, e a unica
/// forma de encerrar passou a ser [`Self::cancelar`]. A mutacao "soltar o
/// handle no call-site" deixou de ser expressavel.
///
/// O `Mutex` aqui e tocado duas vezes na vida do processo (subir e encerrar),
/// nunca por request, entao ele nao carrega o risco que motivou o `AtomicU8`
/// do `bridge`; ainda assim nenhum acesso usa `unwrap()` — ver
/// [`Self::reter_cancelamento`].
#[derive(Debug)]
pub struct WhatsAppLinkedRuntime {
    bridge: AtomicU8,
    cancel: std::sync::Mutex<Option<watch::Sender<bool>>>,
    /// Mensagens recusadas de remetente `@lid` sem numero desde o boot
    /// (#1345). Contagem, nunca identidade: e o que o `/api/diagnostics`
    /// (auth-free) mostra para o operador entender por que um numero que ele
    /// autorizou nao recebe resposta.
    recusas_lid: AtomicU64,
}

impl Default for WhatsAppLinkedRuntime {
    fn default() -> Self {
        Self {
            bridge: AtomicU8::new(view_to_u8(BridgeView::Unknown)),
            cancel: std::sync::Mutex::new(None),
            recusas_lid: AtomicU64::new(0),
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

    /// Quantas mensagens de remetente `@lid` sem numero foram recusadas desde
    /// o boot (#1345).
    pub fn recusas_lid(&self) -> u64 {
        self.recusas_lid.load(Ordering::Relaxed)
    }

    /// Conta mais uma recusa de `@lid` sem numero; devolve o total novo.
    pub fn registrar_recusa_lid(&self) -> u64 {
        self.recusas_lid
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1)
    }

    /// Estaciona o cancelamento do supervisor que acabou de subir.
    ///
    /// Um supervisor anterior (que so existe se alguem subir o canal duas
    /// vezes) e cancelado ao ser substituido, em vez de ficar orfao com a ponte
    /// aberta.
    ///
    /// # Envenenamento
    ///
    /// Os tres acessos recuperam o guarda com `into_inner()` em vez de tratar o
    /// `Err` como falha. Envenenado aqui significa so "alguem entrou em panico
    /// segurando este slot"; o `Option<Sender>` dentro dele nao tem invariante
    /// para quebrar. E o custo de desistir seria alto e silencioso: **soltar** o
    /// `Sender` mata o canal recem-subido, que e exatamente o defeito que este
    /// campo existe para impedir.
    pub fn reter_cancelamento(&self, tx: watch::Sender<bool>) {
        let mut slot = self.cancel.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(anterior) = slot.replace(tx) {
            let _ = anterior.send(true);
        }
    }

    /// Ha supervisor retido? E a pergunta que o teste de boot faz.
    pub fn cancelamento_vivo(&self) -> bool {
        self.cancel
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    /// Encerra o supervisor graciosamente. `false` quando nao havia nenhum.
    pub fn cancelar(&self) -> bool {
        match self.cancel.lock().unwrap_or_else(|e| e.into_inner()).take() {
            Some(tx) => tx.send(true).is_ok(),
            None => false,
        }
    }
}

/// O arquivo, no diretorio da sessao, em que o gateway conta as recusas de
/// remetente `@lid` sem numero (#1345). E como o `garraia whatsapp status` —
/// outro processo — fica sabendo por que um numero autorizado nao responde.
pub const ARQUIVO_RECUSAS_LID: &str = "recusas-lid.json";

/// O conteudo de [`ARQUIVO_RECUSAS_LID`]. Nunca o LID inteiro.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RecusasLid {
    /// O gateway que escreveu: a CLI so mostra o arquivo do gateway vivo, e
    /// um arquivo de uma execucao anterior nao diz nada sobre esta.
    pub pid: u32,
    /// Recusas de `@lid` sem numero desde o boot desse gateway.
    pub recusas: u64,
    /// Os quatro ultimos digitos do ultimo LID recusado.
    pub final4: String,
}

/// Le [`ARQUIVO_RECUSAS_LID`] de `dir`. Ausente, grande demais ou ilegivel =
/// `None`: e informacao de apoio, nunca motivo de erro.
pub fn ler_recusas_lid(dir: &Path) -> Option<RecusasLid> {
    let bytes = std::fs::read(dir.join(ARQUIVO_RECUSAS_LID)).ok()?;
    if bytes.len() > 4096 {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

/// Grava [`ARQUIVO_RECUSAS_LID`] de forma atomica (tmp `0600` no mesmo
/// diretorio + `rename`). O nome do tmp carrega pid e contagem: dois turnos
/// recusados ao mesmo tempo nao disputam o mesmo arquivo.
pub fn gravar_recusas_lid(dir: &Path, recusas: &RecusasLid) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(recusas).map_err(std::io::Error::other)?;
    let tmp = dir.join(format!(
        ".{ARQUIVO_RECUSAS_LID}.{}.{}.tmp",
        recusas.pid, recusas.recusas
    ));
    let escrito = (|| {
        let mut opcoes = std::fs::OpenOptions::new();
        opcoes.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opcoes.mode(0o600);
        }
        let mut arquivo = opcoes.open(&tmp)?;
        std::io::Write::write_all(&mut arquivo, &bytes)?;
        arquivo.sync_all()?;
        std::fs::rename(&tmp, dir.join(ARQUIVO_RECUSAS_LID))
    })();
    if escrito.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    escrito
}

/// Onde a sessao e a ponte deste canal moram, derivados do data dir.
#[derive(Debug, Clone)]
pub struct LinkedPaths {
    pub store: SessionStore,
    pub bridge_dir: PathBuf,
}

impl LinkedPaths {
    /// # Por que `Result`
    ///
    /// `SessionStore::for_data_dir` valida o segmento de conta e devolve
    /// `Result`; o gateway so passa [`DEFAULT_ACCOUNT`], `pub const … =
    /// "default"`, que satisfaz a allowlist em tempo de compilacao, entao o
    /// `Err` nao tem como acontecer hoje. Ele e propagado assim mesmo porque a
    /// alternativa seria `unwrap()` em producao (regra 4) e porque e esse
    /// `Result` que mantem verdadeira a afirmacao de que `for_data_dir` e o
    /// unico construtor visivel de fora da crate — de que a supressao CodeQL
    /// 173 depende.
    pub fn from_config(config: &AppConfig) -> Result<Self, SessionError> {
        let data_dir = config.resolved_data_dir();
        Ok(Self {
            store: SessionStore::for_data_dir(&data_dir, DEFAULT_ACCOUNT)?,
            bridge_dir: bridge_dir(&data_dir),
        })
    }
}

/// Onde a ponte Node vive. Nao depende do store, e por isso sobrevive ao
/// braco de erro de [`LinkedPaths::from_config`].
fn bridge_dir(data_dir: &std::path::Path) -> PathBuf {
    data_dir.join("whatsapp").join("bridge")
}

/// O veredito que o `/api/channels` e o `/api/diagnostics` mostram, com o
/// diretorio da ponte que a mensagem de proximo passo cita.
///
/// **Uma** chamada, os dois consumidores: e o que impede o console de dizer
/// "ativo" enquanto a pagina de diagnostico diz "ponte caida". Devolve o
/// `bridge_dir` e nao o [`LinkedPaths`] inteiro porque e so isso que os dois
/// consomem — o store fica onde e usado.
pub fn health(config: &AppConfig, runtime: &WhatsAppLinkedRuntime) -> (LinkHealth, PathBuf) {
    match LinkedPaths::from_config(config) {
        Ok(paths) => {
            let facts = DiskFacts::read(&paths.store, &paths.bridge_dir);
            (classify(&facts, runtime.bridge()), paths.bridge_dir)
        }
        // Inalcancavel com `DEFAULT_ACCOUNT` — ver o docstring acima. Sem
        // store nao ha fato de disco para ler, entao fail-closed em "ninguem
        // vinculou": o console mostra o canal como opcional em vez de mentir
        // "conectado".
        Err(_) => (
            LinkHealth::NotLinked,
            bridge_dir(&config.resolved_data_dir()),
        ),
    }
}

// ---------------------------------------------------------------------------
// Config do canal
// ---------------------------------------------------------------------------

/// Os poucos botoes que este canal tem. Nenhum deles afrouxa a allowlist.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinkedSettings {
    pub enabled: bool,
    /// Identidades que o operador declarou na config. Somam-se a allowlist
    /// global; **nao** a substituem, e nao existe valor que signifique "todos".
    pub allow: Vec<String>,
    /// Identidades do **dono** (ADR 0024). Mesma normalizacao do `allow`, e
    /// quem esta aqui e admitido como se estivesse la. O que muda e o piso:
    /// em `execution.profile = isolated-pod`, e so em conversa 1:1, o dono
    /// recebe [`Self::modo_padrao_efetivo`] do pod (`code` por default). Em
    /// `standard` esta lista nao confere poder nenhum — o `config check`
    /// avisa. Pareamento por codigo **nunca** entra aqui: e credencial fraca
    /// (memoria do processo, seis digitos), e dono e identidade declarada.
    pub owners: Vec<String>,
    /// Responder em grupo? Falso por padrao: um agente que responde sozinho no
    /// grupo da familia do operador e um incidente, nao um recurso.
    pub reply_in_groups: bool,
    /// Modo que vale quando a sessao nao escolheu nenhum, **como declarado**.
    ///
    /// `None` quando a chave esta ausente ou em branco — e ai o default
    /// depende do perfil de execucao (ver [`Self::modo_padrao_efetivo`]).
    /// Vem **cru** da config: `settings_from_config` e pura e nao decide nada.
    /// Quem valida e [`modo_padrao`] — na subida, por [`deve_supervisionar`]
    /// (recusa com [`NaoSubiu::ModoPadraoInvalido`]), e no turno, por
    /// [`piso_somente_leitura`] (cai para [`DEFAULT_MODE`]).
    pub default_mode: Option<String>,
}

impl LinkedSettings {
    /// Quantas identidades distintas este canal admite pela config: `allow`
    /// uniao `owners`, sem repeticao (#1345).
    ///
    /// E a mesma uniao que [`PortaoDoCanal::from_settings`] monta, entao um
    /// `0` aqui e exatamente "ninguem recebera resposta" — o que o `status`
    /// da CLI, o `/api/diagnostics` e o aviso de boot dizem. Contagem, nunca
    /// identidade: nenhum dos tres consumidores pode listar numeros.
    pub fn autorizados(&self) -> usize {
        self.allow
            .iter()
            .chain(self.owners.iter())
            .map(|id| chave_do_portao(id))
            .collect::<std::collections::HashSet<_>>()
            .len()
    }

    /// Quantos donos distintos, pela mesma chave do portao: o mesmo numero
    /// escrito duas vezes (ou com e sem o nono digito) conta um.
    pub fn donos(&self) -> usize {
        self.owners
            .iter()
            .map(|id| chave_do_portao(id))
            .collect::<std::collections::HashSet<_>>()
            .len()
    }

    /// O `default_mode` que vale para um dado perfil de execucao: o valor
    /// declarado, se houver; senao [`DEFAULT_MODE`] em `standard` e
    /// [`DEFAULT_MODE_DO_DONO_NO_POD`] em `isolated-pod`.
    ///
    /// E o unico lugar em que o perfil de execucao vira nome de modo. O
    /// chamador escolhe o perfil que **cabe ao remetente** — e nao o do
    /// processo — via [`modo_do_piso`]: remetente que nao e dono, ou fala por
    /// grupo, recebe o valor de `standard` mesmo num pod.
    pub fn modo_padrao_efetivo(&self, perfil: ExecutionProfile) -> String {
        match &self.default_mode {
            Some(declarado) => declarado.clone(),
            None => match perfil {
                ExecutionProfile::Standard => DEFAULT_MODE,
                ExecutionProfile::IsolatedPod => DEFAULT_MODE_DO_DONO_NO_POD,
            }
            .to_string(),
        }
    }
}

/// Le uma lista de identidades da secao (`allow`, `owners`): cada item por
/// [`normalizar_identidade`], vazios descartados, chave ausente = vazia.
fn identidades_da_secao(section: &garraia_config::ChannelConfig, chave: &str) -> Vec<String> {
    section
        .settings
        .get(chave)
        .and_then(|v| v.as_array())
        .map(|itens| {
            itens
                .iter()
                .filter_map(|v| v.as_str())
                .map(normalizar_identidade)
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
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
    LinkedSettings {
        // `enabled` ausente significa **desligado** neste canal, ao contrario
        // do default do `build_channels` (`unwrap_or(true)`): a secao so
        // aparece porque `garra whatsapp link` a escreveu, e ela e escrita
        // DEPOIS de a sessao existir. Um `true` implicito ligaria a supervisao
        // numa maquina onde o pareamento foi abortado no meio.
        enabled: section.enabled == Some(true),
        allow: identidades_da_secao(section, "allow"),
        owners: identidades_da_secao(section, "owners"),
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
            .filter(|s| !s.is_empty()),
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

/// A forma que o **portao** compara (#1345): [`normalizar_identidade`] mais
/// o nono digito dos celulares brasileiros.
///
/// O WhatsApp nao acrescentou o 9 ao JID de todo celular brasileiro: muita
/// conta antiga (DDD 31 em diante, sobretudo) segue com o JID de 12 digitos
/// (`55 31 9999-8888`), enquanto o operador digita o numero como ele e hoje,
/// com 13 (`+55 31 99999-8888`). Comparados byte a byte os dois nunca casavam
/// e a recusa era silenciosa — com os mesmos quatro ultimos digitos no log.
///
/// A regra e a do plano de numeracao: `55` + DDD + `9` + oito digitos cujo
/// primeiro e 6 a 9 (a faixa do celular antigo) e o mesmo assinante que
/// `55` + DDD + esses oito digitos. So essa forma perde o 9; fixo (primeiro
/// digito 2 a 5), numero de outro pais e `@lid` passam intactos. A config
/// guarda o que o operador digitou — esta chave so existe na comparacao.
pub fn chave_do_portao(identidade: &str) -> String {
    let b = identidade.as_bytes();
    let celular_br = b.len() == 13
        && b.iter().all(u8::is_ascii_digit)
        && identidade.starts_with("55")
        && b[4] == b'9'
        && matches!(b[5], b'6'..=b'9');
    if celular_br {
        // So digitos ASCII: o corte por byte nao parte caractere.
        let (ddd, resto) = identidade.split_at(4);
        format!("{ddd}{}", &resto[1..])
    } else {
        identidade.to_string()
    }
}

/// O remetente so e conhecido pelo JID `@lid`, sem numero (#1345)?
pub fn e_lid(identidade: &str) -> bool {
    identidade.ends_with("@lid")
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

/// A allowlist **deste canal**, e de mais nenhum.
///
/// # Por que este canal nao usa o `Allowlist` global
///
/// O `garraia_security::Allowlist` e um slot unico por instalacao, e **todo**
/// canal irmao o preenche sozinho: `bootstrap/whatsapp.rs`, `signal.rs`,
/// `slack.rs`, `teams.rs` e `matrix.rs` chamam `claim_owner(<primeiro
/// remetente>)` — auto-claim — e `claim_owner` **tambem insere o dono em
/// `allowed_users`**. Consultar aquele objeto aqui, por `is_owner` ou por
/// `list_users()`, anulava o recuso de auto-claim deste canal por tabela:
/// bastava um estranho mandar "oi" para o numero da Cloud API (que identifica
/// por `from_number`, digitos puros — a mesma forma que
/// [`normalizar_identidade`] produz) para ele passar a ser admitido no numero
/// **pessoal** do operador, sem `allow` e sem codigo de pareamento.
///
/// E `Allowlist::add` chama `save()`: a lista `allow` da config era gravada no
/// `allowlist.json` global. Duas consequencias, as duas erradas — tirar o
/// numero do `allow` no `config.yml` nao tirava nada (nao havia revogacao por
/// config), e um numero liberado aqui virava identidade valida em Telegram,
/// Discord, Slack, Signal, Matrix e no `/start`.
///
/// # A fonte de verdade e a config
///
/// `da_config` e reconstruido do `AppConfig` a cada boot: o que sai do `allow`
/// sai do portao. Quem resgata um codigo `/pair` entra em `pareados`, que e
/// **memoria do processo** e nao sobrevive a um restart — um codigo de
/// pareamento e um bootstrap, nao uma credencial duravel. Acesso que precisa
/// durar se declara no `allow`, que e tambem o unico lugar de onde da para
/// remove-lo. Uma fonte de verdade, revogacao que funciona, e nenhum arquivo
/// novo com permissao para nascer frouxo.
#[derive(Debug, Default)]
pub struct PortaoDoCanal {
    da_config: std::collections::HashSet<String>,
    pareados: std::collections::HashSet<String>,
}

impl PortaoDoCanal {
    /// O portao que a config descreve. Lista vazia significa **ninguem**.
    ///
    /// `owners` entra como `allow` (ADR 0024): dono e admitido sem precisar
    /// se listar duas vezes. O que `owners` confere **alem** da admissao nao
    /// mora aqui — e por turno, em [`perfil_do_turno`].
    pub fn from_settings(settings: &LinkedSettings) -> Self {
        Self {
            da_config: settings
                .allow
                .iter()
                .chain(settings.owners.iter())
                .map(|id| chave_do_portao(id))
                .collect(),
            pareados: std::collections::HashSet::new(),
        }
    }

    /// Presenca explicita: nao existe modo, nem dono, nem valor que signifique
    /// "todos". Compara pela [`chave_do_portao`].
    pub fn libera(&self, remetente: &str) -> bool {
        let chave = chave_do_portao(remetente);
        self.da_config.contains(&chave) || self.pareados.contains(&chave)
    }

    fn parear(&mut self, remetente: &str) {
        self.pareados.insert(chave_do_portao(remetente));
    }

    /// Troca a parte que vem da config e mantem `pareados` (#1345) — menos
    /// quem acabou de **sair** da config.
    ///
    /// Chamado a cada turno com a config viva: o que entrou no `allow` passa a
    /// valer na proxima mensagem, e o que saiu deixa de valer — sem restart.
    /// `pareados` nao vem da config e nao tem como ser relido dela; um codigo
    /// resgatado continua valendo ate o processo cair, como antes. A excecao
    /// e a revogacao: quem estava no `allow` (ou em `owners`) e foi tirado
    /// perde tambem o pareamento, senao "apague o numero da lista" nao
    /// revogaria quem um dia resgatou um codigo.
    pub fn recarregar(&mut self, settings: &LinkedSettings) {
        let mut pareados = std::mem::take(&mut self.pareados);
        let anterior = std::mem::take(&mut self.da_config);
        *self = Self::from_settings(settings);
        pareados.retain(|p| !(anterior.contains(p) && !self.da_config.contains(p)));
        self.pareados = pareados;
    }
}

/// A admissao que vale **neste turno**: `boot` com `enabled`, `allow` e
/// `owners` trocados pelos da config viva (#1345).
///
/// # O que recarrega e o que nao
///
/// So a **admissao** — quem entra e quem e dono. `default_mode` e
/// `reply_in_groups` ficam os do boot: o primeiro foi validado por
/// [`deve_supervisionar`] na subida e o aviso de drift de MCP falou dele; o
/// segundo decide se o turno nem nasce, no `deliver`. Mudar os dois pede
/// restart, e a documentacao diz isso.
///
/// # Por que `owners` recarrega junto com `allow`
///
/// Porque recarregar so o `allow` seria fail-open na revogacao: um dono tirado
/// de `owners` que continua no `allow` seguiria recebendo o piso do pod
/// (`code`: filesystem, bash, MCP) ate o proximo restart. Com os dois lidos da
/// mesma fonte, tirar da lista tira o poder na mensagem seguinte.
///
/// # Fail-closed
///
/// A config viva sem a secao, com `type` errado ou com `enabled != true`
/// chega aqui como `LinkedSettings` com `enabled = false` (e o que
/// [`settings_from_config`] devolve), e o resultado admite **ninguem** —
/// nem pela config, nem por dono. Quem recusa o turno inteiro, codigo de
/// pareamento incluso, e o sink, olhando o `enabled` devolvido.
pub fn admissao_vigente(boot: &LinkedSettings, viva: &LinkedSettings) -> LinkedSettings {
    let mut vigente = boot.clone();
    vigente.enabled = viva.enabled;
    if viva.enabled {
        vigente.allow = viva.allow.clone();
        vigente.owners = viva.owners.clone();
    } else {
        vigente.allow = Vec::new();
        vigente.owners = Vec::new();
    }
    vigente
}

/// O aviso de boot de um canal que sobe sem ninguem autorizado (#1345).
///
/// `None` quando ha pelo menos uma identidade em `allow` ou `owners`. O texto
/// cita o comando que resolve e nao carrega numero nenhum. Puro, para o teste
/// nao depender de capturar `tracing`.
///
/// "Vale sem reiniciar" so e dito com `a_quente` — o `ConfigWatcher` ligado,
/// que o boot so liga quando o `config.yml` ja existia. Sem ele a lista do
/// boot vale o processo inteiro, e o texto manda reiniciar.
pub fn aviso_portao_vazio(settings: &LinkedSettings, bin: &str, a_quente: bool) -> Option<String> {
    (settings.autorizados() == 0).then(|| {
        let depois = if a_quente {
            "vale sem reiniciar".to_string()
        } else {
            format!("e depois `{bin} restart`: este gateway nao vigia o config.yml")
        };
        format!(
            "whatsapp_linked: nenhum numero autorizado em `channels.whatsapp_linked.allow` \
             (nem em `owners`) — toda mensagem sera descartada em silencio. Rode \
             `{bin} whatsapp allow <numero>` ({depois})"
        )
    })
}

/// Admite (ou nao) um remetente.
///
/// # A diferenca para o canal Cloud, e por que ela existe
///
/// Duas coisas que o `bootstrap/whatsapp.rs` faz e que aqui seriam falhas:
///
/// - **Nao ha auto-claim de dono.** La, o primeiro remetente vira dono do bot.
///   Num canal onde o remetente e qualquer pessoa que tenha o numero pessoal do
///   operador, isso entrega o agente ao primeiro estranho que mandar "oi". E
///   nao basta nao fazer auto-claim aqui: e preciso nao **honrar** o auto-claim
///   que os irmaos fazem — ver [`PortaoDoCanal`].
/// - **`AllowlistMode::Open` nao vale aqui.** `Allowlist::is_allowed` devolve
///   `true` para qualquer um em modo aberto; esse modo existe para ergonomia
///   local e nao pode valer para um canal exposto a internet. O
///   [`PortaoDoCanal`] nao tem modo nenhum, entao a pergunta nem chega a
///   existir.
///
/// Portao vazio significa **ninguem**. Quem precisa entrar entra com um codigo
/// de 6 digitos do `/pair`, ou pela lista `allow` da config.
///
/// O `PairingManager` continua sendo o global de proposito: o docblock dele diz
/// que um codigo "vale em qualquer canal habilitado e o `channel_id` e
/// informativo, nao um escopo", e e assim que o operador gera por outro canal o
/// codigo que entra neste. O que muda e o destino do resgate — ele libera
/// **aqui**, e nao na allowlist da instalacao.
pub fn admitir(
    portao: &mut PortaoDoCanal,
    pairing: &mut PairingManager,
    remetente: &str,
    texto: &str,
) -> Admissao {
    if portao.libera(remetente) {
        return Admissao::Aceito;
    }
    let candidato = texto.trim();
    if candidato.len() == 6 && candidato.chars().all(|c| c.is_ascii_digit()) {
        // `claim` e quem conta tentativa e queima o codigo (#1191).
        if pairing.claim(candidato, remetente).is_some() {
            portao.parear(remetente);
            return Admissao::PareadoAgora;
        }
    }
    Admissao::Recusado
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

/// O modo que `channels.whatsapp_linked.default_mode` pode nomear.
///
/// So modo **nativo**, e nao `auto`. Os dois limites existem porque o nome vai
/// virar `ToolGate` sem ninguem no meio:
///
/// - `ToolGate::for_mode_name` trata nome desconhecido como **portao aberto**
///   ("recusar tudo porque alguem digitou errado seria pior que ignorar o
///   modo"). E o default certo para a CLI, onde quem digita e o dono da
///   maquina, e o errado aqui: `default_mode = "pesquisa"` — um typo — daria
///   `bash`, `file_write` e toda ferramenta MCP registrada a quem manda
///   mensagem para o numero do operador. Modo **customizado** (#986) cai no
///   mesmo caso: o perfil dele mora no banco e e resolvido so para o modo que
///   a *sessao* escolheu (`AppState::exec_context_for_inner`), nunca para o
///   piso do canal — para o portao, o nome de um modo customizado aqui e um
///   nome desconhecido. Quem quiser um customizado neste canal o escolhe com
///   `/mode` na sessao, que e escolha explicita e resolve o perfil.
/// - `auto` deixa o **texto da mensagem** escolher o perfil
///   (`ToolGate::para_o_turno` → `classify_heuristic`), `code` incluso, e cai
///   em portao aberto quando a heuristica nao classifica. Num canal em que o
///   texto vem de um estranho, isso e deixar o estranho escolher a politica.
///
/// `None` e recusa: na subida vira [`NaoSubiu::ModoPadraoInvalido`]; no turno,
/// [`piso_somente_leitura`] cai para [`DEFAULT_MODE`]. Os dois lados, para que
/// nenhum call-site que pule `deve_supervisionar` monte um portao aberto.
pub fn modo_padrao(nome: &str) -> Option<AgentMode> {
    match AgentMode::from_str(nome.trim()) {
        Some(AgentMode::Auto) | None => None,
        Some(modo) => Some(modo),
    }
}

/// Aplica o piso somente-leitura do canal.
///
/// `ExecContext` sem modo significa **sem politica de ferramenta** (ver o
/// docblock de `ExecContext::agent_mode`): todo o conjunto liberado, `bash`
/// incluso. Isso e o default certo para a CLI, onde quem digita e o dono da
/// maquina, e o errado aqui. Escolha explicita do usuario (`/mode`) continua
/// vencendo — e assim que o operador "sobe o nivel".
///
/// O nome que entra no `ExecContext` e o `as_str()` do modo validado por
/// [`modo_padrao`], nunca a string da config: `ToolGate::for_mode_name`, que e
/// quem le esse campo no turno, trata nome desconhecido como portao aberto, e
/// o piso nao pode ser a porta para isso. Nome que nao valida cai em
/// [`DEFAULT_MODE`] — [`deve_supervisionar`] ja recusou a subida nesse caso,
/// entao chegar aqui e um call-site que a pulou, e ainda assim nao abre nada.
pub fn piso_somente_leitura(mut exec: ExecContext, modo_default: &str) -> ExecContext {
    if exec.agent_mode.is_none() && exec.custom_profile.is_none() {
        exec.agent_mode = Some(match modo_padrao(modo_default) {
            Some(modo) => modo.as_str().to_string(),
            None => {
                warn!(
                    "whatsapp_linked: `default_mode` nao e um modo nativo deste canal; \
                     o piso do turno e `{DEFAULT_MODE}`"
                );
                DEFAULT_MODE.to_string()
            }
        });
    }
    exec
}

/// O perfil efetivo de **um turno** (ADR 0024).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PerfilDoTurno {
    /// Dono declarado, em conversa 1:1, num processo em `isolated-pod`: o
    /// piso e o do pod ([`DEFAULT_MODE_DO_DONO_NO_POD`] salvo `default_mode`).
    Completo,
    /// Todo o resto: o piso de `standard` ([`DEFAULT_MODE`] salvo
    /// `default_mode`).
    Padrao,
}

impl PerfilDoTurno {
    /// `completo` | `padrao` — o que vai para o log do turno.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completo => "completo",
            Self::Padrao => "padrao",
        }
    }
}

/// Decide o perfil do turno. Pura, e avaliada **depois** de [`admitir`] e
/// **antes** de montar o `ExecContext`.
///
/// Sao tres condicoes, todas obrigatorias, e nenhuma delas e inferida:
///
/// 1. `execution.profile = isolated-pod` — o operador declarou que o pod e a
///    fronteira. Em `standard`, `owners` nao muda nada (o `config check`
///    avisa).
/// 2. Conversa **1:1**. Mensagem de grupo carrega o JID do participante, mas
///    quem le a resposta e o grupo inteiro; poder total nunca e herdado por
///    grupo, nem quando o participante e o dono.
/// 3. O remetente esta em `owners`, pela mesma [`chave_do_portao`] que a
///    admissao usa. Contato so pareado por codigo nao esta —
///    `pareados` e memoria do processo, nao identidade declarada.
///
/// Qualquer duvida cai em [`PerfilDoTurno::Padrao`], que e o comportamento
/// de hoje.
pub fn perfil_do_turno(
    perfil: ExecutionProfile,
    settings: &LinkedSettings,
    remetente: &str,
    is_group: bool,
) -> PerfilDoTurno {
    let chave = chave_do_portao(remetente);
    if perfil.is_isolated_pod()
        && !is_group
        && settings.owners.iter().any(|d| chave_do_portao(d) == chave)
    {
        PerfilDoTurno::Completo
    } else {
        PerfilDoTurno::Padrao
    }
}

/// O nome do modo que vale como piso para um perfil de turno.
///
/// `Completo` recebe o `default_mode` efetivo do pod; `Padrao` recebe o de
/// `standard` — **mesmo num processo em `isolated-pod`**. E assim que
/// remetente admitido que nao e dono, e qualquer mensagem de grupo, ficam
/// exatamente onde estao hoje (`search`, salvo `default_mode` declarado). O
/// perfil de execucao do processo nao entra aqui de proposito: ele ja foi
/// consumido por [`perfil_do_turno`], e um `Completo` so existe em
/// `isolated-pod`.
pub fn modo_do_piso(perfil_turno: PerfilDoTurno, settings: &LinkedSettings) -> String {
    match perfil_turno {
        PerfilDoTurno::Completo => settings.modo_padrao_efetivo(ExecutionProfile::IsolatedPod),
        PerfilDoTurno::Padrao => settings.modo_padrao_efetivo(ExecutionProfile::Standard),
    }
}

/// Servidores MCP cujas ferramentas o portao de um perfil **libera**.
///
/// # O que isto substitui (#1327)
///
/// Ate a #1327 este canal se recusava a subir — e recusava cada turno —
/// enquanto houvesse qualquer ferramenta de servidor MCP registrada. A recusa
/// nasceu para a #1264, quando `ToolGate::permite` isentava do whitelist todo
/// nome com `__`; a #1288 fechou a isencao, e desde entao ferramenta MCP so
/// passa por um perfil com whitelist quando a `allowed` a declara
/// (`servidor/*` ou o nome completo). O piso [`piso_somente_leitura`] escolhe
/// o `search`, que nao declara servidor nenhum: `filesystem__write_file` e
/// negada por nome, como `bash`. A recusa ficou sem funcao — e, como toda
/// instalacao nova ganha o servidor `filesystem` no primeiro boot, ela fazia o
/// canal nunca subir em instalacao padrao.
///
/// # O que fica: aviso, nao recusa
///
/// `default_mode` so aceita modo nativo ([`modo_padrao`]), entao o portao em
/// exame na subida e sempre o de um perfil nativo — e, dos nativos, os que tem
/// whitelist nao declaram servidor MCP nenhum. O aviso so tem o que dizer
/// quando o operador escolheu um perfil **sem** whitelist (`ask`, `code`), em
/// que passa tudo que o `denied` nao nomeia — ferramenta MCP inclusa. Isso e
/// **escolha declarada** — o `default_mode` e do operador —, entao o canal
/// avisa em vez de recusar: [`spawn_whatsapp_linked`] emite um `warn!` na
/// subida listando o que esta funcao devolve, com o motivo
/// ([`motivo_da_liberacao`]). Com o `search` a lista e vazia.
///
/// A funcao em si e generica sobre qualquer `ToolGate` — `permite()` tambem
/// devolve `true` com `whitelist_mode` ligado e `allowed` vazia
/// (`ToolGate::whitelist_ligada_mas_vazia`) e para o que `servidor/*` declara
/// —, mas esses dois portoes so existem em perfil customizado, que nao chega
/// aqui: modo customizado nao serve de `default_mode` (ver [`modo_padrao`]).
///
/// Devolve nomes de **servidor**, ordenados e sem repeticao: e o que o
/// operador reconhece no `mcp.json`, e nao carrega argumento nem segredo.
pub fn mcp_liberadas_pelo_perfil(
    gate: &ToolGate,
    inventario: &[garraia_agents::runtime::ToolInventoryEntry],
) -> Vec<String> {
    let mut servidores: Vec<String> = inventario
        .iter()
        .filter(|t| t.source == "mcp" && gate.permite(&t.name))
        .map(|t| {
            t.server
                .clone()
                .unwrap_or_else(|| "<servidor desconhecido>".to_string())
        })
        .collect();
    servidores.sort();
    servidores.dedup();
    servidores
}

/// Por que um portao deixou ferramenta MCP passar — o pedaco do aviso de
/// [`avisar_drift_de_mcp`] que precisa ser verdade.
///
/// A primeira versao do aviso dizia "e o `allowed` declarado" para todo caso,
/// e o unico caso alcancavel pelo `default_mode` — perfil nativo sem whitelist
/// — nao tem `allowed` nenhuma. Sao quatro formas de um `ToolGate` liberar
/// MCP, e o texto diz qual foi:
///
/// | portao                                   | alcancavel por `default_mode`? |
/// |------------------------------------------|--------------------------------|
/// | sem perfil (`sem_politica`)              | nao — [`modo_padrao`] recusa   |
/// | whitelist ligada com `allowed` vazia     | nao — so em perfil customizado |
/// | whitelist com `servidor/*` declarado     | nao — so em perfil customizado |
/// | perfil sem whitelist (`ask`, `code`)     | **sim**                        |
pub fn motivo_da_liberacao(gate: &ToolGate) -> &'static str {
    if gate.nome_do_modo().is_none() {
        "nao ha perfil nenhum em vigor: portao aberto"
    } else if gate.whitelist_ligada_mas_vazia() {
        "o perfil liga `whitelist_mode` com `allowed` vazia, o que permite tudo"
    } else if gate.restringe_por_whitelist() {
        "a `allowed` do perfil declara esses servidores (`servidor/*` ou nome completo)"
    } else {
        "o perfil nao tem whitelist de ferramenta, entao passa tudo que o `denied` nao nomeia"
    }
}

/// O `warn!` de [`mcp_liberadas_pelo_perfil`], uma vez por subida do canal.
///
/// Recebe o modo **ja validado** por [`modo_padrao`], e nao a string da
/// config: e a assinatura que impede este aviso de examinar um portao que o
/// turno nao monta. O portao e `ToolGate::for_mode_name(<nome do modo>)` — o
/// mesmo caminho que [`piso_somente_leitura`] + `ToolGate::para_o_turno`
/// percorrem no turno de uma sessao sem modo escolhido —, e com o modo
/// validado ele e sempre o de um perfil nativo, nunca `sem_politica()`.
///
/// # Por perfil de execucao (ADR 0024)
///
/// - `standard`: `modo` e o piso de todo remetente. Liberar MCP aqui e
///   escolha declarada do operador no `default_mode`: avisa e diz como
///   voltar ao `search`.
/// - `isolated-pod` **sem** dono em `owners`: o perfil nao muda nada neste
///   canal — o aviso diz isso, e examina o piso que de fato vale para todo
///   mundo (o de `standard`).
/// - `isolated-pod` **com** dono: `modo` e o piso do dono em 1:1. Liberar
///   MCP e o que o perfil existe para fazer, entao o texto diz "decisao de
///   perfil", nao "confirme que e intencional" — e lembra que nao-dono e
///   grupo continuam no piso de `standard`.
fn avisar_drift_de_mcp(
    modo: AgentMode,
    perfil: ExecutionProfile,
    settings: &LinkedSettings,
    agents: &garraia_agents::AgentRuntime,
) {
    let inventario = agents.tool_inventory();
    match perfil {
        ExecutionProfile::Standard => avisar_escolha_do_operador(modo, &inventario),
        ExecutionProfile::IsolatedPod if settings.owners.is_empty() => {
            let piso = settings.modo_padrao_efetivo(ExecutionProfile::Standard);
            warn!(
                "whatsapp_linked: execution.profile = isolated-pod, mas \
                 `channels.whatsapp_linked.owners` esta vazio — o perfil nao muda nada neste \
                 canal: todo remetente admitido fica no piso `{piso}`. Declare a identidade do \
                 dono em `owners` para o perfil completo valer em conversa 1:1 (ADR 0024)"
            );
            // O piso que vale para todo mundo e o de `standard`; se ele
            // libera MCP, e o `default_mode` declarado, e o aviso e o mesmo.
            if let Some(modo) = modo_padrao(&piso) {
                avisar_escolha_do_operador(modo, &inventario);
            }
        }
        ExecutionProfile::IsolatedPod => {
            let nome = modo.as_str();
            let gate = ToolGate::for_mode_name(nome);
            let liberadas = mcp_liberadas_pelo_perfil(&gate, &inventario);
            if liberadas.is_empty() {
                return;
            }
            let motivo = motivo_da_liberacao(&gate);
            let servidores = liberadas.join(", ");
            let donos = settings.owners.len();
            let piso_padrao = settings.modo_padrao_efetivo(ExecutionProfile::Standard);
            warn!(
                "whatsapp_linked: perfil isolated-pod — o dono em conversa 1:1 ({donos} \
                 declarado(s) em `channels.whatsapp_linked.owners`) recebe o piso `{nome}`, que \
                 libera ferramentas MCP dos servidores {servidores} ({motivo}). E decisao de \
                 perfil (ADR 0024), nao erro: o pod e a fronteira. Remetente admitido que nao e \
                 dono e mensagem de grupo ficam no piso `{piso_padrao}`"
            );
        }
    }
}

/// O aviso de drift do perfil `standard`: liberar MCP no piso de todo
/// remetente e escolha do operador, e o texto diz como voltar atras.
fn avisar_escolha_do_operador(
    modo: AgentMode,
    inventario: &[garraia_agents::runtime::ToolInventoryEntry],
) {
    let nome = modo.as_str();
    let gate = ToolGate::for_mode_name(nome);
    let liberadas = mcp_liberadas_pelo_perfil(&gate, inventario);
    if liberadas.is_empty() {
        return;
    }
    let motivo = motivo_da_liberacao(&gate);
    let servidores = liberadas.join(", ");
    warn!(
        "whatsapp_linked: o perfil `{nome}` (`channels.whatsapp_linked.default_mode`) libera \
         ferramentas MCP dos servidores {servidores} a quem manda mensagem para este numero \
         — {motivo}; use `search` para um piso somente-leitura, ou confirme que e intencional"
    );
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

/// Os settings que valem para **um turno** (#1345).
///
/// Com o `ConfigWatcher` ligado (o boot liga quando o `config.yml` existe), a
/// admissao — `enabled`, `allow`, `owners` — e relida da config viva a cada
/// mensagem, via [`admissao_vigente`]: `garraia whatsapp allow` e a revogacao
/// por edicao do arquivo valem na mensagem seguinte, sem restart.
///
/// Sem watcher a config nao muda durante o processo, e `current_config()`
/// devolveria a mesma do boot — de onde `boot` ja saiu. Os settings do boot
/// valem inteiros, e e isso que deixa o harness de teste passar settings
/// direto ao [`supervisionar`] sem escrever a secao no `AppConfig`.
fn settings_do_turno(state: &SharedState, boot: &LinkedSettings) -> LinkedSettings {
    if state.has_config_watcher() {
        admissao_vigente(boot, &settings_from_config(&state.current_config()))
    } else {
        boot.clone()
    }
}

// ---------------------------------------------------------------------------
// Sink
// ---------------------------------------------------------------------------

/// O consumidor de mensagens: onde o transporte encosta no gateway.
pub struct GatewaySink {
    state: SharedState,
    runtime: Arc<WhatsAppLinkedRuntime>,
    settings: LinkedSettings,
    /// O portao **deste** canal. Nao e o `state.allowlist`: ver
    /// [`PortaoDoCanal`].
    portao: Arc<std::sync::Mutex<PortaoDoCanal>>,
    outbound: mpsc::Sender<BridgeCommand>,
}

impl GatewaySink {
    pub fn new(
        state: SharedState,
        runtime: Arc<WhatsAppLinkedRuntime>,
        settings: LinkedSettings,
        outbound: mpsc::Sender<BridgeCommand>,
    ) -> Self {
        let portao = Arc::new(std::sync::Mutex::new(PortaoDoCanal::from_settings(
            &settings,
        )));
        Self {
            state,
            runtime,
            settings,
            portao,
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

    /// Conta a recusa de um `@lid` sem numero no runtime (para o
    /// `/api/diagnostics`) e no [`ARQUIVO_RECUSAS_LID`] (para o `status` da
    /// CLI). Best-effort: falhar ao gravar nao muda a recusa.
    fn registrar_recusa_lid(state: &SharedState, final4: &str) {
        let recusas = state.whatsapp_linked.registrar_recusa_lid();
        let Ok(paths) = LinkedPaths::from_config(&state.config) else {
            return;
        };
        let registro = RecusasLid {
            pid: std::process::id(),
            recusas,
            final4: final4.to_string(),
        };
        if let Err(e) = gravar_recusas_lid(paths.store.dir(), &registro) {
            warn!("whatsapp_linked: nao consegui registrar a recusa de @lid: {e}");
        }
    }

    /// O turno inteiro de uma mensagem. `async` e fora do `deliver` porque
    /// `InboundSink::deliver` e sincrono e roda dentro do `select!` do driver —
    /// bloquear ali pararia de ler o stdout da ponte.
    async fn turno(
        state: SharedState,
        settings: LinkedSettings,
        portao: Arc<std::sync::Mutex<PortaoDoCanal>>,
        outbound: mpsc::Sender<BridgeCommand>,
        msg: InboundMessage,
    ) {
        let remetente = identidade_do_remetente(&msg);
        let last4 = msg.sender_jid.last4();
        let bruto = msg.text.clone().unwrap_or_default();

        // Só o `PairingManager` e compartilhado com os outros canais — um
        // codigo do `/pair` vale em qualquer canal, e o docblock dele diz isso.
        // A allowlist **nao** e: ver [`PortaoDoCanal`].
        let (_, pairing) = channel_gates(&state);
        // #1345: a admissao deste turno sai da config VIVA, nao da do boot.
        // Ver `admissao_vigente` e `settings_do_turno`.
        let settings = settings_do_turno(&state, &settings);
        let admissao = if !settings.enabled {
            // Canal desligado (ou secao sumida) na config viva: ninguem entra,
            // nem por codigo de pareamento. Mesmo silencio do `Recusado`.
            Admissao::Recusado
        } else {
            // Os dois locks juntos, e soltos antes do `await`: `std::sync::
            // MutexGuard` nao e `Send`.
            let (Ok(mut gate), Ok(mut pair)) = (portao.lock(), pairing.lock()) else {
                warn!("whatsapp_linked: gate envenenado; recusando por seguranca");
                return;
            };
            gate.recarregar(&settings);
            admitir(&mut gate, &mut pair, &remetente, &bruto)
        };

        match admissao {
            Admissao::Recusado => {
                // Nao ha resposta: responder confirmaria ao estranho que o
                // numero roda um bot. Fica o log, com os 4 digitos apenas.
                if e_lid(&remetente) {
                    // #1345: remetente so com LID, sem numero. Um numero no
                    // `allow` nunca casa com ele — o operador precisa saber
                    // que e ESTE o caso, e nao "o numero esta errado".
                    Self::registrar_recusa_lid(&state, &last4);
                    warn!(
                        lid_last4 = %last4,
                        "whatsapp_linked: remetente @lid sem numero fora da allowlist, \
                         mensagem descartada (um numero no `allow` nao casa com LID)"
                    );
                } else {
                    warn!(
                        phone_last4 = %last4,
                        "whatsapp_linked: remetente fora da allowlist, mensagem descartada"
                    );
                }
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

        // O perfil do turno (ADR 0024): DEPOIS de admitir, ANTES do
        // `ExecContext`. Puro sobre o que ja esta na mao — perfil do processo,
        // `owners`, identidade normalizada e `is_group`. O que sai daqui e so
        // um NOME de modo; quem o aplica e `piso_somente_leitura`, e quem o
        // faz valer contra o inventario vivo e o `ToolGate` do runtime, a
        // cada turno — ferramenta MCP inclusa (ver `mcp_liberadas_pelo_perfil`).
        let politica = politica_de_execucao(&state.config);
        let perfil_turno = perfil_do_turno(politica.perfil, &settings, &remetente, msg.is_group);
        let modo_do_piso = modo_do_piso(perfil_turno, &settings);
        // O que vai ao log e o nome VALIDADO do piso (o `as_str()` do modo), e
        // nao a string da config; o fallback e o mesmo de `piso_somente_leitura`.
        let etiqueta = perfil_turno.as_str();
        let piso = modo_padrao(&modo_do_piso).map_or(DEFAULT_MODE, |m| m.as_str());
        info!(
            phone_last4 = %last4,
            perfil = etiqueta,
            modo = piso,
            "whatsapp_linked: turno admitido"
        );

        let sid = session_id(&msg);
        state
            .hydrate_session_history(&sid, Some(CONFIG_KEY), Some(&remetente))
            .await;
        let history: Vec<ChatMessage> = state.session_history(&sid);
        let continuity_key = state.continuity_key();
        // #1343: o pedido de confirmacao que pausar este turno so pode ser
        // aprovado pelo MESMO remetente, na mesma conversa. Em grupo a sessao
        // e do grupo (`session_id` usa o `chat_jid`), e o remetente e quem
        // falou: o "sim" de outro membro nao aprova e ainda encerra o pedido
        // (fail-closed). O escopo entra DEPOIS do piso, que nao o toca.
        let exec = crate::approval_scope::com_escopo(
            piso_somente_leitura(
                state
                    .exec_context_for_msg(&sid, Some(&remetente), Some(&texto))
                    .await,
                &modo_do_piso,
            ),
            CONFIG_KEY,
            &sid,
            &remetente,
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
        let portao = Arc::clone(&self.portao);
        let outbound = self.outbound.clone();
        tokio::spawn(Self::turno(state, settings, portao, outbound, message));
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
///
/// O `Display` de cada variante diz a **acao**, nao o nome: e o que sai no
/// `warn!` do boot (#1327). Ate la o log dizia `canal nao subiu (<nome da
/// variante da recusa por servidor MCP>)` em `INFO`, e o operador ficava com
/// "tudo vinculado" no `status` e um canal mudo. Aquela variante nao existe
/// mais (#1327, #1330) e um teste varre este arquivo para ela nao voltar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NaoSubiu {
    /// `channels.whatsapp_linked.enabled` nao e `true`.
    Desabilitado,
    /// `channels.whatsapp_linked.default_mode` nao nomeia um modo nativo, ou
    /// nomeia `auto`. Ver [`modo_padrao`]: subir assim seria subir com o
    /// portao de ferramenta aberto a quem manda mensagem.
    ModoPadraoInvalido {
        /// O valor como veio da config. E config do operador, nao PII, e e o
        /// que ele precisa ver para achar o typo no arquivo.
        modo: String,
    },
    /// Nao ha `session.enc` legivel: `garra whatsapp link` ainda nao rodou,
    /// ou a chave da sessao nao abre.
    SemSessao,
    /// `node` nao esta na PATH.
    SemNode,
}

impl std::fmt::Display for NaoSubiu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Desabilitado => f.write_str(
                "canal desligado na config (`channels.whatsapp_linked.enabled = false`)",
            ),
            Self::ModoPadraoInvalido { modo } => write!(
                f,
                "`channels.whatsapp_linked.default_mode` = `{modo}` nao e um modo nativo deste \
                 canal (e `auto` nao vale aqui): use `search`, outro modo nativo, ou remova a chave"
            ),
            // O gateway e o mesmo executavel que o operador chama: o passo
            // nomeia `garra` ou `garraia` conforme o que esta rodando, nunca
            // um alias que pode nao existir na maquina (#1329).
            Self::SemSessao => write!(
                f,
                "nao ha sessao vinculada legivel neste data dir: rode `{} whatsapp link`",
                garraia_common::executavel::nome()
            ),
            Self::SemNode => f.write_str(
                "`node` nao encontrado: instale Node.js 20+ e garanta `node` na PATH do gateway",
            ),
        }
    }
}

/// Decide se ha o que supervisionar — e devolve o modo que vai valer como
/// piso. Pura o bastante para ter teste proprio.
///
/// A ordem e a da acao que o operador tem de tomar: ligar antes de corrigir o
/// `default_mode` (as duas chaves estao na mesma secao da config), corrigir a
/// config antes de vincular, vincular antes de instalar `node` (o proprio
/// `garra whatsapp link` exige `node`). O inventario de ferramentas MCP
/// **nao** e entrada desta decisao desde a #1327 — ver
/// [`mcp_liberadas_pelo_perfil`].
///
/// # Por que o modo e validado AQUI, e nao so no turno
///
/// [`piso_somente_leitura`] ja cai para [`DEFAULT_MODE`] quando o nome nao
/// resolve, entao o turno nunca roda com portao aberto. Mas `default_mode` que
/// nao vale e config errada, e config errada que "funciona" e a especie de
/// defeito que a #1327 nasceu para tirar do log: o operador escreve `code` com
/// typo, o canal sobe em `search` e ele passa a tarde perguntando por que o
/// agente nao escreve arquivo. Recusar a subida, com a frase de acao no
/// `Display`, e o que faz o erro aparecer onde foi cometido.
///
/// # O que "o modo que vai valer como piso" significa por perfil (ADR 0024)
///
/// O que se valida e o `default_mode` **efetivo** para o perfil de execucao
/// do processo ([`LinkedSettings::modo_padrao_efetivo`]): o valor declarado,
/// se houver, ou o default do perfil. Em `standard` esse e o piso de todo
/// remetente. Em `isolated-pod` e o piso do **dono em 1:1** — o mais alto que
/// este canal pode aplicar nesta subida —, e e sobre ele que o aviso de drift
/// fala; nao-dono e grupo ficam em [`DEFAULT_MODE`] (ou no valor declarado,
/// que e a mesma string e portanto a mesma validacao). Os dois defaults sao
/// literais nativos, entao a unica coisa que pode falhar aqui e um valor
/// declarado — o mesmo que falharia hoje.
pub fn deve_supervisionar(
    settings: &LinkedSettings,
    perfil: ExecutionProfile,
    sessao_existe: bool,
    node_presente: bool,
) -> Result<AgentMode, NaoSubiu> {
    if !settings.enabled {
        return Err(NaoSubiu::Desabilitado);
    }
    let efetivo = settings.modo_padrao_efetivo(perfil);
    let modo = modo_padrao(&efetivo).ok_or_else(|| NaoSubiu::ModoPadraoInvalido {
        modo: efetivo.clone(),
    })?;
    if !sessao_existe {
        return Err(NaoSubiu::SemSessao);
    }
    if !node_presente {
        return Err(NaoSubiu::SemNode);
    }
    Ok(modo)
}

/// Sobe o canal, quando ha o que subir.
///
/// `Err` com o motivo quando nao havia o que fazer — **nao e erro**: um gateway
/// sem WhatsApp vinculado e o caso comum.
///
/// # O que esta funcao deliberadamente NAO devolve
///
/// O `watch::Sender` de cancelamento. Ele e estacionado em
/// [`WhatsAppLinkedRuntime::reter_cancelamento`], que vive no `AppState`.
///
/// A versao anterior o devolvia, e o boot escrevia
/// `Ok(_cancel) => info!("canal supervisionado")`. O `_cancel` morria no fim do
/// braco do `match`, `watch::Receiver::changed()` passava a devolver `Err`, e o
/// primeiro braco do `select!` `biased` de `serve_once` trata `changed.is_err()`
/// como cancelamento: a cada boot o canal fazia spawn do node, handshake,
/// `session_load` e `start`, e entao a primeira iteracao do loop matava o filho
/// — em decimos de segundo, sem erro nenhum no log. O canal cuja razao de
/// existir e fazer a mensagem escaneada chegar ao agente nunca recebeu uma.
///
/// Devolver um handle cuja queda desliga o subsistema e um convite a esse
/// defeito, e nenhum teste do supervisor o pega, porque todo harness guarda o
/// `Sender`. Com o handle estacionado, a mutacao "soltar o cancelamento no
/// call-site" deixa de ser expressavel: nao ha o que soltar.
pub fn spawn_whatsapp_linked(state: &SharedState) -> Result<(), NaoSubiu> {
    let settings = settings_from_config(&state.config);
    let paths = LinkedPaths::from_config(&state.config).map_err(|_| NaoSubiu::SemSessao)?;
    let node = bridge::find_executable("node");
    // O perfil de execucao ja vem resolvido pelo loader (env > arquivo >
    // default); aqui ele so escolhe qual `default_mode` efetivo validar.
    let politica = politica_de_execucao(&state.config);

    let modo = deve_supervisionar(
        &settings,
        politica.perfil,
        paths.store.exists(),
        node.is_some(),
    )?;
    // Depois de decidir que sobe, e antes de subir: o aviso de drift (#1327),
    // sobre o modo que `deve_supervisionar` acabou de validar. Aviso, nao
    // recusa — ver `mcp_liberadas_pelo_perfil`.
    avisar_drift_de_mcp(modo, politica.perfil, &settings, &state.agents);
    // #1345: sobe assim mesmo (o `allow` recarrega a quente), mas diz que
    // ninguem vai receber resposta ate alguem ser autorizado.
    if let Some(aviso) = aviso_portao_vazio(
        &settings,
        &garraia_common::executavel::nome(),
        state.has_config_watcher(),
    ) {
        warn!("{aviso}");
    }
    // `deve_supervisionar` ja provou que ha `node`; o `else` existe porque o
    // compilador nao sabe disso, e um `unwrap()` em producao e proibido.
    let Some(node) = node else {
        return Err(NaoSubiu::SemNode);
    };

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

    let launcher: Arc<dyn bridge::BridgeLauncher> =
        Arc::new(NodeLauncher::new(node, paths.bridge_dir.clone()));

    supervisionar(state, settings, paths.store.clone(), key, launcher);
    Ok(())
}

/// A fiacao do supervisor, sem a descoberta do `node`.
///
/// Separada de [`spawn_whatsapp_linked`] por um motivo so: e o que permite a um
/// teste exercitar **esta** fiacao — a que o boot usa — contra a ponte falsa,
/// em vez de montar a sua propria e provar outra coisa. O que o boot faz a mais
/// e achar o `node` e abrir a chave; tudo o que vem depois esta aqui.
pub(crate) fn supervisionar(
    state: &SharedState,
    settings: LinkedSettings,
    store: SessionStore,
    key: SessionKey,
    launcher: Arc<dyn bridge::BridgeLauncher>,
) {
    let runtime = Arc::clone(&state.whatsapp_linked);
    runtime.set_bridge(BridgeView::NotStarted);

    let (outbound_tx, outbound_rx) = mpsc::channel(OUTBOUND_CAPACITY);
    // O `GatewaySink` monta o [`PortaoDoCanal`] a partir destes `settings`: as
    // identidades do `allow` ficam **neste** canal, e nao no `allowlist.json`
    // da instalacao. Ver o docblock de `PortaoDoCanal`.
    let sink: Arc<dyn InboundSink> = Arc::new(GatewaySink::new(
        Arc::clone(state),
        Arc::clone(&runtime),
        settings,
        outbound_tx,
    ));

    let (cancel_tx, cancel_rx) = watch::channel(false);

    let runtime_para_tarefa = Arc::clone(&runtime);
    tokio::spawn(async move {
        let resultado = serve(launcher, store, key, sink, outbound_rx, cancel_rx, || {
            rand::random::<f64>()
        })
        .await;
        runtime_para_tarefa.set_bridge(BridgeView::Down);
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

    // DEPOIS do spawn, e antes de sair: enquanto este `Sender` viver, o
    // supervisor vive. Ver o docblock de `WhatsAppLinkedRuntime`.
    runtime.reter_cancelamento(cancel_tx);
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
