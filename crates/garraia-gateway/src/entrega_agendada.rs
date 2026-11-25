//! Entrega ativa da resposta de tarefa agendada (#2002).
//!
//! O executor (`execute_scheduled_task_inner`) grava a resposta no historico
//! da sessao SEMPRE; este modulo decide se ela tambem e empurrada ao canal, e
//! devolve o desfecho para o ledger de runs (`agent_runs.delivery`) — quem le
//! `garraia runs` distingue resposta entregue de resposta so persistida.
//!
//! # Onde ha entrega
//!
//! - **Telegram**: o endereco vem de `chat_session_keys` (#921), como antes.
//! - **Discord, Slack, Matrix**: o id da sessao JA e o endereco
//!   (`discord-{canal}`, `slack-{canal}`, `matrix-{sala}`, montado pelo
//!   proprio canal no `bootstrap`). Vale so quando o `channel_id` gravado na
//!   sessao e o prefixo do id concordam: o `channel_id` e o ULTIMO a gravar a
//!   linha, e um cliente HTTP escolhe o id da sessao (`X-Session-Id`) — nem
//!   um nem outro sozinho prova de onde a conversa veio.
//! - **WhatsApp vinculado**: pela ponte do canal (a saida mora no
//!   [`crate::bootstrap::WhatsAppLinkedRuntime`]), com tres regras proprias —
//!   ver abaixo.
//! - **IRC e iMessage ficam fora**, de proposito: o IRC manda o texto cru ao
//!   servidor sem quebrar linha (`IrcChannel::send_privmsg`), e uma resposta
//!   do modelo com quebra de linha viraria comando IRC; e as mensagens
//!   privadas de todo mundo caem numa sessao so (`irc-<nick do bot>`). No
//!   iMessage a sessao de grupo guarda o NOME do grupo, que nao e endereco.
//!   Os dois continuam como estavam: o adaptador e chamado sem endereco e
//!   recusa, e o ledger registra `nao_entregue:falhou`.
//!
//! # WhatsApp vinculado: as tres decisoes do canal
//!
//! O envio do canal era sempre RESPOSTA a uma mensagem recebida; a tarefa
//! agendada e o primeiro envio proativo, e as tres protecoes reativas foram
//! revistas para ele:
//!
//! 1. **Cadencia.** A guarda de turnos (`cadencia.rs`) conta mensagens
//!    RECEBIDAS. A entrega agendada tem orcamento PROPRIO por chat
//!    (`MAX_ENTREGAS_AGENDADAS_POR_JANELA`), separado do dos turnos: uma
//!    tarefa recorrente nao gasta o teto do humano da conversa, e uma rajada
//!    de tarefas nao vira rajada numa conta pessoal. Estourou, a entrega e
//!    descartada (a resposta fica no historico) e o ledger diz `teto`. Mesmo
//!    precedente do `telegram_send` (`channel_send::SendBudget`).
//! 2. **Marca de auto-conversa.** Toda entrega agendada sai com
//!    `MARCA_DO_GARRA`, em qualquer chat. O executor so tem o id da sessao e
//!    nao sabe se o chat e a conversa "Voce" — o JID da propria conta vive na
//!    ponte Node. A marca e um separador invisivel: na conversa "Voce" e o
//!    que impede outro Garra da mesma conta de responder a entrega; nos
//!    demais chats nao muda nada, porque mensagem da propria conta fora da
//!    conversa "Voce" nunca vira turno (`SemTurno::DaPropriaConta`).
//! 3. **Contato bloqueado ou removido.** A entrega reconsulta a config VIVA:
//!    canal desligado, grupo com resposta desligada e quem criou a tarefa nao
//!    ser mais admitido (bloqueado, ou fora da politica) barram o envio, com
//!    o mesmo silencio que o turno da a quem o portao recusa. A execucao em
//!    si segue a regra do #1985 (roda sob o teto vigente); o que muda aqui e
//!    so o destino da resposta.
//!
//! E a ponte fora do ar nao enfileira: o `runner` descarta de proposito o
//! que chega com a ponte caida, entao a entrega so vai com a ponte
//! conectada, e o ledger diz `ponte_fora_do_ar` nos outros casos.
//!
//! # Sem telefone em log
//!
//! O id da sessao do WhatsApp vinculado carrega o numero do contato. Nada
//! deste caminho loga o id da sessao: o executor registra o id da tarefa, o
//! canal e o desfecho.

