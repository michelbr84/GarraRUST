//! As decisoes da ponte MCP HTTP, sem relogio, sem I/O e sem `AppState`.
//!
//! Mesmo recorte do `spinner.rs` da CLI e do `state.rs` do
//! `garraia-desktop-core`: o que e *decisao* mora aqui, puro e testavel a seco;
//! o que e *efeito* (ler `AppState`, alcancar um canal, responder JSON-RPC)
//! mora no [`super::handler`]. A separacao existe porque a decisao
//! interessante deste modulo — "este envio sai?" — e exatamente a que ninguem
//! quer ter de provar com um gateway de pe e um bot do Telegram do outro lado.

use garraia_config::AppConfig;
use garraia_config::defaults::DEFAULT_CLOUD_MODEL;

use super::ferramentas::{TOOL_SEND_MESSAGE, tool_por_nome_curto};
use crate::auth_common::constant_time_token_eq;
use crate::channel_send::ProactiveTargets;

/// O unico canal que `garra_send_message` alcanca na v1.
///
/// Nao e falta de vontade: a allowlist de destino que serve de aprovacao
/// out-of-band ([`ProactiveTargets`]) so existe para o Telegram
/// (`channels.<canal>.proactive_chat_ids`). Um canal sem allowlist nao tem como
/// dizer "o dono liberou este destino", e sem isso o envio nao teria aprovacao
/// nenhuma — entao ele fica de fora em vez de virar uma excecao.
pub const CANAL_DE_ENVIO: &str = "telegram";

/// A sessao contra a qual o teto de envios da ponte e cobrado.
///
/// Uma so, e nao uma por chamador: o [`crate::channel_send::SendBudget`] e
/// anti-amplificacao, e um orquestrador que pudesse escolher a chave do proprio
/// teto nao teria teto. Todo envio por MCP HTTP divide os mesmos 5 por minuto.
pub const SESSAO_DO_TETO: &str = "mcp-http";

/// A sessao contra a qual o teto de chamadas de `garra_ask` e cobrado (#1612).
///
/// Um orcamento **proprio**, separado do de envio: gastar inferencia nao pode
/// consumir a cota de mensagens do dono, nem o contrario. Mesmo raciocinio de
/// [`SESSAO_DO_TETO`]: uma so para a ponte inteira, para o chamador nao zerar o
/// teto abrindo outra sessao.
pub const SESSAO_DO_TETO_ASK: &str = "mcp-http-ask";

/// Quem fez uma chamada da ponte, pela credencial que ela trouxe (#1613).
///
/// `Dono` e o `gateway.api_key`, com a politica de sempre: os interruptores
/// globais e a allowlist de destino, sem restricao por orquestrador. `Nome` e
/// uma entrada de `gateway.mcp_http.orchestrators`, que so passa pelo que a
/// sua propria politica liberou **e** pelas travas globais.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Orquestrador {
    Dono,
    Nome(String),
}

impl Orquestrador {
    /// O rotulo que vai para o log de auditoria. Nunca o token.
    pub fn rotulo(&self) -> &str {
        match self {
            Self::Dono => "dono",
            Self::Nome(nome) => nome,
        }
    }
}

/// Descobre quem apresentou `apresentada`, comparando contra cada orquestrador
/// e, por ultimo, contra o `gateway.api_key` do dono (#1613).
///
/// A ordem e a da spec: um token de orquestrador vence. Se ele coincidir com o
/// do dono, quem chama fica com a politica do orquestrador — a coincidencia
/// ainda nao e recusada na config (follow-up). Token vazio ou que nao bate com
/// ninguem da `None`, e a chamada e recusada como `unauthorized` sem dizer
/// quais orquestradores existem.
///
/// Pura: recebe os pares `(nome, token)` ja lidos, para que o teste nao dependa
/// do ambiente.
pub fn identificar(
    orquestradores: Vec<(String, Option<String>)>,
    dono: Option<&str>,
    apresentada: &str,
) -> Option<Orquestrador> {
    if apresentada.is_empty() {
        return None;
    }
    let bate = |esperada: &str| constant_time_token_eq(apresentada.as_bytes(), esperada.as_bytes());
    for (nome, token) in orquestradores {
        if token.as_deref().is_some_and(bate) {
            return Some(Orquestrador::Nome(nome));
        }
    }
    dono.filter(|d| bate(d)).map(|_| Orquestrador::Dono)
}

/// A politica de um orquestrador nomeado: o que ele pode chamar e para onde.
///
/// Sem o token. A identificacao le o ambiente na hora de cada chamada, e a
/// politica nunca guarda segredo — um `Debug` dela nao pode vazar nada.
#[derive(Debug, Clone)]
struct Concessao {
    nome: String,
    /// Nomes `garra_*` das tools liberadas, ja traduzidos de `tools`.
    ferramentas: Vec<&'static str>,
    /// Destinos declarados para este orquestrador. Ainda passam pela allowlist
    /// global: a interseccao e a regra.
    chats: Vec<i64>,
}

