//! `telegram_send_voice` — the agent's proactive outbound VOICE message.
//!
//! Companion of [`crate::tools::TelegramSendTool`] (issue #921): mesmo destino,
//! mesma decisão de chat, mesmo teto anti-amplificação — o que muda é o corpo:
//! o texto é sintetizado pelo TTS configurado (`state.voice_client`) e sai como
//! *voice message* no Telegram, pelo mesmo caminho `send_message` do adaptador
//! que o handler de voice message usa no sentido contrário.
//!
//! Por que uma tool e não um parâmetro do `telegram_send`: voz é mais
//! intrusiva que texto — notificação com som no celular — e sintetizar custa
//! GPU/latência. Duas superfícies separadas deixam o operador (e o modo) freiá-
//! la sem frear o texto: a tabela de capacidades nativas mapeia as duas para
//! `MessageSend`, então um modo que libera mensagem libera as duas, e um modo
//! que queira só texto nega `telegram_send_voice` pelo nome.
//!
//! Falha de TTS é ERRO, não fallback silencioso para texto: o modelo sabe que
//! a fala não saiu e decide (mandar texto, avisar o usuário). O fallback
//! silencioso era como o runtime mandava áudio que ninguém ouvia.

use std::sync::{Arc, Weak};

use async_trait::async_trait;
use garraia_agents::tools::{Tool, ToolContext, ToolOutput};
use garraia_common::{
    ChannelId, Message, MessageContent, MessageDirection, Result, SessionId, UserId,
};

use crate::channel_send::{ProactiveTargets, SendBudget, with_channel_address};
use crate::state::AppState;

/// Teto do TEXTO que vira voz. TTS tem timeout próprio (`timeouts.tts`, 120 s
/// padrão) e voz longa é abuso auditivo — bem antes do limite do Telegram,
/// o texto que vale a pena ouvir é curto. O `telegram_send` continua sendo o
/// caminho para texto longo.
const MAX_VOICE_TEXT_CHARS: usize = 1000;

/// Teto de voz por sessao por janela ([`crate::channel_send::SEND_WINDOW`]).
/// Menor que o do texto (5): voz toca no celular, e o custo por envio
/// (sintese + upload) e maior. Barato de subir depois; caro de explicar spam.
pub const MAX_VOICE_SENDS_PER_WINDOW: u32 = 2;

pub struct TelegramSendVoiceTool {
    /// **Weak** on purpose — same rationale as [`crate::tools::TelegramSendTool`].
    state: Weak<AppState>,
    /// Anti-amplification ceiling, per session. See [`SendBudget`].
    budget: SendBudget,
}

impl TelegramSendVoiceTool {
    pub fn new(state: &Arc<AppState>) -> Self {
        Self {
            state: Arc::downgrade(state),
            budget: SendBudget::with_max(MAX_VOICE_SENDS_PER_WINDOW),
        }
    }

    /// The live state, or `None` once the gateway has shut down.
    fn state(&self) -> Option<Arc<AppState>> {
        self.state.upgrade()
    }
}

#[async_trait]
impl Tool for TelegramSendVoiceTool {
    fn name(&self) -> &str {
        "telegram_send_voice"
    }