use crate::bootstrap::WhatsAppLinkedSettings;
use crate::bootstrap::whatsapp_linked_politica::principal_do_turno;

/// O prefixo de `session_id` do WhatsApp vinculado.
pub(crate) const PREFIXO_WHATSAPP_LINKED: &str = "whatsapp-linked-";

/// Por que a resposta ficou so no historico.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Motivo {
    /// O canal tem entrega, mas a sessao nao diz para onde mandar.
    SemEndereco,
    /// O canal tem entrega, mas nao esta registrado neste gateway agora.
    CanalOffline,
    /// WhatsApp vinculado desligado na config viva.
    CanalDesligado,
    /// Grupo do WhatsApp com resposta desligada na config viva.
    GrupoDesligado,
    /// Quem criou a tarefa nao e mais admitido pela politica de acesso.
    NaoAdmitido,
    /// A ponte do WhatsApp nao esta conectada.
    PonteForaDoAr,
    /// Teto de entregas agendadas do chat atingido.
    Teto,
    /// O canal recusou o envio.
    Falhou,
}

impl Motivo {
    /// Etiqueta fixa: vai ao ledger e ao log, nunca identidade.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::SemEndereco => "sem_endereco",
            Self::CanalOffline => "canal_offline",
            Self::CanalDesligado => "canal_desligado",
            Self::GrupoDesligado => "grupo_desligado",
            Self::NaoAdmitido => "nao_admitido",
            Self::PonteForaDoAr => "ponte_fora_do_ar",
            Self::Teto => "teto",
            Self::Falhou => "falhou",
        }
    }
}

/// O destino da resposta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Entrega {
    /// O canal aceitou o envio (no WhatsApp: a ponte conectada recebeu).
    Entregue,
    /// O canal tem entrega ativa e a resposta nao foi; o motivo diz por que.
    NaoEntregue(Motivo),
    /// O canal da sessao nao tem entrega ativa (web, API, console local).
    SemCanal,
}

impl Entrega {
    /// O valor de `agent_runs.delivery`.
    pub(crate) fn para_o_ledger(&self) -> String {
        match self {
            Self::Entregue => "entregue".to_string(),
            Self::SemCanal => "sem_entrega".to_string(),
            Self::NaoEntregue(m) => format!("nao_entregue:{}", m.as_str()),
        }
    }
}

/// Os canais com entrega ativa por adaptador registrado. Fora desta lista, um
/// adaptador ausente e "sem entrega", e nao "canal offline".
pub(crate) const CANAIS_COM_ENTREGA: [&str; 4] = ["telegram", "discord", "slack", "matrix"];

/// O endereco de entrega dos canais cujo id de sessao ja e o endereco.
/// `None` quando o canal nao e um deles, quando o prefixo do id nao e o do
/// canal ou quando o resto nao tem a forma de um endereco daquele canal.
pub(crate) fn endereco_pela_sessao(canal: &str, session_id: &str) -> Option<serde_json::Value> {
    let (chave, prefixo, valido): (&str, &str, fn(&str) -> bool) = match canal {
        // Snowflake do Discord: o adaptador faz `parse::<u64>`.
        "discord" => ("discord_channel_id", "discord-", |s| {
            s.parse::<u64>().is_ok()
        }),
        // Ids do Slack: `C…`, `D…`, `G…`, so ASCII alfanumerico.
        "slack" => ("slack_channel_id", "slack-", |s| {
            s.chars().all(|c| c.is_ascii_alphanumeric())
        }),
        // Room id do Matrix: `!opaco:servidor`.
        "matrix" => ("matrix_room_id", "matrix-", |s| {
            s.starts_with('!')
                && s.contains(':')
                && !s.chars().any(|c| c.is_whitespace() || c.is_control())
        }),
        _ => return None,
    };
    let alvo = session_id.strip_prefix(prefixo)?;
    if alvo.is_empty() || !valido(alvo) {
        return None;
    }
    Some(serde_json::json!({ chave: alvo }))
}