/// Por que uma chamada foi barrada por causa de quem a fez (#1613).
///
/// Separado de [`Recusa`] e de [`RecusaAsk`]: estas sao travas globais, e esta
/// e a trava de identidade. Mesmo formato, com codigo estavel e explicacao que
/// diz o passo que destrava.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecusaOrquestrador {
    /// A credencial nao identifica nenhum orquestrador configurado.
    NaoAutorizado,
    /// A tool existe, mas nao esta na lista deste orquestrador.
    ToolNaoPermitida,
    /// O chat nao esta na lista deste orquestrador.
    DestinoNaoPermitido,
}

impl RecusaOrquestrador {
    /// O motivo estavel, para o `error.kind` do envelope e para o log.
    pub fn codigo(self) -> &'static str {
        match self {
            Self::NaoAutorizado => "unauthorized",
            Self::ToolNaoPermitida => "tool_not_allowed",
            Self::DestinoNaoPermitido => "destination_not_allowed",
        }
    }

    /// A explicacao que o orquestrador le. Nenhuma ecoa destino nem nome de
    /// outro orquestrador.
    pub fn explicacao(self) -> &'static str {
        match self {
            Self::NaoAutorizado => {
                "a credencial nao identifica nenhum orquestrador deste Garra. Confira o \
                 token com o operador; ele e configurado em `gateway.mcp_http.orchestrators`."
            }
            Self::ToolNaoPermitida => {
                "esta tool nao esta liberada para este orquestrador. O operador a libera em \
                 `gateway.mcp_http.orchestrators[].tools`. Nao ha como liberar pela propria \
                 ponte."
            }
            Self::DestinoNaoPermitido => {
                "este destino nao esta liberado para este orquestrador. O operador o lista \
                 em `gateway.mcp_http.orchestrators[].chats`. Nao ha como aprovar pela \
                 propria ponte."
            }
        }
    }
}

/// A politica viva da ponte, derivada da config a cada chamada.
///
/// Derivada por chamada e nao lida uma vez no boot, pelo mesmo motivo que o
/// `telegram_send` le a allowlist por chamada (auditoria do #921): o operador
/// que tira um destino da lista — ou que desliga `allow_send` — espera que
/// aquilo valha agora, nao no proximo restart. A janela em que um destino
/// revogado ainda funciona e precisamente a coisa errada de se ter.
#[derive(Debug, Clone)]
pub struct PoliticaMcpHttp {
    /// `gateway.mcp_http.allow_send`.
    envio_liberado: bool,
    /// Destinos que o operador liberou, de `proactive_chat_ids`.
    destinos: ProactiveTargets,
    /// Teto de mensagens por chamada de `garra_read_history`.
    teto_do_historico: usize,
    /// `gateway.mcp_http.allow_ask` (#1612).
    ask_liberado: bool,
    /// `gateway.mcp_http.ask_allowed_models`. Vazio = so o modelo default do
    /// projeto e aceito (ver [`PoliticaMcpHttp::decidir_ask`]).
    modelos_de_ask: Vec<String>,
    /// Os orquestradores validos de `gateway.mcp_http.orchestrators` (#1613).
    orquestradores: Vec<Concessao>,
}

/// Por que um envio nao saiu.
///
/// Enum e nao `String` porque cada variante e um teste: a mensagem que chega ao
/// chamador pode mudar de redacao sem que a suite pare de provar *qual* trava
/// segurou o envio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recusa {
    /// `gateway.mcp_http.allow_send` esta em `false` (o default).
    InterruptorDesligado,
    /// O interruptor esta ligado, mas nenhum destino foi liberado. O estado de
    /// quem ligou a chave e esqueceu a allowlist — e que nao envia nada.
    SemAllowlist,
    /// Ha allowlist, e este destino nao esta nela.
    ForaDaAllowlist,
    /// Canal diferente de [`CANAL_DE_ENVIO`].
    CanalNaoSuportado,
}

/// Por que uma chamada de `garra_ask` nao rodou (#1612).
///
/// Mesmo formato de [`Recusa`]: enum com codigo estavel, nunca `String`, e a
/// explicacao diz o passo que destrava. Nenhuma variante ecoa o modelo pedido.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecusaAsk {
    /// `gateway.mcp_http.allow_ask` esta em `false` (o default).
    InterruptorDesligado,
    /// O modelo pedido nao esta permitido pela politica do operador.
    ModeloNaoPermitido,
    /// O teto por minuto de `garra_ask` foi atingido.
    OrcamentoEsgotado,
}

