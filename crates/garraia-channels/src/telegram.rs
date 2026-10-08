use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use teloxide::dispatching::UpdateFilterExt;
use teloxide::prelude::*;
use teloxide::types::{BotCommand, ChatAction, ParseMode};
use tokio::sync::{mpsc, watch};
use tracing::{error, info, warn};

use crate::telegram_fmt::to_telegram_markdown;
use crate::traits::{Channel, ChannelStatus};
use garraia_common::{Message, MessageContent, Result};

/// Callback invoked when the bot receives a text message.
///
/// Arguments: `(chat_id, user_id_string, user_display_name, text, delta_sender)`.
/// When `delta_sender` is `Some`, the callback should send text deltas through it
/// for streaming display. The callback still returns the final complete text.
/// Return `Err("__blocked__")` to silently drop the message (unauthorized user).
pub type OnMessageFn = Arc<
    dyn Fn(
            i64,
            String,
            String,
            String,
            Option<mpsc::Sender<String>>,
        ) -> Pin<Box<dyn Future<Output = std::result::Result<String, String>> + Send>>
        + Send
        + Sync,
>;

/// Callback invoked when the bot receives a voice message.
///
/// Arguments: `(bot, message)`.
/// The callback is responsible for downloading the voice file, processing it
/// through the voice pipeline (STT → LLM → TTS), and sending the response
/// back to the chat. This keeps the heavy voice logic out of the channel crate.
pub type OnVoiceFn = Arc<
    dyn Fn(
            Bot,
            teloxide::types::Message,
        ) -> Pin<Box<dyn Future<Output = std::result::Result<(), String>> + Send>>
        + Send
        + Sync,
>;

pub struct TelegramChannel {
    bot_token: String,
    display: String,
    status: ChannelStatus,
    on_message: OnMessageFn,
    on_voice: Option<OnVoiceFn>,
    bot: Option<Bot>,
    shutdown_tx: Option<watch::Sender<bool>>,
    /// Commands to register via `setMyCommands` on boot.
    commands_for_menu: Vec<(String, String)>,
}

impl TelegramChannel {
    pub fn new(bot_token: String, on_message: OnMessageFn) -> Self {
        Self {
            bot_token,
            display: "Telegram".to_string(),
            status: ChannelStatus::Disconnected,
            on_message,
            on_voice: None,
            bot: None,
            shutdown_tx: None,
            commands_for_menu: Vec::new(),
        }
    }

    /// Set an optional voice message handler.
    ///
    /// When set, incoming voice messages will be routed to this callback
    /// instead of being silently dropped.
    pub fn with_voice_handler(mut self, handler: OnVoiceFn) -> Self {
        self.on_voice = Some(handler);
        self
    }

    /// Set commands to register via Telegram's `setMyCommands` on boot.
    ///
    /// Each entry is `(command_name, description)`. These appear in the
    /// autocomplete menu when users type `/` in the chat.
    pub fn with_commands(mut self, commands: Vec<(String, String)>) -> Self {
        self.commands_for_menu = commands;
        self
    }
}

// ── #1594: validação do token com retry e classificação honesta ─────────

/// Por que o `GetMe` falhou — a classe que decide se retry ajuda e qual
/// mensagem o operador recebe.
///
/// O defect da #1594: `connect()` tratava TODO erro como "token inválido",
/// então uma falha de rede sob Termux/Android (Bionic sem `SSL_CERT_FILE`,
/// DNS do provedor, sleep do aparelho) mandava o operador trocar um token
/// que estava certo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GetMeFalha {
    /// A API do Telegram respondeu e disse Unauthorized/Not Found — o token
    /// mesmo está errado. Retry não conserta: falha rápido e direto.
    TokenInvalido,
    /// Rede/DNS/SSL/IO não completou a chamada — transitório em princípio
    /// (foi exatamente o sintoma reportado sob Termux). Retry com backoff.
    Transiente,
    /// Qualquer outra resposta da API (migração, JSON quebrado de proxy,
    /// erro de método). Não é o token, mas repetir não muda nada.
    Outro,
}