/// O chat do WhatsApp vinculado de uma tarefa: exige o canal da sessao E o
/// prefixo do id, pelo mesmo motivo de [`endereco_pela_sessao`].
pub(crate) fn chat_do_whatsapp<'a>(canal: &str, session_id: &'a str) -> Option<&'a str> {
    if canal != crate::bootstrap::WHATSAPP_LINKED_CONFIG_KEY {
        return None;
    }
    let chat = session_id.strip_prefix(PREFIXO_WHATSAPP_LINKED)?;
    (chat.contains('@') && !chat.chars().any(|c| c.is_whitespace() || c.is_control()))
        .then_some(chat)
}

/// Empurra `texto` (a resposta ja persistida) ao canal da sessao da tarefa e
/// diz o desfecho. Nunca falha: entrega e transparencia, nao controle — o
/// teto da execucao ja valeu, e a resposta ja esta no historico.
pub(crate) async fn entregar(
    state: &crate::state::AppState,
    task: &garraia_db::ScheduledTask,
    texto: &str,
) -> Entrega {
    let canal = task.channel_id.as_str();

    if let Some(chat) = chat_do_whatsapp(canal, &task.session_id) {
        let settings = crate::bootstrap::whatsapp_linked_settings(&state.current_config());
        if let Err(motivo) = decidir_whatsapp(&settings, &task.user_id, chat) {
            return Entrega::NaoEntregue(motivo);
        }
        return match state.whatsapp_linked.entregar_agendada(chat, texto).await {
            Ok(()) => Entrega::Entregue,
            Err(motivo) => Entrega::NaoEntregue(motivo),
        };
    }

    let metadata = match canal {
        // #921: o endereco do Telegram mora em `chat_session_keys`, gravado
        // por `resolve_session`.
        "telegram" => {
            let external_id = match &state.chat_session_manager {
                Some(mgr) => mgr
                    .external_key_for(&task.session_id, garraia_db::ChatSource::Telegram)
                    .await
                    .unwrap_or_else(|e| {
                        tracing::warn!(
                            task = %task.id,
                            error = %e,
                            "tarefa agendada: falhou ao resolver o endereco do Telegram"
                        );
                        None
                    }),
                None => None,
            };
            crate::channel_send::with_channel_address(
                &task.session_metadata,
                canal,
                external_id.as_deref(),
            )
        }
        "discord" | "slack" | "matrix" => match endereco_pela_sessao(canal, &task.session_id) {
            Some(endereco) => mesclar(&task.session_metadata, endereco),
            None => return Entrega::NaoEntregue(Motivo::SemEndereco),
        },
        // Os demais (IRC, iMessage, Signal...) seguem como estavam: o
        // adaptador recebe a metadata da sessao e decide. Ver o doc do modulo.
        _ => task.session_metadata.clone(),
    };

    let mensagem = garraia_common::Message {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: garraia_common::SessionId::from_string(&task.session_id),
        channel_id: garraia_common::ChannelId::from_string(canal),
        user_id: garraia_common::UserId::from_string("genesis"),
        direction: garraia_common::MessageDirection::Outgoing,
        content: garraia_common::MessageContent::Text(texto.to_string()),
        timestamp: chrono::Utc::now(),
        metadata,
    };

    let canais = state.channels.read().await;
    let Some(adaptador) = canais.get(canal) else {
        return if CANAIS_COM_ENTREGA.contains(&canal) {
            Entrega::NaoEntregue(Motivo::CanalOffline)
        } else {
            Entrega::SemCanal
        };
    };
    match adaptador.send_message(&mensagem).await {
        Ok(()) => Entrega::Entregue,
        Err(e) => {
            // O erro do adaptador pode citar o endereco; o id da tarefa basta
            // para o operador achar o resto.
            tracing::debug!(task = %task.id, canal, error = %e, "tarefa agendada: o canal recusou o envio");
            Entrega::NaoEntregue(Motivo::Falhou)
        }
    }
}