impl RecusaAsk {
    /// O motivo estavel, para o `error.kind` do envelope e para o log.
    pub fn codigo(self) -> &'static str {
        match self {
            Self::InterruptorDesligado => "ask_disabled",
            Self::ModeloNaoPermitido => "model_not_allowed",
            Self::OrcamentoEsgotado => "over_budget",
        }
    }

    /// A explicacao que o orquestrador le, com o passo que destrava.
    pub fn explicacao(self) -> &'static str {
        match self {
            Self::InterruptorDesligado => {
                "garra_ask esta desligado neste Garra. O operador precisa por \
                 `gateway.mcp_http.allow_ask: true` na config e reiniciar. Nao ha como \
                 aprovar isso pela propria ponte."
            }
            Self::ModeloNaoPermitido => {
                "esse modelo nao e permitido neste Garra. Sem lista na config, so o \
                 modelo default do projeto e aceito; com lista, so os modelos listados em \
                 `gateway.mcp_http.ask_allowed_models`. Trocar de modelo nao o libera: so \
                 o operador pode, na config."
            }
            Self::OrcamentoEsgotado => {
                "limite de chamadas de garra_ask por minuto atingido. Espere a janela \
                 fechar, ou junte o que falta perguntar numa unica chamada. O teto e \
                 `gateway.mcp_http.ask_budget_per_minute`."
            }
        }
    }
}

impl Recusa {
    /// O motivo estavel, para o campo `reason` da resposta e para o log.
    ///
    /// Nunca leva o `chat_id`: um destino recusado ainda e a conversa de uma
    /// pessoa real, e ela nao tem por que aparecer no log de um pedido que veio
    /// de fora.
    pub fn codigo(self) -> &'static str {
        match self {
            Self::InterruptorDesligado => "send_disabled",
            Self::SemAllowlist => "no_allowlist",
            Self::ForaDaAllowlist => "target_not_allowed",
            Self::CanalNaoSuportado => "channel_unsupported",
        }
    }

    /// A explicacao que o orquestrador le — com o passo que destrava, quando
    /// existe um. Um "nao" sem o proximo passo faz o chamador tentar de novo.
    pub fn explicacao(self) -> &'static str {
        match self {
            Self::InterruptorDesligado => {
                "envio por MCP esta desligado neste Garra. O operador precisa por \
                 `gateway.mcp_http.allow_send: true` na config e reiniciar. Nao ha \
                 como aprovar isso pela propria ponte."
            }
            Self::SemAllowlist => {
                "nenhum destino foi liberado pelo operador. Ele precisa listar os chats \
                 em `channels.<canal>.proactive_chat_ids` — `allow_send` sozinho nao envia."
            }
            Self::ForaDaAllowlist => {
                "esse destino nao esta em `proactive_chat_ids`. So o operador pode \
                 adiciona-lo, na config."
            }
            Self::CanalNaoSuportado => {
                "so o canal `telegram` tem allowlist de destino neste Garra, entao e o \
                 unico que a ponte MCP alcanca."
            }
        }
    }
}

impl PoliticaMcpHttp {
    /// Le a politica da config viva.
    pub fn da_config(config: &AppConfig) -> Self {
        Self {
            envio_liberado: config.gateway.mcp_http.allow_send,
            destinos: ProactiveTargets::from_config(config),
            teto_do_historico: config.gateway.mcp_http.max_history_messages,
            ask_liberado: config.gateway.mcp_http.allow_ask,
            modelos_de_ask: config.gateway.mcp_http.ask_allowed_models.clone(),
            orquestradores: config
                .gateway
                .mcp_http
                .orquestradores_validos()
                .into_iter()
                .map(|o| Concessao {
                    nome: o.nome.clone(),
                    ferramentas: o
                        .tools
                        .iter()
                        .filter_map(|t| tool_por_nome_curto(t))
                        .collect(),
                    chats: o.chats.clone(),
                })
                .collect(),
        }
    }

    /// Construtor puro para os testes, que assim nunca montam um `AppConfig`
    /// inteiro so para exercitar uma trava.
    #[cfg(test)]
    pub(crate) fn nova(envio_liberado: bool, destinos: ProactiveTargets) -> Self {
        Self {
            envio_liberado,
            destinos,
            teto_do_historico: 50,
            ask_liberado: false,
            modelos_de_ask: Vec::new(),
            orquestradores: Vec::new(),
        }
    }

    /// Liga `garra_ask` com a lista de modelos dada, para os testes.
    #[cfg(test)]
    pub(crate) fn com_ask(mut self, modelos: &[&str]) -> Self {
        self.ask_liberado = true;
        self.modelos_de_ask = modelos.iter().map(|m| m.to_string()).collect();
        self
    }

    /// Acrescenta um orquestrador nomeado, para os testes.
    #[cfg(test)]
    pub(crate) fn com_orquestrador(
        mut self,
        nome: &str,
        ferramentas: &[&str],
        chats: &[i64],
    ) -> Self {
        self.orquestradores.push(Concessao {
            nome: nome.to_string(),
            ferramentas: ferramentas
                .iter()
                .filter_map(|t| tool_por_nome_curto(t))
                .collect(),
            chats: chats.to_vec(),
        });
        self
    }

    /// A concessao de um orquestrador nomeado. `None` para o dono (que nao tem
    /// concessao: a politica dele e a global) e para um nome desconhecido.
    fn concessao(&self, quem: &Orquestrador) -> Option<&Concessao> {
        match quem {
            Orquestrador::Dono => None,
            Orquestrador::Nome(nome) => self.orquestradores.iter().find(|c| &c.nome == nome),
        }
    }