/// #1594: classifica um erro do `getMe` para a política de retry.
///
/// `ApiError::InvalidToken` cobre "Unauthorized" e "Not Found" — os dois
/// textos que o Telegram devolve para token errado (teloxide 0.17). Rede e
/// IO viram [`GetMeFalha::Transiente`]; tudo o resto, [`GetMeFalha::Outro`].
pub(crate) fn classifica_get_me(e: &teloxide::RequestError) -> GetMeFalha {
    match e {
        teloxide::RequestError::Api(teloxide::ApiError::InvalidToken) => GetMeFalha::TokenInvalido,
        teloxide::RequestError::Network(_) | teloxide::RequestError::Io(_) => {
            GetMeFalha::Transiente
        }
        _ => GetMeFalha::Outro,
    }
}

/// #1594: valida o `GetMe` com retry para falha transitória de rede.
///
/// - [`GetMeFalha::TokenInvalido`] e [`GetMeFalha::Outro`] retornam na
///   primeira tentativa — repetir não muda a resposta da API.
/// - [`GetMeFalha::Transiente`] re-tenta com backoff exponencial
///   (`backoff_base_ms * 2^(n-1)`) até esgotar as tentativas; o último erro
///   é devolvido com a contagem para a mensagem final.
///
/// Genérica no retorno e na closure de propósito: a política (quando retry
/// ajuda, quanto espera) é testável sem um `Bot` real e sem rede.
pub(crate) async fn valida_get_me<T, F, Fut>(
    mut tentar: F,
    tentativas: u32,
    backoff_base_ms: u64,
) -> std::result::Result<T, (GetMeFalha, teloxide::RequestError, u32)>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = std::result::Result<T, teloxide::RequestError>>,
{
    assert!(tentativas >= 1, "ao menos uma tentativa");
    let mut ultimo: Option<(GetMeFalha, teloxide::RequestError, u32)> = None;
    for tentativa in 1..=tentativas {
        match tentar().await {
            Ok(valor) => return Ok(valor),
            Err(e) => {
                let falha = classifica_get_me(&e);
                match falha {
                    // Token errado: a API já respondeu, não há o que esperar.
                    GetMeFalha::TokenInvalido => return Err((falha, e, tentativa)),
                    // Transitório: vale re-tentar enquanto há tentativa.
                    GetMeFalha::Transiente if tentativa < tentativas => {
                        let espera_ms = backoff_base_ms * 2u64.pow(tentativa - 1);
                        warn!(
                            tentativa,
                            total = tentativas,
                            classe = ?falha,
                            espera_ms,
                            erro = %e,
                            erro_debug = ?e,
                            "telegram GetMe falhou com erro transitório de rede; re-tentando"
                        );
                        tokio::time::sleep(Duration::from_millis(espera_ms)).await;
                        ultimo = Some((falha, e, tentativa));
                    }
                    // Transitório esgotado, ou classe não-retryable.
                    _ => return Err((falha, e, tentativa)),
                }
            }
        }
    }
    Err(ultimo.expect("tentativas >= 1 garante ao menos um erro registrado"))
}

/// Extracts chat ID and user info from a message.
/// Returns None if the message should be ignored (e.g. from a bot or missing sender).
fn extract_message_info(msg: &teloxide::types::Message) -> Option<(i64, String, String)> {
    // Ignore messages without a sender (e.g. channel posts)
    let user = msg.from.as_ref()?;

    // Telegram "Group Anonymous Bot" ID used for anonymous admins.
    const ANONYMOUS_BOT_ID: u64 = 1087968824;

    // Ignore bots to prevent loops, but allow anonymous admins.
    if user.is_bot && user.id.0 != ANONYMOUS_BOT_ID {
        // Only log at debug/trace level to avoid spam, or warn if unexpected
        return None;
    }

    let chat_id = msg.chat.id;
    let user_id = user.id.0.to_string();
    let user_name = user.first_name.clone();

    Some((chat_id.0, user_id, user_name))
}

#[async_trait]
impl Channel for TelegramChannel {
    fn channel_type(&self) -> &str {
        "telegram"
    }

    fn display_name(&self) -> &str {
        &self.display
    }