/// `endereco` por cima da metadata da sessao (que pode ser `{}`, `null` ou
/// trazer a `continuity_key`).
fn mesclar(metadata: &serde_json::Value, endereco: serde_json::Value) -> serde_json::Value {
    let mut saida = match metadata {
        serde_json::Value::Object(m) => m.clone(),
        _ => serde_json::Map::new(),
    };
    if let serde_json::Value::Object(e) = endereco {
        saida.extend(e);
    }
    serde_json::Value::Object(saida)
}

/// A politica de entrega do WhatsApp vinculado, pela config VIVA: o canal
/// ligado, o grupo com resposta ligada e quem criou a tarefa ainda admitido —
/// o mesmo motor do turno (`principal_do_turno`). `pareado` vai `false`,
/// como em `tarefa_agendada::vigente_do_whatsapp`: quem so esta pareado tem
/// alcance de leitura e nunca conseguiu agendar.
pub(crate) fn decidir_whatsapp(
    settings: &WhatsAppLinkedSettings,
    criador: &str,
    chat: &str,
) -> Result<(), Motivo> {
    if !settings.enabled {
        return Err(Motivo::CanalDesligado);
    }
    let e_grupo = chat.ends_with("@g.us");
    if e_grupo && !settings.responde_em_grupo() {
        return Err(Motivo::GrupoDesligado);
    }
    if !principal_do_turno(settings, criador, chat, e_grupo, false).admitido() {
        return Err(Motivo::NaoAdmitido);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bootstrap::{WHATSAPP_LINKED_CONFIG_KEY, whatsapp_linked_settings};

    const DONO: &str = "5511999990000";
    const USUARIO: &str = "5511888880000";
    const BLOQUEADO: &str = "5511666660000";
    const ESTRANHO: &str = "5511777770000";

    fn settings(valor: serde_json::Value) -> WhatsAppLinkedSettings {
        let mut config = garraia_config::AppConfig::default();
        config.channels.insert(
            WHATSAPP_LINKED_CONFIG_KEY.to_string(),
            garraia_config::ChannelConfig {
                channel_type: WHATSAPP_LINKED_CONFIG_KEY.to_string(),
                enabled: Some(true),
                settings: valor
                    .as_object()
                    .expect("objeto")
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            },
        );
        whatsapp_linked_settings(&config)
    }

    fn politica_padrao() -> WhatsAppLinkedSettings {
        settings(serde_json::json!({
            "owners": [DONO],
            "access": {
                "users": {
                    USUARIO: { "level": "full" },
                    BLOQUEADO: { "level": "full", "blocked": true }
                }
            }
        }))
    }

    fn jid(numero: &str) -> String {
        format!("{numero}@s.whatsapp.net")
    }

    // --- endereco pela sessao -------------------------------------------

    #[test]
    fn discord_slack_e_matrix_tiram_o_endereco_do_id_da_sessao() {
        assert_eq!(
            endereco_pela_sessao("discord", "discord-987654321"),
            Some(serde_json::json!({ "discord_channel_id": "987654321" }))
        );
        assert_eq!(
            endereco_pela_sessao("slack", "slack-C0123ABCD"),
            Some(serde_json::json!({ "slack_channel_id": "C0123ABCD" }))
        );
        assert_eq!(
            endereco_pela_sessao("matrix", "matrix-!sala:exemplo.org"),
            Some(serde_json::json!({ "matrix_room_id": "!sala:exemplo.org" }))
        );
    }

    /// O canal e o prefixo precisam concordar: o `channel_id` da sessao e o
    /// ULTIMO a gravar a linha, e um id escolhido por cliente HTTP
    /// (`X-Session-Id`) pode imitar o prefixo de outro canal.
    #[test]
    fn canal_e_prefixo_discordantes_nao_dao_endereco() {
        assert_eq!(endereco_pela_sessao("discord", "slack-C0123ABCD"), None);
        assert_eq!(endereco_pela_sessao("api", "discord-123"), None);
        assert_eq!(endereco_pela_sessao("telegram", "telegram-123"), None);
    }

    #[test]
    fn endereco_malformado_nao_vira_destino() {
        assert_eq!(endereco_pela_sessao("discord", "discord-"), None);
        assert_eq!(endereco_pela_sessao("discord", "discord-abc"), None);
        assert_eq!(endereco_pela_sessao("slack", "slack-"), None);
        assert_eq!(endereco_pela_sessao("slack", "slack-C01 23"), None);
        assert_eq!(
            endereco_pela_sessao("matrix", "matrix-sem-exclamacao"),
            None
        );
        assert_eq!(endereco_pela_sessao("matrix", "matrix-!sala\n:x.org"), None);
    }

    /// IRC e iMessage ficam fora (#2002): o IRC manda a resposta crua ao
    /// servidor sem quebrar linha (`send_privmsg`), e as mensagens privadas de
    /// todos caem numa sessao so (`irc-<nick do bot>`); no iMessage a sessao
    /// de grupo guarda o NOME do grupo, que nao e endereco de envio.
    #[test]
    fn irc_e_imessage_nao_ganham_endereco() {
        assert_eq!(endereco_pela_sessao("irc", "irc-#garra"), None);
        assert_eq!(
            endereco_pela_sessao("imessage", "imessage-+5511999990000"),
            None
        );
    }

    // --- WhatsApp vinculado ---------------------------------------------

    #[test]
    fn chat_do_whatsapp_exige_canal_e_prefixo() {
        let sessao = format!("whatsapp-linked-{}", jid(DONO));
        assert_eq!(
            chat_do_whatsapp(WHATSAPP_LINKED_CONFIG_KEY, &sessao),
            Some(jid(DONO).as_str())
        );
        assert_eq!(chat_do_whatsapp("api", &sessao), None, "prefixo sem canal");
        assert_eq!(
            chat_do_whatsapp(WHATSAPP_LINKED_CONFIG_KEY, "sess-qualquer"),
            None,
            "canal sem prefixo"
        );
        assert_eq!(
            chat_do_whatsapp(WHATSAPP_LINKED_CONFIG_KEY, "whatsapp-linked-"),
            None
        );
        assert_eq!(
            chat_do_whatsapp(WHATSAPP_LINKED_CONFIG_KEY, "whatsapp-linked-sem-arroba"),
            None
        );
    }

    #[test]
    fn dono_e_usuario_admitidos_recebem() {
        let s = politica_padrao();
        assert_eq!(decidir_whatsapp(&s, DONO, &jid(DONO)), Ok(()));
        assert_eq!(decidir_whatsapp(&s, USUARIO, &jid(USUARIO)), Ok(()));
    }

    #[test]
    fn bloqueado_e_removido_nao_recebem() {
        let s = politica_padrao();
        assert_eq!(
            decidir_whatsapp(&s, BLOQUEADO, &jid(BLOQUEADO)),
            Err(Motivo::NaoAdmitido)
        );
        assert_eq!(
            decidir_whatsapp(&s, ESTRANHO, &jid(ESTRANHO)),
            Err(Motivo::NaoAdmitido),
            "removido da config depois de agendar"
        );
    }

    #[test]
    fn canal_desligado_a_quente_nao_entrega() {
        let mut s = politica_padrao();
        s.enabled = false;
        assert_eq!(
            decidir_whatsapp(&s, DONO, &jid(DONO)),
            Err(Motivo::CanalDesligado)
        );
    }

    #[test]
    fn grupo_desligado_a_quente_nao_entrega() {
        let grupo = "120363000000000000@g.us";
        let desligado = settings(serde_json::json!({
            "owners": [DONO],
            "access": { "groups": { "enabled": false } }
        }));
        assert_eq!(
            decidir_whatsapp(&desligado, DONO, grupo),
            Err(Motivo::GrupoDesligado)
        );
    }

    // --- envio pela ponte ------------------------------------------------

    use crate::bootstrap::{
        WHATSAPP_LINKED_MARCA_DO_GARRA, WHATSAPP_LINKED_MAX_ENTREGAS_AGENDADAS,
        WhatsAppLinkedRuntime,
    };
    use garraia_channels::whatsapp_linked::BridgeCommand;
    use garraia_channels::whatsapp_linked::health::BridgeView;
    use tokio::sync::mpsc;

    fn ponte_conectada() -> (WhatsAppLinkedRuntime, mpsc::Receiver<BridgeCommand>) {
        let rt = WhatsAppLinkedRuntime::default();
        let (tx, rx) = mpsc::channel(16);
        rt.reter_saida(tx);
        rt.set_bridge(BridgeView::Connected);
        (rt, rx)
    }

    /// A entrega sai pela ponte, no chat da sessao, com a marca invisivel na
    /// frente: o executor nao sabe se o chat e a conversa "Voce".
    #[tokio::test]
    async fn entrega_sai_pela_ponte_com_a_marca() {
        let (rt, mut rx) = ponte_conectada();
        assert_eq!(
            rt.entregar_agendada(&jid(DONO), "relatorio pronto").await,
            Ok(())
        );
        match rx.try_recv() {
            Ok(BridgeCommand::Send { chat_jid, text, .. }) => {
                assert_eq!(chat_jid.as_str(), jid(DONO));
                assert_eq!(
                    text,
                    format!("{WHATSAPP_LINKED_MARCA_DO_GARRA}relatorio pronto")
                );
            }
            outro => panic!("esperava um Send, veio {outro:?}"),
        }
    }

    /// Ponte caida nao enfileira: o `runner` descarta o que chega com ela
    /// fora do ar, entao enfileirar so esconderia que a resposta nao foi.
    #[tokio::test]
    async fn ponte_fora_do_ar_nao_enfileira() {
        let (rt, mut rx) = ponte_conectada();
        rt.set_bridge(BridgeView::Down);
        assert_eq!(
            rt.entregar_agendada(&jid(DONO), "x").await,
            Err(Motivo::PonteForaDoAr)
        );
        assert!(rx.try_recv().is_err(), "nada foi para a fila");

        let sem_saida = WhatsAppLinkedRuntime::default();
        sem_saida.set_bridge(BridgeView::Connected);
        assert_eq!(
            sem_saida.entregar_agendada(&jid(DONO), "x").await,
            Err(Motivo::PonteForaDoAr),
            "supervisor que nunca subiu"
        );
    }

    /// O orcamento e por chat e proprio das entregas agendadas: estourar num
    /// chat nao cala o outro.
    #[tokio::test]
    async fn teto_de_entregas_e_por_chat() {
        let (rt, mut rx) = ponte_conectada();
        for _ in 0..WHATSAPP_LINKED_MAX_ENTREGAS_AGENDADAS {
            assert_eq!(rt.entregar_agendada(&jid(DONO), "x").await, Ok(()));
        }
        assert_eq!(
            rt.entregar_agendada(&jid(DONO), "x").await,
            Err(Motivo::Teto)
        );
        assert_eq!(
            rt.entregar_agendada(&jid(USUARIO), "x").await,
            Ok(()),
            "outro chat tem o proprio orcamento"
        );
        let mut enviados = 0;
        while rx.try_recv().is_ok() {
            enviados += 1;
        }
        assert_eq!(enviados, WHATSAPP_LINKED_MAX_ENTREGAS_AGENDADAS + 1);
    }

    /// O orcamento das entregas nao e o da cadencia dos turnos: uma tarefa
    /// recorrente nao gasta o teto do humano da conversa.
    #[tokio::test]
    async fn entrega_nao_gasta_a_cadencia_dos_turnos() {
        let (rt, _rx) = ponte_conectada();
        let antes = rt.cadencia_diagnostico();
        for _ in 0..WHATSAPP_LINKED_MAX_ENTREGAS_AGENDADAS {
            let _ = rt.entregar_agendada(&jid(DONO), "x").await;
        }
        assert_eq!(rt.cadencia_diagnostico(), antes);
    }

    // --- o executor: `entregar` ------------------------------------------

    use crate::state::AppState;
    use garraia_agents::AgentRuntime;
    use garraia_channels::{Channel, ChannelRegistry, ChannelStatus};
    use std::sync::{Arc, Mutex};

    /// Canal que grava a metadata de cada envio, ou falha se pedido.
    struct CanalGravador {
        tipo: &'static str,
        enviados: Arc<Mutex<Vec<serde_json::Value>>>,
        falha: bool,
    }

    #[async_trait::async_trait]
    impl Channel for CanalGravador {
        fn channel_type(&self) -> &str {
            self.tipo
        }
        fn display_name(&self) -> &str {
            self.tipo
        }
        async fn connect(&mut self) -> garraia_common::Result<()> {
            Ok(())
        }
        async fn disconnect(&mut self) -> garraia_common::Result<()> {
            Ok(())
        }
        async fn send_message(&self, m: &garraia_common::Message) -> garraia_common::Result<()> {
            if self.falha {
                return Err(garraia_common::Error::Channel("recusado".into()));
            }
            if let Ok(mut v) = self.enviados.lock() {
                v.push(m.metadata.clone());
            }
            Ok(())
        }
        fn status(&self) -> ChannelStatus {
            ChannelStatus::Connected
        }
    }

    fn estado_com(
        config: garraia_config::AppConfig,
        canal: Option<(&'static str, bool)>,
    ) -> (AppState, Arc<Mutex<Vec<serde_json::Value>>>) {
        let enviados = Arc::new(Mutex::new(Vec::new()));
        let mut canais = ChannelRegistry::new();
        if let Some((tipo, falha)) = canal {
            canais.register(Box::new(CanalGravador {
                tipo,
                enviados: Arc::clone(&enviados),
                falha,
            }));
        }
        (
            AppState::new(config, Arc::new(AgentRuntime::new()), canais),
            enviados,
        )
    }

    fn tarefa_em(canal: &str, sessao: &str, criador: &str) -> garraia_db::ScheduledTask {
        garraia_db::ScheduledTask {
            id: "tarefa-1".to_string(),
            session_id: sessao.to_string(),
            channel_id: canal.to_string(),
            user_id: criador.to_string(),
            execute_at: chrono::Utc::now(),
            payload: "gera o relatorio".to_string(),
            session_metadata: serde_json::json!({}),
            cron_expr: None,
            timezone: None,
            run_count: 0,
            max_runs: None,
            attempts: 0,
            authority: None,
        }
    }

    #[tokio::test]
    async fn discord_recebe_no_canal_da_sessao() {
        let (state, enviados) = estado_com(Default::default(), Some(("discord", false)));
        let task = tarefa_em("discord", "discord-123456789", "u1");
        assert_eq!(entregar(&state, &task, "pronto").await, Entrega::Entregue);
        let enviados = enviados.lock().unwrap();
        assert_eq!(enviados.len(), 1);
        assert_eq!(enviados[0]["discord_channel_id"], "123456789");
    }

    #[tokio::test]
    async fn discord_sem_endereco_nao_chama_o_adaptador() {
        let (state, enviados) = estado_com(Default::default(), Some(("discord", false)));
        let task = tarefa_em("discord", "discord-nao-e-numero", "u1");
        assert_eq!(
            entregar(&state, &task, "pronto").await,
            Entrega::NaoEntregue(Motivo::SemEndereco)
        );
        assert!(enviados.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn canal_com_entrega_desconectado_e_offline_e_o_resto_e_sem_entrega() {
        let (state, _) = estado_com(Default::default(), None);
        assert_eq!(
            entregar(&state, &tarefa_em("slack", "slack-C01", "u1"), "x").await,
            Entrega::NaoEntregue(Motivo::CanalOffline)
        );
        assert_eq!(
            entregar(&state, &tarefa_em("web", "sess-web", "u1"), "x").await,
            Entrega::SemCanal
        );
    }

    #[tokio::test]
    async fn recusa_do_adaptador_vira_falhou() {
        let (state, _) = estado_com(Default::default(), Some(("matrix", true)));
        let task = tarefa_em("matrix", "matrix-!sala:x.org", "u1");
        assert_eq!(
            entregar(&state, &task, "x").await,
            Entrega::NaoEntregue(Motivo::Falhou)
        );
    }

    fn config_do_whatsapp() -> garraia_config::AppConfig {
        let mut config = garraia_config::AppConfig::default();
        config.channels.insert(
            WHATSAPP_LINKED_CONFIG_KEY.to_string(),
            garraia_config::ChannelConfig {
                channel_type: WHATSAPP_LINKED_CONFIG_KEY.to_string(),
                enabled: Some(true),
                settings: serde_json::json!({
                    "owners": [DONO],
                    "access": { "users": { BLOQUEADO: { "level": "full", "blocked": true } } }
                })
                .as_object()
                .expect("objeto")
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
            },
        );
        config
    }

    #[tokio::test]
    async fn whatsapp_entrega_ao_dono_pela_ponte() {
        let (state, _) = estado_com(config_do_whatsapp(), None);
        let (tx, mut rx) = mpsc::channel(4);
        state.whatsapp_linked.reter_saida(tx);
        state.whatsapp_linked.set_bridge(BridgeView::Connected);
        let sessao = format!("whatsapp-linked-{}", jid(DONO));
        let task = tarefa_em(WHATSAPP_LINKED_CONFIG_KEY, &sessao, DONO);
        assert_eq!(entregar(&state, &task, "pronto").await, Entrega::Entregue);
        assert!(matches!(rx.try_recv(), Ok(BridgeCommand::Send { .. })));
    }

    #[tokio::test]
    async fn whatsapp_nao_entrega_a_bloqueado_nem_gasta_o_orcamento() {
        let (state, _) = estado_com(config_do_whatsapp(), None);
        let (tx, mut rx) = mpsc::channel(4);
        state.whatsapp_linked.reter_saida(tx);
        state.whatsapp_linked.set_bridge(BridgeView::Connected);
        let sessao = format!("whatsapp-linked-{}", jid(BLOQUEADO));
        let task = tarefa_em(WHATSAPP_LINKED_CONFIG_KEY, &sessao, BLOQUEADO);
        for _ in 0..=WHATSAPP_LINKED_MAX_ENTREGAS_AGENDADAS {
            assert_eq!(
                entregar(&state, &task, "x").await,
                Entrega::NaoEntregue(Motivo::NaoAdmitido)
            );
        }
        assert!(rx.try_recv().is_err(), "nada foi para a ponte");
    }

    // --- ledger -----------------------------------------------------------

    #[test]
    fn ledger_distingue_entregue_de_so_persistida() {
        assert_eq!(Entrega::Entregue.para_o_ledger(), "entregue");
        assert_eq!(Entrega::SemCanal.para_o_ledger(), "sem_entrega");
        assert_eq!(
            Entrega::NaoEntregue(Motivo::Teto).para_o_ledger(),
            "nao_entregue:teto"
        );
        assert_eq!(
            Entrega::NaoEntregue(Motivo::NaoAdmitido).para_o_ledger(),
            "nao_entregue:nao_admitido"
        );
    }
}