    /// Chamável só com o Telegram configurado E conectado E um TTS saudável.
    /// Sem TTS a tool some da lista com o motivo — o modelo nunca tenta
    /// sintetizar num runtime sem voz.
    fn disponibilidade(&self) -> garraia_agents::tools::Disponibilidade {
        use garraia_agents::tools::Disponibilidade;
        let Some(state) = self.state() else {
            return Disponibilidade::indisponivel(
                "gateway_down",
                "o gateway esta encerrando.",
                None,
            );
        };
        let Some(tts) = &state.voice_client else {
            return Disponibilidade::indisponivel(
                "tts_unconfigured",
                "o TTS nao esta configurado neste Garra (voice.enabled ausente ou sem provedor).",
                Some(
                    "Configure `voice` (provider/endpoint) e reinicie; enquanto isso, \
                     use `telegram_send` para texto."
                        .to_string(),
                ),
            );
        };
        let _ = tts; // a saude da sintese se prova na chamada, nao aqui.
        let config = state.current_config();
        let configurado = config
            .channels
            .values()
            .any(|c| c.channel_type == "telegram" && c.enabled.unwrap_or(true));
        if !configurado {
            return Disponibilidade::indisponivel(
                "not_configured",
                "o canal Telegram nao esta configurado (ou esta desligado) neste Garra.",
                Some(
                    "Configure `channels.<nome>` com `type: telegram` e o token do bot \
                     (`garraia channel add telegram`) e reinicie."
                        .to_string(),
                ),
            );
        }
        let conectado = match state.channels.try_read() {
            Ok(registry) => registry.list().contains(&"telegram"),
            Err(_) => true,
        };
        if !conectado {
            return Disponibilidade::indisponivel(
                "channel_offline",
                "o canal Telegram esta configurado, mas nao esta conectado agora.",
                Some("Veja `garra_status` (lista `channels`) e o log do gateway.".to_string()),
            );
        }
        Disponibilidade::Disponivel
    }