    /// Esta chamada, feita por `quem`, pode usar esta tool? (#1613)
    ///
    /// Dono: sempre (a trava dele e a global de cada tool). Nome desconhecido
    /// e recusado como `unauthorized`: o handler so chega aqui com um nome que
    /// ele mesmo identificou, entao isto e cinto e suspensorio.
    pub fn decidir_ferramenta(
        &self,
        quem: &Orquestrador,
        ferramenta: &str,
    ) -> Result<(), RecusaOrquestrador> {
        if *quem == Orquestrador::Dono {
            return Ok(());
        }
        let concessao = self
            .concessao(quem)
            .ok_or(RecusaOrquestrador::NaoAutorizado)?;
        if concessao.ferramentas.contains(&ferramenta) {
            Ok(())
        } else {
            Err(RecusaOrquestrador::ToolNaoPermitida)
        }
    }

    /// Este destino esta na lista de `quem`? (#1613)
    ///
    /// E a **primeira** trava do envio, e de proposito vem antes da global:
    /// um orquestrador so ve "nao liberado" para qualquer chat que nao seja
    /// dele, e nao aprende nada sobre a allowlist do operador. A segunda trava
    /// (a global) continua valendo depois — a interseccao manda.
    pub fn decidir_destino_de(
        &self,
        quem: &Orquestrador,
        chat_id: i64,
    ) -> Result<(), RecusaOrquestrador> {
        if *quem == Orquestrador::Dono {
            return Ok(());
        }
        let concessao = self
            .concessao(quem)
            .ok_or(RecusaOrquestrador::NaoAutorizado)?;
        if concessao.chats.contains(&chat_id) {
            Ok(())
        } else {
            Err(RecusaOrquestrador::DestinoNaoPermitido)
        }
    }

    /// A tool sai na lista de `tools/list` para `quem`? (#1613)
    ///
    /// Uma tool que `quem` nao pode chamar nao e anunciada, pelo mesmo motivo
    /// de a de envio nao ser anunciada quando o envio nao sairia: o modelo do
    /// outro lado nao planeja em cima de uma capacidade que nao tem. O envio
    /// exige ainda um destino da sua lista que esteja **tambem** na global.
    pub fn anuncia(&self, quem: &Orquestrador, ferramenta: &str) -> bool {
        if self.decidir_ferramenta(quem, ferramenta).is_err() {
            return false;
        }
        if ferramenta == TOOL_SEND_MESSAGE {
            return self.anuncia_envio_para(quem);
        }
        true
    }

    /// `garra_send_message` tem um destino que `quem` pode usar de fato?
    fn anuncia_envio_para(&self, quem: &Orquestrador) -> bool {
        if !self.anuncia_envio() {
            return false;
        }
        match quem {
            Orquestrador::Dono => true,
            Orquestrador::Nome(_) => self
                .concessao(quem)
                .is_some_and(|c| c.chats.iter().any(|&id| self.destinos.allows(id))),
        }
    }

    /// `garra_send_message` esta na superficie anunciada?
    ///
    /// **Uma tool que nao pode enviar nao e anunciada.** Um `tools/list` que
    /// mostra `garra_send_message` num Garra que vai recusar todo envio faz o
    /// modelo do outro lado planejar em cima de uma capacidade que nao existe —
    /// e depois interpretar a recusa como falha transitoria e tentar de novo.
    ///
    /// Note que isto e so o *anuncio*: a recusa em [`Self::decidir_envio`] nao
    /// depende dele. Nao existe "desligada mas chamavel".
    pub fn anuncia_envio(&self) -> bool {
        self.envio_liberado && !self.destinos.is_empty()
    }

    /// Este envio sai?
    ///
    /// Fail-closed em ordem: interruptor, canal, existencia da allowlist,
    /// pertinencia a ela. O primeiro "nao" e o que volta — e de proposito o
    /// mais generico primeiro, para que a resposta nao vire um oraculo de
    /// pertinencia (com o interruptor desligado, todo destino da a mesma
    /// resposta, e o chamador nao descobre quem esta na lista).
    pub fn decidir_envio(&self, canal: &str, chat_id: i64) -> Result<(), Recusa> {
        if !self.envio_liberado {
            return Err(Recusa::InterruptorDesligado);
        }
        if canal != CANAL_DE_ENVIO {
            return Err(Recusa::CanalNaoSuportado);
        }
        if self.destinos.is_empty() {
            return Err(Recusa::SemAllowlist);
        }
        if !self.destinos.allows(chat_id) {
            return Err(Recusa::ForaDaAllowlist);
        }
        Ok(())
    }

    /// Quantas mensagens `garra_read_history` devolve, dado o que o chamador
    /// pediu. Pedido ausente ou acima do teto viram o teto; `0` tambem, porque
    /// um historico vazio por pedido do chamador nao e resposta util.
    pub fn limite_do_historico(&self, pedido: Option<usize>) -> usize {
        let teto = self.teto_do_historico.max(1);
        match pedido {
            Some(n) if n > 0 => n.min(teto),
            _ => teto,
        }
    }