    async fn connect(&mut self) -> Result<()> {
        let bot = Bot::new(&self.bot_token);

        // Validate token before starting polling.
        //
        // #1594: retry com backoff para falha transitória de rede e
        // mensagem honesta por classe — "token inválido" só quando a API
        // do Telegram respondeu Unauthorized/Not Found. Rede/DNS/SSL sob
        // Termux não manda o operador trocar um token que está certo.
        const GET_ME_TENTATIVAS: u32 = 3;
        const GET_ME_BACKOFF_BASE_MS: u64 = 1_000;
        // O `.await` vive DENTRO da closure: `JsonRequest<GetMe>` só vira
        // future quando polido via `Requester`, e o tipo concreto não é
        // nomeável aqui — o async-block resolve sem expor o `Pending`.
        let bot_info = match valida_get_me(
            || async { bot.get_me().await },
            GET_ME_TENTATIVAS,
            GET_ME_BACKOFF_BASE_MS,
        )
        .await
        {
            Ok(bot_info) => bot_info,
            Err((falha, e, tentativas)) => {
                self.status = ChannelStatus::Disconnected;
                return Err(match falha {
                    GetMeFalha::TokenInvalido => {
                        error!(classe = ?falha, tentativas, erro = %e, "Telegram token validation failed");
                        garraia_common::Error::Channel(format!(
                            "Telegram token inválido: {}. Verifique o token no arquivo .env",
                            e
                        ))
                    }
                    _ => {
                        error!(
                            classe = ?falha,
                            tentativas,
                            erro = %e,
                            erro_debug = ?e,
                            "Telegram GetMe falhou sem ser problema de token"
                        );
                        garraia_common::Error::Channel(format!(
                            "Falha de rede ao validar o bot do Telegram após {} tentativa(s): {}. \
                             Não é o token — verifique conectividade/DNS/SSL do ambiente (Termux: \
                             SSL_CERT_FILE, veja o bloco Termux do `garraia doctor`). Detalhe: {:?}",
                            tentativas, e, e
                        ))
                    }
                });
            }
        };
        info!(
            "Telegram bot validated: @{}",
            bot_info.username.as_deref().unwrap_or("unknown")
        );

        self.bot = Some(bot.clone());

        // ── Register commands via setMyCommands ─────────────────────
        if !self.commands_for_menu.is_empty() {
            let bot_commands: Vec<BotCommand> = self
                .commands_for_menu
                .iter()
                .map(|(name, desc)| BotCommand::new(name.clone(), desc.clone()))
                .collect();
            let count = bot_commands.len();
            match bot.set_my_commands(bot_commands).await {
                Ok(_) => info!("telegram: registered {count} commands via setMyCommands"),
                Err(e) => warn!("telegram: failed to set commands: {e}"),
            }
        }

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        self.shutdown_tx = Some(shutdown_tx);

        let on_message = Arc::clone(&self.on_message);
        let on_voice = self.on_voice.clone();

        tokio::spawn(async move {
            let handler = Update::filter_message()
                .endpoint(
                    move |bot: Bot, msg: teloxide::types::Message| {
                        let on_message = Arc::clone(&on_message);
                        let on_voice = on_voice.clone();
                        async move {
                            let (chat_id_raw, user_id, user_name) = match extract_message_info(&msg) {
                                Some(info) => info,
                                None => return respond(()),
                            };

                            // ── Voice message handling ──────────────────────────
                            if msg.voice().is_some() {
                                if let Some(ref voice_handler) = on_voice {
                                    info!(
                                        "telegram voice message from {} [uid={}] (chat {})",
                                        user_name, user_id, chat_id_raw
                                    );
                                    if let Err(e) = voice_handler(bot.clone(), msg).await
                                        && e != "__blocked__" {
                                            warn!("voice handler error for chat {}: {e}", chat_id_raw);
                                            let _ = bot
                                                .send_message(
                                                    ChatId(chat_id_raw),
                                                    format!("Sorry, an error occurred processing voice: {e}"),
                                                )
                                                .await;
                                        }
                                } else {
                                    info!(
                                        "telegram: voice message from {} ignored (voice handler not configured)",
                                        user_name
                                    );
                                    let _ = bot
                                        .send_message(
                                            ChatId(chat_id_raw),
                                            "Voice messages are not enabled. Start the server with --with-voice.",
                                        )
                                        .await;
                                }
                                return respond(());
                            }

                            // ── Text message handling ──────────────────────────
                            let text = match msg.text() {
                                Some(t) => t.to_string(),
                                None => {
                                    // Other message types (photos, stickers, etc.) — ignore
                                    tracing::trace!(
                                        "telegram: non-text, non-voice message from {} ignored",
                                        user_name
                                    );
                                    return respond(());
                                }
                            };

                            // ChatId wrapper for teloxide calls
                            let chat_id = ChatId(chat_id_raw);

                            info!(
                                "telegram message from {} [uid={}] (chat {}): {} chars",
                                user_name,
                                user_id,
                                chat_id,
                                text.len()
                            );

                            // Send typing indicator
                            let _ = bot.send_chat_action(chat_id, ChatAction::Typing).await;

                            // Create streaming channel
                            let (delta_tx, mut delta_rx) = mpsc::channel::<String>(64);

                            // Spawn callback
                            let callback_handle = tokio::spawn({
                                let on_message = Arc::clone(&on_message);
                                let user_id = user_id.clone();
                                let user_name = user_name.clone();
                                let text = text.clone();
                                async move {
                                    on_message(
                                        chat_id.0,
                                        user_id,
                                        user_name,
                                        text,
                                        Some(delta_tx),
                                    )
                                    .await
                                }
                            });

                            // Consume streaming deltas and edit message.
                            // Buffer for 1s before sending the first message so short
                            // responses appear as a single formatted message instead of
                            // flashing the first word then replacing it.
                            let mut accumulated = String::new();
                            let mut msg_id: Option<teloxide::types::MessageId> = None;
                            let mut last_edit = tokio::time::Instant::now();
                            let mut first_delta_at: Option<tokio::time::Instant> = None;

                            loop {
                                tokio::select! {
                                    delta = delta_rx.recv() => {
                                        match delta {
                                            Some(text) => {
                                                accumulated.push_str(&text);
                                                if first_delta_at.is_none() {
                                                    first_delta_at = Some(tokio::time::Instant::now());
                                                }

                                                if msg_id.is_none() {
                                                    // Only send after 1s buffer period
                                                    if first_delta_at.unwrap().elapsed() >= Duration::from_secs(1) {
                                                        match bot.send_message(chat_id, &accumulated).await {
                                                            Ok(sent) => {
                                                                msg_id = Some(sent.id);
                                                                last_edit = tokio::time::Instant::now();
                                                            }
                                                            Err(e) => {
                                                                error!("failed to send streaming message: {e}");
                                                                break;
                                                            }
                                                        }
                                                    }
                                                } else if last_edit.elapsed() >= Duration::from_millis(1000)
                                                    && let Some(id) = msg_id
                                                {
                                                    let _ = bot
                                                        .edit_message_text(chat_id, id, &accumulated)
                                                        .await;
                                                    last_edit = tokio::time::Instant::now();
                                                }
                                            }
                                            None => break, // Sender dropped — callback finished
                                        }
                                    }
                                    _ = tokio::time::sleep(Duration::from_secs(4)) => {
                                        // Keep typing indicator alive during pauses (e.g. tool execution)
                                        let _ = bot.send_chat_action(chat_id, ChatAction::Typing).await;
                                    }
                                }
                            }

                            // Get callback result
                            let result = callback_handle
                                .await
                                .unwrap_or_else(|e| Err(format!("task panic: {e}")));

                            match result {
                                Ok(final_text) => {
                                    if let Some(id) = msg_id {
                                        // Final edit with MarkdownV2 formatting
                                        let formatted = to_telegram_markdown(&final_text);
                                        let edit_result = bot
                                            .edit_message_text(chat_id, id, &formatted)
                                            .parse_mode(ParseMode::MarkdownV2)
                                            .await;
                                        if edit_result.is_err() {
                                            // Fallback: plain text
                                            let _ = bot
                                                .edit_message_text(chat_id, id, &final_text)
                                                .await;
                                        }
                                    } else {
                                        // No streaming happened (command response) — send directly
                                        let formatted = to_telegram_markdown(&final_text);
                                        let send_result = bot
                                            .send_message(chat_id, &formatted)
                                            .parse_mode(ParseMode::MarkdownV2)
                                            .await;
                                        if send_result.is_err() {
                                            // Fallback: plain text
                                            let _ =
                                                bot.send_message(chat_id, &final_text).await;
                                        }
                                    }
                                }
                                Err(e) if e == "__blocked__" => {
                                    // Silently drop — unauthorized user
                                }
                                Err(e) => {
                                    if let Some(id) = msg_id {
                                        let _ = bot
                                            .edit_message_text(
                                                chat_id,
                                                id,
                                                format!("Sorry, an error occurred: {e}"),
                                            )
                                            .await;
                                    } else {
                                        warn!(
                                            "agent error for telegram chat {}: {e}",
                                            chat_id
                                        );
                                        let _ = bot
                                            .send_message(
                                                chat_id,
                                                format!("Sorry, an error occurred: {e}"),
                                            )
                                            .await;
                                    }
                                }
                            }

                            respond(())
                        }
                    },
                );

            let mut dispatcher = Dispatcher::builder(bot, handler)
                .default_handler(|upd| async move {
                    tracing::trace!("unhandled update: {:?}", upd.kind);
                })
                .build();

            let token = dispatcher.shutdown_token();
            tokio::spawn(async move {
                let mut rx = shutdown_rx;
                while rx.changed().await.is_ok() {
                    if *rx.borrow() {
                        if let Err(e) = token.shutdown() {
                            warn!("telegram shutdown token error: {e:?}");
                        }
                        break;
                    }
                }
            });

            info!("telegram bot polling started");
            dispatcher.dispatch().await;
            info!("telegram bot polling stopped");
        });

        self.status = ChannelStatus::Connected;
        info!("telegram channel connected");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(true);
        }
        self.bot = None;
        self.status = ChannelStatus::Disconnected;
        info!("telegram channel disconnected");
        Ok(())
    }

    async fn send_message(&self, message: &Message) -> Result<()> {
        let bot = self
            .bot
            .as_ref()
            .ok_or_else(|| garraia_common::Error::Channel("telegram bot not connected".into()))?;

        let chat_id: i64 = message
            .metadata
            .get("telegram_chat_id")
            .and_then(|v| v.as_i64())
            .ok_or_else(|| {
                garraia_common::Error::Channel("missing telegram_chat_id in metadata".into())
            })?;

        // `Audio` chega do tool `telegram_send_voice`: o `url` e um CAMINHO
        // LOCAL para o WAV sintetizado e vai como voice message — o mesmo caminho que
        // o handler de voice message do bootstrap usa no sentido contrario.
        // Qualquer outro conteudo continua recusado, como antes.
        let audio: Option<&String> = match &message.content {
            MessageContent::Audio { url, .. } => Some(url),
            MessageContent::Text(_) => None,
            _ => {
                return Err(garraia_common::Error::Channel(
                    "only text and audio (voice) messages are supported for telegram send".into(),
                ));
            }
        };

        if let Some(url) = audio {
            // So caminho local: o tool grava o WAV sintetizado e passa o
            // caminho. URL remota nao e aceita aqui (nada de o modelo fazer o
            // bot buscar um endereco arbitrario).
            let path = std::path::Path::new(url);
            if !path.is_file() {
                return Err(garraia_common::Error::Channel(
                    "arquivo de audio nao encontrado".into(),
                ));
            }
            let input_file = teloxide::types::InputFile::file(path);
            bot.send_voice(ChatId(chat_id), input_file)
                .await
                .map_err(|e| {
                    garraia_common::Error::Channel(format!("telegram send_voice failed: {e}"))
                })?;
            return Ok(());
        }

        let text = match &message.content {
            MessageContent::Text(t) => t.clone(),
            _ => unreachable!("audio tratado acima"),
        };

        let formatted = to_telegram_markdown(&text);
        let send_result = bot
            .send_message(ChatId(chat_id), &formatted)
            .parse_mode(ParseMode::MarkdownV2)
            .await;
        if send_result.is_err() {
            // Fallback: plain text
            bot.send_message(ChatId(chat_id), text).await.map_err(|e| {
                garraia_common::Error::Channel(format!("telegram send failed: {e}"))
            })?;
        }

        Ok(())
    }

    fn status(&self) -> ChannelStatus {
        self.status.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_type_is_telegram() {
        let on_msg: OnMessageFn = Arc::new(|_chat_id, _uid, _user, _text, _delta_tx| {
            Box::pin(async { Ok("test".to_string()) })
        });
        let channel = TelegramChannel::new("fake-token".to_string(), on_msg);
        assert_eq!(channel.channel_type(), "telegram");
        assert_eq!(channel.display_name(), "Telegram");
        assert_eq!(channel.status(), ChannelStatus::Disconnected);
    }

    // ── #1594: classificação e retry do GetMe ────────────────────────────

    /// Um `RequestError` de rede de verdade: conexão recusada em porta
    /// fechada do loopback — o único que um teste provoca sem rede externa.
    async fn erro_de_rede() -> teloxide::RequestError {
        let porta = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind efemero");
            l.local_addr().expect("addr").port()
        };
        reqwest::Client::new()
            .get(format!("http://127.0.0.1:{porta}/"))
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .expect_err("ninguem escuta nessa porta")
            .into()
    }

    /// Unauthorized/Not Found é token inválido (teloxide 0.17 mapeia os dois
    /// para `InvalidToken`); rede e IO são transitórios; o resto é "outro".
    #[test]
    fn classifica_get_me_distingue_token_de_rede() {
        use teloxide::RequestError;
        let invalido: RequestError = teloxide::ApiError::InvalidToken.into();
        assert_eq!(classifica_get_me(&invalido), GetMeFalha::TokenInvalido);

        let io: RequestError = std::sync::Arc::new(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "reset",
        ))
        .into();
        assert_eq!(classifica_get_me(&io), GetMeFalha::Transiente);

        let migracao = RequestError::MigrateToChatId(teloxide::types::ChatId(1));
        assert_eq!(classifica_get_me(&migracao), GetMeFalha::Outro);
    }

    /// O defeito da #1594 em uma frase: rede caída NÃO é "token inválido".
    /// A classificação da rede de verdade (recusa de conexão) tem de ser
    /// transitória — é o caso do Termux com SSL/DNS quebrado.
    #[tokio::test]
    async fn rede_caída_nao_e_classificada_como_token_invalido() {
        let e = erro_de_rede().await;
        assert_eq!(classifica_get_me(&e), GetMeFalha::Transiente);
    }

    /// Política de retry: transitório re-tenta e pode recuperar; token
    /// inválido falha na primeira (repetir Unauthorized não muda resposta);
    /// transitório esgotado devolve a contagem certa.
    #[tokio::test]
    async fn retry_do_get_me_segue_a_politica_de_classe() {
        // Transitório duas vezes, sucesso na terceira: recupera.
        let mut chamadas = 0u32;
        let ok = valida_get_me(
            || {
                chamadas += 1;
                let n = chamadas;
                async move {
                    if n < 3 {
                        Err(std::sync::Arc::new(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "timeout",
                        ))
                        .into())
                    } else {
                        Ok::<_, teloxide::RequestError>("bot-ok")
                    }
                }
            },
            3,
            1, // backoff de 1ms: o teste mede política, nao espera real
        )
        .await
        .expect("transitório com tentativa sobrando tem de recuperar");
        assert_eq!(ok, "bot-ok");
        assert_eq!(chamadas, 3);

        // Token inválido: uma chamada só, sem sleep.
        let mut chamadas = 0u32;
        let erro = valida_get_me(
            || {
                chamadas += 1;
                async { Err::<&str, _>(teloxide::ApiError::InvalidToken.into()) }
            },
            3,
            1,
        )
        .await
        .expect_err("token invalido nao pode recuperar");
        assert_eq!(erro.0, GetMeFalha::TokenInvalido);
        assert_eq!(chamadas, 1, "retry de token invalido e desperdicio");

        // Transitório até esgotar: devolve a contagem de tentativas.
        let mut chamadas = 0u32;
        let erro = valida_get_me(
            || {
                chamadas += 1;
                async {
                    Err::<&str, _>(
                        std::sync::Arc::new(std::io::Error::new(
                            std::io::ErrorKind::ConnectionReset,
                            "reset",
                        ))
                        .into(),
                    )
                }
            },
            3,
            1,
        )
        .await
        .expect_err("sem tentativa sobrando nao ha recuperacao");
        assert_eq!(erro.0, GetMeFalha::Transiente);
        assert_eq!(erro.2, 3, "a mensagem final leva o total de tentativas");
        assert_eq!(chamadas, 3);
    }

    #[test]
    fn test_extract_message_info_private() {
        // Construct a private message JSON
        let json = r#"{
            "message_id": 1,
            "date": 1620000000,
            "chat": {
                "id": 12345,
                "type": "private",
                "first_name": "Alice"
            },
            "from": {
                "id": 111,
                "is_bot": false,
                "first_name": "Alice",
                "username": "alice"
            },
            "text": "hello"
        }"#;
        let msg: teloxide::types::Message =
            serde_json::from_str(json).expect("failed to parse json");

        let info = extract_message_info(&msg).expect("should extract info");
        assert_eq!(info.0, 12345);
        assert_eq!(info.1, "111");
        assert_eq!(info.2, "Alice");
    }

    #[test]
    fn test_extract_message_info_group() {
        // Construct a group message JSON (negative chat_id)
        let json = r#"{
            "message_id": 2,
            "date": 1620000000,
            "chat": {
                "id": -987654321,
                "type": "supergroup",
                "title": "My Group"
            },
            "from": {
                "id": 222,
                "is_bot": false,
                "first_name": "Bob"
            },
            "text": "hello group"
        }"#;
        let msg: teloxide::types::Message =
            serde_json::from_str(json).expect("failed to parse json");

        let info = extract_message_info(&msg).expect("should extract info");
        assert_eq!(info.0, -987654321);
        assert_eq!(info.1, "222");
        assert_eq!(info.2, "Bob");
    }

    #[test]
    fn test_extract_message_info_bot_ignored() {
        // Message from a bot
        let json = r#"{
            "message_id": 3,
            "date": 1620000000,
            "chat": {
                "id": 12345,
                "type": "private"
            },
            "from": {
                "id": 333,
                "is_bot": true,
                "first_name": "SomeBot"
            },
            "text": "I am a bot"
        }"#;
        let msg: teloxide::types::Message =
            serde_json::from_str(json).expect("failed to parse json");

        let info = extract_message_info(&msg);
        assert!(info.is_none(), "should ignore bot messages");
    }

    #[test]
    fn test_extract_message_info_anonymous_admin_allowed() {
        // Message from Group Anonymous Bot (ID 1087968824)
        let json = r#"{
            "message_id": 5,
            "date": 1620000000,
            "chat": {
                "id": -987654321,
                "type": "supergroup",
                "title": "My Group"
            },
            "from": {
                "id": 1087968824,
                "is_bot": true,
                "first_name": "Group Anonymous Bot",
                "username": "GroupAnonymousBot"
            },
            "sender_chat": {
                 "id": -987654321,
                 "type": "supergroup",
                 "title": "My Group"
            },
            "text": "admin command"
        }"#;
        let msg: teloxide::types::Message =
            serde_json::from_str(json).expect("failed to parse json");

        let info = extract_message_info(&msg).expect("should allow anonymous admin");
        assert_eq!(info.0, -987654321);
        assert_eq!(info.1, "1087968824");
        assert_eq!(info.2, "Group Anonymous Bot");
    }

    #[test]
    fn test_extract_message_info_channel_post_ignored() {
        // Channel post often lacks 'from' or behaves differently.
        // If we simulate a message without 'from' (if possible in teloxide types).
        // Standard messages usually have 'from', but let's try to omit it.
        // teloxide::types::Message 'from' is Option<User>.
        let json = r#"{
            "message_id": 4,
            "date": 1620000000,
            "chat": {
                "id": -1001234567890,
                "type": "channel",
                "title": "My Channel"
            },
            "text": "channel post"
        }"#;
        let msg: teloxide::types::Message =
            serde_json::from_str(json).expect("failed to parse json");

        let info = extract_message_info(&msg);
        assert!(
            info.is_none(),
            "should ignore messages without sender (channel posts)"
        );
    }
}