    fn description(&self) -> &str {
        "Envia uma MENSAGEM DE VOZ no Telegram por iniciativa própria: sintetiza o texto \
         com o TTS configurado e entrega como voice message. Sem `chat_id`, responde no \
         chat desta conversa; com `chat_id`, só para chats liberados pelo operador. \
         Use quando o contexto pede voz (saudação, resposta a voice message, recado \
         falado); para texto, use `telegram_send`."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "Texto a ser falado (máximo 1000 caracteres)"
                },
                "chat_id": {
                    "type": "integer",
                    "description": "Chat de destino. Omita para responder no chat desta \
                                    conversa — é o caso normal. Um chat_id explícito precisa \
                                    estar liberado pelo operador."
                }
            },
            "required": ["text"]
        })
    }

    async fn execute(&self, context: &ToolContext, input: serde_json::Value) -> Result<ToolOutput> {
        let Some(text) = input.get("text").and_then(|v| v.as_str()) else {
            return Ok(ToolOutput::error("parâmetro 'text' ausente"));
        };
        if text.trim().is_empty() {
            return Ok(ToolOutput::error("'text' está vazio"));
        }
        if text.chars().count() > MAX_VOICE_TEXT_CHARS {
            return Ok(ToolOutput::error(format!(
                "texto para voz muito longo: {} caracteres (limite: {MAX_VOICE_TEXT_CHARS}). \
                 Encurte ou use `telegram_send` para texto.",
                text.chars().count()
            )));
        }

        let Some(state) = self.state() else {
            return Ok(ToolOutput::error("gateway encerrando"));
        };
        let Some(tts) = state.voice_client.clone() else {
            return Ok(ToolOutput::error(
                "TTS não está configurado neste runtime; nenhuma voz foi enviada.",
            ));
        };

        let requested = input.get("chat_id").and_then(|v| v.as_i64());
        let targets = ProactiveTargets::from_config(&state.current_config());
        let chat_id = match crate::tools::channel_send_tool::resolver_alvo_telegram(
            Some(state.clone()),
            context,
            requested,
            &targets,
        )
        .await
        {
            Ok(id) => id,
            Err(reason) => return Ok(ToolOutput::error(reason)),
        };

        // Teto proprio e mais apertado que o do texto: voz notifica com som.
        if let Err(used) = self
            .budget
            .try_consume(&context.session_id, std::time::Instant::now())
        {
            tracing::warn!(
                session = %context.session_id,
                used,
                "telegram_send_voice: teto de envios da sessão atingido"
            );
            return Ok(ToolOutput::error(format!(
                "limite de {used} mensagens de voz por minuto nesta conversa atingido. \
                 Junte o que falta dizer numa única mensagem (ou use `telegram_send`)."
            )));
        }

        let language = state.current_config().voice.language.clone();
        let audio = match tts.synthesize_bytes(text, &language).await {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::error!(
                    session = %context.session_id,
                    error = %e,
                    "telegram_send_voice: síntese TTS falhou"
                );
                return Ok(ToolOutput::error(format!(
                    "falha ao sintetizar voz: {e}. Nada foi enviado; se quiser, \
                     mande o recado como texto com `telegram_send`."
                )));
            }
        };

        // WAV cru em arquivo temporário próprio; o adaptador lê como
        // `InputFile::file` e o registro de uso fica no log de tracing.
        let tmp = std::env::temp_dir().join(format!("garraia_voice_{}.wav", uuid::Uuid::new_v4()));
        if let Err(e) = tokio::fs::write(&tmp, &audio).await {
            return Ok(ToolOutput::error(format!(
                "falha ao gravar o áudio temporário: {e}. Nada foi enviado."
            )));
        }

        let metadata = with_channel_address(
            &serde_json::json!({}),
            "telegram",
            Some(&chat_id.to_string()),
        );
        let message = Message {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: SessionId::from_string(&context.session_id),
            channel_id: ChannelId::from_string("telegram"),
            user_id: UserId::from_string(context.user_id.as_deref().unwrap_or("genesis")),
            direction: MessageDirection::Outgoing,
            content: MessageContent::Audio {
                url: tmp.display().to_string(),
                duration_secs: None,
            },
            timestamp: chrono::Utc::now(),
            metadata,
        };

        let channels = state.channels.read().await;
        let Some(channel) = channels.get("telegram") else {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Ok(ToolOutput::error(
                "canal telegram não está registrado neste gateway",
            ));
        };

        let resultado = channel.send_message(&message).await;
        // O temporário morre vença o envio ou não: o Telegram já leu o arquivo
        // no upload síncrono do teloxide.
        let _ = tokio::fs::remove_file(&tmp).await;
        match resultado {
            Ok(()) => {
                // chat_id is not logged: it identifies a person or a group.
                tracing::info!(
                    session = %context.session_id,
                    explicit_target = requested.is_some(),
                    chars = text.chars().count(),
                    audio_bytes = audio.len(),
                    "telegram_send_voice: voice message entregue"
                );
                Ok(ToolOutput::success("mensagem de voz enviada no Telegram"))
            }
            Err(e) => Ok(ToolOutput::error(format!(
                "falha ao enviar a voz: {e}. Nada chegou ao usuário."
            ))),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(session: &str) -> ToolContext {
        ToolContext {
            session_id: session.to_string(),
            user_id: None,
            is_heartbeat: false,
            approval: garraia_agents::tools::approval::ToolApproval::None,
            working_dir: None,
            project_id: None,
        }
    }

    /// Estado sem store nem canal — igual ao fixture do `telegram_send`.
    fn tool() -> (Arc<AppState>, TelegramSendVoiceTool) {
        let state = Arc::new(crate::state::AppState::new(
            garraia_config::AppConfig::default(),
            Arc::new(garraia_agents::AgentRuntime::new()),
            garraia_channels::ChannelRegistry::new(),
        ));
        let t = TelegramSendVoiceTool::new(&state);
        (state, t)
    }

    /// TTS de teste que registra se foi chamado: a recusa de destino tem de
    /// acontecer ANTES de qualquer sintese.
    struct TtsEspiao(std::sync::atomic::AtomicBool);

    #[async_trait::async_trait]
    impl garraia_voice::TtsSynthesizer for TtsEspiao {
        async fn synthesize_bytes(
            &self,
            _text: &str,
            _language: &str,
        ) -> std::result::Result<Vec<u8>, garraia_voice::VoiceError> {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(vec![0u8; 4])
        }
    }

    fn tool_com_tts() -> (Arc<AppState>, Arc<TtsEspiao>, TelegramSendVoiceTool) {
        let mut state = crate::state::AppState::new(
            garraia_config::AppConfig::default(),
            Arc::new(garraia_agents::AgentRuntime::new()),
            garraia_channels::ChannelRegistry::new(),
        );
        let espiao = Arc::new(TtsEspiao(std::sync::atomic::AtomicBool::new(false)));
        state.voice_client = Some(espiao.clone());
        let state = Arc::new(state);
        let t = TelegramSendVoiceTool::new(&state);
        (state, espiao, t)
    }

    /// Schema sanity: `chat_id` opcional, como no `telegram_send`.
    #[test]
    fn only_text_is_required() {
        let (_state, t) = tool();
        let schema = t.input_schema();
        assert_eq!(schema["required"], serde_json::json!(["text"]));
    }

    /// Entrada podre e voz gigante: recusadas ANTES de qualquer sintese —
    /// o TTS não é chamado para um texto que nunca sairia.
    #[tokio::test]
    async fn rejects_empty_and_oversized_text() {
        let (_state, t) = tool();
        let out = t
            .execute(&ctx("s1"), serde_json::json!({"text": "   "}))
            .await
            .unwrap();
        assert!(out.is_error);
        assert!(out.content.contains("vazio"));

        let long = "a".repeat(MAX_VOICE_TEXT_CHARS + 1);
        let out = t
            .execute(&ctx("s1"), serde_json::json!({"text": long}))
            .await
            .unwrap();
        assert!(out.is_error);
        assert!(out.content.contains("muito longo"));
        assert!(
            out.content.contains("telegram_send"),
            "aponta o caminho de texto"
        );

        let out = t.execute(&ctx("s1"), serde_json::json!({})).await.unwrap();
        assert!(out.is_error);
    }

    /// Sem TTS configurado: indisponivel com o codigo certo, e o execute
    /// tambem falha fechado (defesa em profundidade — a disponibilidade e
    /// lida pelo runtime, o execute e a ultima linha).
    #[tokio::test]
    async fn sem_tts_e_indisponivel_e_execute_falha_fechado() {
        use garraia_agents::tools::Disponibilidade;
        let (_state, t) = tool();
        match t.disponibilidade() {
            Disponibilidade::Indisponivel { codigo, .. } => {
                assert_eq!(codigo, "tts_unconfigured");
            }
            Disponibilidade::Disponivel => panic!("sem TTS nao pode estar disponivel"),
        }
        let out = t
            .execute(&ctx("s1"), serde_json::json!({"text": "ola"}))
            .await
            .unwrap();
        assert!(out.is_error);
        assert!(out.content.contains("TTS"));
    }

    /// chat_id explicito fora da allowlist: a MESMA recusa do `telegram_send`,
    /// pelo helper compartilhado — e a recusa nao ecoa o id (e o chat de
    /// alguem) e nao queima o teto.
    #[tokio::test]
    async fn explicit_target_refused_shares_the_text_tool_decision() {
        let (_state, espiao, t) = tool_com_tts();
        let out = t
            .execute(
                &ctx("s1"),
                serde_json::json!({"text": "oi", "chat_id": 12345}),
            )
            .await
            .unwrap();
        assert!(out.is_error);
        assert!(
            out.content.contains("proactive_chat_ids"),
            "{}",
            out.content
        );
        assert!(!out.content.contains("12345"), "id de terceiro nao volta");
        assert!(
            !espiao.0.load(std::sync::atomic::Ordering::SeqCst),
            "destino recusado nao pode sintetizar"
        );
    }

    /// O teto de voz e mais apertado que o do texto e vale por sessao.
    #[test]
    fn teto_de_voz_e_menor_que_o_do_texto() {
        const { assert!(MAX_VOICE_SENDS_PER_WINDOW < crate::channel_send::MAX_SENDS_PER_WINDOW) };
        let (_state, t) = tool();
        let agora = std::time::Instant::now();
        for _ in 0..MAX_VOICE_SENDS_PER_WINDOW {
            assert!(t.budget.try_consume("s1", agora).is_ok());
        }
        assert!(t.budget.try_consume("s1", agora).is_err());
        assert!(
            t.budget.try_consume("s2", agora).is_ok(),
            "outra sessao tem teto proprio"
        );
    }
}