    /// Quantos destinos o operador liberou. So para diagnostico — os ids nunca
    /// saem daqui.
    pub fn destinos_liberados(&self) -> usize {
        self.destinos.len()
    }

    /// `garra_ask` esta na superficie anunciada? (#1612)
    ///
    /// Igual ao envio: so o interruptor decide o anuncio. A lista de modelos
    /// nao entra aqui, porque o default (sem lista) ja e um modelo aceito.
    pub fn anuncia_ask(&self) -> bool {
        self.ask_liberado
    }

    /// Esta chamada de `garra_ask` pode rodar com este modelo? (#1612)
    ///
    /// Fail-closed em ordem: interruptor e depois o modelo. Sem lista, so o
    /// default do projeto passa; com lista, so o que estiver nela. O modelo
    /// vem do chamador, e o teto de chamadas e cobrado pelo handler **depois**
    /// de esta decisao aprovar — o mesmo motivo do envio: uma recusa que gastasse
    /// cota deixaria um chamador sondando modelos trancar o dono fora.
    pub fn decidir_ask(&self, modelo: &str) -> Result<(), RecusaAsk> {
        if !self.ask_liberado {
            return Err(RecusaAsk::InterruptorDesligado);
        }
        let aceito = if self.modelos_de_ask.is_empty() {
            modelo == DEFAULT_CLOUD_MODEL
        } else {
            self.modelos_de_ask.iter().any(|m| m == modelo)
        };
        if !aceito {
            return Err(RecusaAsk::ModeloNaoPermitido);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp_http::ferramentas::{TOOL_ASK, TOOL_SEND_MESSAGE, TOOL_STATUS};
    use garraia_config::OrchestratorMcpHttp;

    fn com_destino(envio: bool, ids: &[i64]) -> PoliticaMcpHttp {
        PoliticaMcpHttp::nova(envio, ProactiveTargets::from_ids(ids.iter().copied()))
    }

    /// O default da instalacao: nada ligado, nada liberado. Nenhum envio sai, e
    /// o motivo e o interruptor — nao a allowlist.
    #[test]
    fn default_recusa_pelo_interruptor() {
        let p = com_destino(false, &[]);
        assert_eq!(
            p.decidir_envio(CANAL_DE_ENVIO, 42),
            Err(Recusa::InterruptorDesligado)
        );
        assert!(!p.anuncia_envio());
    }

    /// O interruptor ligado sem allowlist e o erro de configuracao mais
    /// provavel, e ele **nao** envia. E a metade "aprovacao" do gate: a chave
    /// destrava a tool, a lista aprova o destino.
    #[test]
    fn interruptor_ligado_sem_allowlist_nao_envia() {
        let p = com_destino(true, &[]);
        assert_eq!(
            p.decidir_envio(CANAL_DE_ENVIO, 42),
            Err(Recusa::SemAllowlist)
        );
        assert!(!p.anuncia_envio());
    }

    #[test]
    fn destino_fora_da_allowlist_nao_envia() {
        let p = com_destino(true, &[7]);
        assert_eq!(
            p.decidir_envio(CANAL_DE_ENVIO, 42),
            Err(Recusa::ForaDaAllowlist)
        );
        // Mas a tool existe: ha destino liberado, so nao esse.
        assert!(p.anuncia_envio());
    }

    #[test]
    fn destino_liberado_envia() {
        let p = com_destino(true, &[42]);
        assert_eq!(p.decidir_envio(CANAL_DE_ENVIO, 42), Ok(()));
        assert!(p.anuncia_envio());
    }

    /// Um canal sem allowlist nao herda a do Telegram.
    #[test]
    fn canal_sem_allowlist_e_recusado_mesmo_com_destino_liberado() {
        let p = com_destino(true, &[42]);
        assert_eq!(
            p.decidir_envio("discord", 42),
            Err(Recusa::CanalNaoSuportado)
        );
    }

    /// Com o interruptor desligado, destino liberado e destino desconhecido dao
    /// a MESMA resposta. E o que impede a recusa de virar consulta a allowlist.
    #[test]
    fn interruptor_desligado_nao_distingue_destino_liberado_de_estranho() {
        let p = com_destino(false, &[42]);
        assert_eq!(
            p.decidir_envio(CANAL_DE_ENVIO, 42),
            p.decidir_envio(CANAL_DE_ENVIO, 999)
        );
    }

    #[test]
    fn limite_do_historico_respeita_teto_e_pedido() {
        let p = com_destino(false, &[]);
        assert_eq!(p.limite_do_historico(None), 50, "ausente = teto");
        assert_eq!(p.limite_do_historico(Some(0)), 50, "zero = teto");
        assert_eq!(p.limite_do_historico(Some(10)), 10);
        assert_eq!(
            p.limite_do_historico(Some(5_000)),
            50,
            "acima do teto = teto"
        );
    }

    /// Cada recusa tem codigo proprio: o chamador distingue "o operador nao
    /// ligou" de "este destino nao vale" sem ler prosa.
    #[test]
    fn cada_recusa_tem_codigo_distinto() {
        let codigos: Vec<&str> = [
            Recusa::InterruptorDesligado,
            Recusa::SemAllowlist,
            Recusa::ForaDaAllowlist,
            Recusa::CanalNaoSuportado,
        ]
        .iter()
        .map(|r| r.codigo())
        .collect();
        let unicos: std::collections::BTreeSet<&str> = codigos.iter().copied().collect();
        assert_eq!(unicos.len(), codigos.len(), "{codigos:?}");
    }

    /// A explicacao nunca ecoa um destino. Ela e `&'static str`, entao isto e
    /// uma guarda contra quem trocar o tipo por `String` formatada mais tarde.
    #[test]
    fn explicacao_nao_carrega_destino() {
        for r in [
            Recusa::InterruptorDesligado,
            Recusa::SemAllowlist,
            Recusa::ForaDaAllowlist,
            Recusa::CanalNaoSuportado,
        ] {
            let texto = r.explicacao();
            assert!(!texto.contains("42"), "{texto}");
            assert!(!texto.is_empty());
        }
    }

    /// #1612 — o default da instalacao nao anuncia `garra_ask` e nao aceita
    /// nenhum modelo: o interruptor vem antes da lista.
    #[test]
    fn ask_default_recusa_pelo_interruptor() {
        let p = com_destino(false, &[]);
        assert!(!p.anuncia_ask());
        assert_eq!(
            p.decidir_ask(DEFAULT_CLOUD_MODEL),
            Err(RecusaAsk::InterruptorDesligado)
        );
    }

    /// Ligado sem lista, so o modelo default do projeto passa. Qualquer outro
    /// modelo — mesmo um barato — e recusado: a lista vazia e o default
    /// conservador.
    #[test]
    fn ask_sem_lista_aceita_so_o_default_do_projeto() {
        let p = com_destino(false, &[]).com_ask(&[]);
        assert!(p.anuncia_ask());
        assert_eq!(p.decidir_ask(DEFAULT_CLOUD_MODEL), Ok(()));
        assert_eq!(
            p.decidir_ask("openrouter/auto"),
            Err(RecusaAsk::ModeloNaoPermitido)
        );
        assert_eq!(
            p.decidir_ask("openrouter/free"),
            Err(RecusaAsk::ModeloNaoPermitido)
        );
    }

    /// A lista **substitui** o default: se o operador listou outro modelo, o
    /// default do projeto deixa de passar, e so o listado passa.
    #[test]
    fn ask_com_lista_substitui_o_default() {
        let p = com_destino(false, &[]).com_ask(&["openrouter/auto"]);
        assert_eq!(p.decidir_ask("openrouter/auto"), Ok(()));
        assert_eq!(
            p.decidir_ask(DEFAULT_CLOUD_MODEL),
            Err(RecusaAsk::ModeloNaoPermitido)
        );
    }

    /// A recusa de modelo nao depende do canal de envio: `allow_send` nao
    /// destrava `garra_ask` e `allow_ask` nao destrava envio.
    #[test]
    fn ask_e_envio_sao_interruptores_independentes() {
        let so_envio = com_destino(true, &[42]);
        assert!(!so_envio.anuncia_ask());
        assert_eq!(
            so_envio.decidir_ask(DEFAULT_CLOUD_MODEL),
            Err(RecusaAsk::InterruptorDesligado)
        );

        let so_ask = com_destino(false, &[]).com_ask(&[]);
        assert_eq!(
            so_ask.decidir_envio(CANAL_DE_ENVIO, 42),
            Err(Recusa::InterruptorDesligado)
        );
    }

    /// Cada recusa de `garra_ask` tem codigo proprio e estavel.
    #[test]
    fn cada_recusa_de_ask_tem_codigo_distinto() {
        let codigos: Vec<&str> = [
            RecusaAsk::InterruptorDesligado,
            RecusaAsk::ModeloNaoPermitido,
            RecusaAsk::OrcamentoEsgotado,
        ]
        .iter()
        .map(|r| r.codigo())
        .collect();
        assert_eq!(
            codigos,
            vec!["ask_disabled", "model_not_allowed", "over_budget"]
        );
    }

    /// A explicacao de ask nunca ecoa um modelo nem um numero de destino.
    #[test]
    fn explicacao_de_ask_nao_ecoa_o_modelo_pedido() {
        for r in [
            RecusaAsk::InterruptorDesligado,
            RecusaAsk::ModeloNaoPermitido,
            RecusaAsk::OrcamentoEsgotado,
        ] {
            let texto = r.explicacao();
            assert!(!texto.is_empty());
            assert!(!texto.contains("openrouter/auto"), "{texto}");
        }
    }

    /// Com `allow_ask` ligado na config, a politica viva le o interruptor e a
    /// lista (e o default continua desligado).
    #[test]
    fn da_config_le_allow_ask_e_a_lista() {
        let padrao = PoliticaMcpHttp::da_config(&AppConfig::default());
        assert!(!padrao.anuncia_ask());

        let mut config = AppConfig::default();
        config.gateway.mcp_http.allow_ask = true;
        config.gateway.mcp_http.ask_allowed_models = vec!["openrouter/auto".to_string()];
        let p = PoliticaMcpHttp::da_config(&config);
        assert!(p.anuncia_ask());
        assert_eq!(p.decidir_ask("openrouter/auto"), Ok(()));
        assert_eq!(
            p.decidir_ask(DEFAULT_CLOUD_MODEL),
            Err(RecusaAsk::ModeloNaoPermitido)
        );
    }

    /// A politica sai da config de verdade, incluindo o default desligado.
    #[test]
    fn da_config_le_o_default_desligado() {
        let p = PoliticaMcpHttp::da_config(&AppConfig::default());
        assert!(!p.anuncia_envio());
        assert_eq!(p.destinos_liberados(), 0);
        assert_eq!(
            p.decidir_envio(CANAL_DE_ENVIO, 1),
            Err(Recusa::InterruptorDesligado)
        );
    }

    fn orq(nome: &str) -> Orquestrador {
        Orquestrador::Nome(nome.to_string())
    }

    fn pares(nome: &str, token: Option<&str>) -> Vec<(String, Option<String>)> {
        vec![(nome.to_string(), token.map(str::to_string))]
    }

    /// #1613 — o token identifica o orquestrador, o do dono identifica o dono,
    /// e qualquer outro (ou vazio) nao identifica ninguem.
    #[test]
    fn identifica_orquestrador_dono_e_desconhecido() {
        let lista = || {
            let mut v = pares("badgood", Some("tok-badgood"));
            v.push(("sem-env".to_string(), None));
            v
        };
        assert_eq!(
            identificar(lista(), Some("tok-dono"), "tok-badgood"),
            Some(orq("badgood"))
        );
        assert_eq!(
            identificar(lista(), Some("tok-dono"), "tok-dono"),
            Some(Orquestrador::Dono)
        );
        assert_eq!(identificar(lista(), Some("tok-dono"), "tok-estranho"), None);
        assert_eq!(identificar(lista(), Some("tok-dono"), ""), None);
        assert_eq!(identificar(lista(), None, "tok-dono"), None);
    }

    /// Se o token de um orquestrador coincidir com o do dono, o orquestrador
    /// vence: e a ordem da spec, e o teste fixa a ordem para nao mudar sem aviso.
    #[test]
    fn orquestrador_vence_o_dono_numa_coincidencia() {
        assert_eq!(
            identificar(pares("x", Some("igual")), Some("igual"), "igual"),
            Some(orq("x"))
        );
    }

    /// Nome que nao esta na politica nao e o dono: e `NaoAutorizado`.
    #[test]
    fn orquestrador_fora_da_politica_e_nao_autorizado() {
        let p = com_destino(false, &[]).com_orquestrador("badgood", &["status"], &[]);
        assert_eq!(
            p.decidir_ferramenta(&orq("fantasma"), TOOL_STATUS),
            Err(RecusaOrquestrador::NaoAutorizado)
        );
        assert_eq!(
            p.decidir_destino_de(&orq("fantasma"), 1),
            Err(RecusaOrquestrador::NaoAutorizado)
        );
    }

    /// O dono nao tem restricao por orquestrador: a politica dele e a global.
    #[test]
    fn dono_nao_e_restringido_pela_politica_de_orquestrador() {
        let p = com_destino(false, &[]).com_orquestrador("badgood", &["status"], &[42]);
        assert_eq!(p.decidir_ferramenta(&Orquestrador::Dono, TOOL_ASK), Ok(()));
        assert_eq!(p.decidir_destino_de(&Orquestrador::Dono, 999), Ok(()));
    }

    /// A tool so passa se estiver na lista dele. Uma tool liberada a outro
    /// orquestrador nao vale para este.
    #[test]
    fn tool_fora_da_lista_do_orquestrador_e_recusada() {
        let p = com_destino(false, &[])
            .com_orquestrador("badgood", &["status", "send_message"], &[])
            .com_orquestrador("outro", &["ask"], &[]);
        assert_eq!(p.decidir_ferramenta(&orq("badgood"), TOOL_STATUS), Ok(()));
        assert_eq!(
            p.decidir_ferramenta(&orq("badgood"), TOOL_ASK),
            Err(RecusaOrquestrador::ToolNaoPermitida)
        );
        assert_eq!(
            p.decidir_ferramenta(&orq("outro"), TOOL_STATUS),
            Err(RecusaOrquestrador::ToolNaoPermitida)
        );
    }

    /// A interseccao: o chat precisa estar na lista do orquestrador E na
    /// global. Estar so numa das duas nao envia.
    #[test]
    fn destino_exige_a_interseccao_das_duas_listas() {
        let p = com_destino(true, &[7]).com_orquestrador("badgood", &["send_message"], &[42]);
        // Na lista dele e na global: passa nas duas.
        assert_eq!(p.decidir_destino_de(&orq("badgood"), 42), Ok(()));
        assert_eq!(
            p.decidir_envio(CANAL_DE_ENVIO, 42),
            Err(Recusa::ForaDaAllowlist),
            "a global nao conhece o 42"
        );
        // Na global, mas fora da lista dele: barrado pela primeira trava.
        assert_eq!(
            p.decidir_destino_de(&orq("badgood"), 7),
            Err(RecusaOrquestrador::DestinoNaoPermitido)
        );
    }

    /// Com a primeira trava recusando, a resposta nao depende da global: um
    /// orquestrador nao aprende a allowlist do operador.
    #[test]
    fn destino_recusado_pelo_orquestrador_nao_revela_a_global() {
        let p = com_destino(true, &[7]).com_orquestrador("badgood", &["send_message"], &[42]);
        assert_eq!(
            p.decidir_destino_de(&orq("badgood"), 7),
            p.decidir_destino_de(&orq("badgood"), 999),
            "um chat da global e um chat estranho devem dar a mesma resposta"
        );
    }

    /// `tools/list` por orquestrador: a tool de envio so aparece com tool
    /// liberada E com um destino em comum com a global.
    #[test]
    fn envio_so_e_anunciado_com_tool_e_destino_em_comum() {
        let sem_interseccao =
            com_destino(true, &[7]).com_orquestrador("badgood", &["send_message"], &[42]);
        assert!(!sem_interseccao.anuncia(&orq("badgood"), TOOL_SEND_MESSAGE));

        let com_interseccao =
            com_destino(true, &[42]).com_orquestrador("badgood", &["send_message"], &[42]);
        assert!(com_interseccao.anuncia(&orq("badgood"), TOOL_SEND_MESSAGE));

        let sem_a_tool = com_destino(true, &[42]).com_orquestrador("badgood", &["status"], &[42]);
        assert!(!sem_a_tool.anuncia(&orq("badgood"), TOOL_SEND_MESSAGE));
        assert!(sem_a_tool.anuncia(&orq("badgood"), TOOL_STATUS));
    }

    /// Sem entradas na config, o anuncio do dono e o de antes, byte a byte.
    #[test]
    fn dono_com_config_sem_orquestradores_mantem_o_anuncio() {
        let p = com_destino(true, &[42]);
        assert!(p.anuncia(&Orquestrador::Dono, TOOL_SEND_MESSAGE));
        assert!(p.anuncia(&Orquestrador::Dono, TOOL_STATUS));
        assert_eq!(
            p.anuncia(&Orquestrador::Dono, TOOL_SEND_MESSAGE),
            p.anuncia_envio()
        );
    }

    /// As tools da config viram os nomes `garra_*`; um nome que a ponte nao
    /// tem nao concede nada, em silencio.
    #[test]
    fn da_config_traduz_as_tools_e_ignora_o_que_nao_existe() {
        let mut config = AppConfig::default();
        config.gateway.mcp_http.orchestrators = vec![OrchestratorMcpHttp {
            nome: "badgood".into(),
            key_env: "BADGOOD_MCP_KEY".into(),
            tools: vec!["status".into(), "tool_inventada".into()],
            chats: vec![42],
        }];
        let p = PoliticaMcpHttp::da_config(&config);
        assert_eq!(p.decidir_ferramenta(&orq("badgood"), TOOL_STATUS), Ok(()));
        assert_eq!(
            p.decidir_ferramenta(&orq("badgood"), TOOL_ASK),
            Err(RecusaOrquestrador::ToolNaoPermitida)
        );
    }

    /// Entrada com nome repetido nao entra na politica: quem a usasse seria
    /// atribuido a uma entrada que nao escolheu.
    #[test]
    fn da_config_nao_concede_por_entrada_invalida() {
        let mut config = AppConfig::default();
        let entrada = || OrchestratorMcpHttp {
            nome: "badgood".into(),
            key_env: "BADGOOD_MCP_KEY".into(),
            tools: vec!["status".into()],
            chats: vec![],
        };
        config.gateway.mcp_http.orchestrators = vec![entrada(), entrada()];
        let p = PoliticaMcpHttp::da_config(&config);
        assert_eq!(
            p.decidir_ferramenta(&orq("badgood"), TOOL_STATUS),
            Err(RecusaOrquestrador::NaoAutorizado)
        );
    }

    /// Codigos estaveis e distintos, e a explicacao nunca ecoa destino.
    #[test]
    fn recusas_de_orquestrador_tem_codigo_estavel_e_nao_ecoam() {
        let recusas = [
            RecusaOrquestrador::NaoAutorizado,
            RecusaOrquestrador::ToolNaoPermitida,
            RecusaOrquestrador::DestinoNaoPermitido,
        ];
        let codigos: Vec<&str> = recusas.iter().map(|r| r.codigo()).collect();
        assert_eq!(
            codigos,
            vec![
                "unauthorized",
                "tool_not_allowed",
                "destination_not_allowed"
            ]
        );
        for r in recusas {
            assert!(!r.explicacao().is_empty());
            assert!(!r.explicacao().contains("42"), "{}", r.explicacao());
        }
    }

    #[test]
    fn rotulo_do_dono_e_o_reservado() {
        assert_eq!(Orquestrador::Dono.rotulo(), "dono");
        assert_eq!(orq("badgood").rotulo(), "badgood");
    }
}
