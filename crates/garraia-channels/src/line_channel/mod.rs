//! LINE Messaging API channel implementation for GarraIA.
//!
//! Provides a `LineChannel` struct that implements the `Channel` trait,
//! communicating via the LINE Messaging API webhooks and REST endpoints.

pub mod config;
pub mod signature;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use reqwest::Client;
use tokio::sync::mpsc;
use tracing::{info, warn};

use crate::traits::{Channel, ChannelStatus};
use garraia_common::{Error, Message, MessageContent, Result};

pub use config::LineConfig;

pub mod webhook;

/// LINE Messaging API base URL.
const LINE_API_BASE: &str = "https://api.line.me/v2/bot";

/// Callback invoked when a LINE webhook event is received.
///
/// Arguments: `(reply_token, user_id, user_name, text, delta_tx)`.
/// Return `Err("__blocked__")` to silently drop unauthorized messages.
pub type LineOnMessageFn = Arc<
    dyn Fn(
            String,
            String,
            String,
            String,
            Option<mpsc::Sender<String>>,
        ) -> Pin<Box<dyn Future<Output = std::result::Result<String, String>> + Send>>
        + Send
        + Sync,
>;

/// LINE Messaging API channel implementation.
///
/// Uses webhook-driven incoming messages and REST API for replies.
pub struct LineChannel {
    config: LineConfig,
    client: Client,
    status: ChannelStatus,
    on_message: LineOnMessageFn,
}

impl LineChannel {
    /// Create a new `LineChannel` from config and callback.
    ///
    /// Recusa a construcao quando `channel_secret` esta vazio (#1051): sem
    /// segredo nao ha como verificar a assinatura do webhook, e um canal
    /// que aceita qualquer POST nao deve existir nem por engano. E aqui,
    /// e nao no bootstrap, porque assim nenhum wiring futuro consegue
    /// pular a checagem — o compilador obriga a tratar o erro.
    pub fn new(config: LineConfig, on_message: LineOnMessageFn) -> Result<Self> {
        if config.channel_secret.trim().is_empty() {
            return Err(Error::Channel(
                "line: channel_secret e obrigatorio para verificar a assinatura do webhook".into(),
            ));
        }
        Ok(Self {
            config,
            client: Client::new(),
            status: ChannelStatus::Disconnected,
            on_message,
        })
    }

    /// Access the current config.
    pub fn config(&self) -> &LineConfig {
        &self.config
    }

    /// Get the channel secret for webhook signature verification.
    pub fn channel_secret(&self) -> &str {
        &self.config.channel_secret
    }

    /// Nome da secao de config que originou este canal.
    ///
    /// Diferente de [`Channel::display_name`], que e a constante `"LINE"`
    /// para todos: com dois canais LINE configurados, o log do webhook
    /// precisa dizer de qual deles se trata.
    pub fn name(&self) -> &str {
        &self.config.name
    }

    /// Process an incoming LINE webhook event.
    pub async fn handle_incoming(
        &self,
        reply_token: &str,
        user_id: &str,
        text: &str,
    ) -> std::result::Result<String, String> {
        (self.on_message)(
            reply_token.to_string(),
            user_id.to_string(),
            user_id.to_string(), // LINE doesn't always provide display name in webhook
            text.to_string(),
            None,
        )
        .await
    }

    /// Reply to a message using the reply token.
    pub async fn reply_message(&self, reply_token: &str, text: &str) -> Result<()> {
        let url = format!("{}/message/reply", LINE_API_BASE);

        let body = serde_json::json!({
            "replyToken": reply_token,
            "messages": [
                {
                    "type": "text",
                    "text": text
                }
            ]
        });

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.config.channel_access_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Channel(format!("line reply failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Channel(format!("line reply error {status}: {body}")));
        }

        Ok(())
    }

    /// Push a message to a user (no reply token needed).
    pub async fn push_message(&self, to: &str, text: &str) -> Result<()> {
        let url = format!("{}/message/push", LINE_API_BASE);

        let body = serde_json::json!({
            "to": to,
            "messages": [
                {
                    "type": "text",
                    "text": text
                }
            ]
        });

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.config.channel_access_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Channel(format!("line push failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Channel(format!("line push error {status}: {body}")));
        }

        Ok(())
    }

    /// Verifica a assinatura de um webhook do LINE.
    ///
    /// `body` tem de ser o corpo cru da requisicao, byte a byte, lido
    /// antes de qualquer parse de JSON — reserializar o valor ja
    /// desserializado muda espacos e ordem de chaves e invalida a
    /// assinatura.
    ///
    /// Devolve `Ok(())` quando confere. O motivo da recusa serve para
    /// log; a resposta HTTP deve ser 403 em todos os casos, sem
    /// distinguir qual deles ocorreu.
    pub fn verify_webhook_signature(
        &self,
        body: &[u8],
        signature: &str,
    ) -> std::result::Result<(), signature::SignatureError> {
        signature::verify_signature(&self.config.channel_secret, body, signature)
    }

    /// Versao booleana de [`Self::verify_webhook_signature`], para quem so
    /// precisa decidir entre seguir e devolver 403.
    pub fn validate_signature(&self, body: &[u8], signature: &str) -> bool {
        match self.verify_webhook_signature(body, signature) {
            Ok(()) => true,
            Err(reason) => {
                warn!(%reason, "line: webhook com assinatura invalida recusado");
                false
            }
        }
    }
}

#[async_trait]
impl Channel for LineChannel {
    fn channel_type(&self) -> &str {
        "line"
    }

    fn display_name(&self) -> &str {
        "LINE"
    }

    async fn connect(&mut self) -> Result<()> {
        // LINE is webhook-driven — no persistent connection needed.
        self.status = ChannelStatus::Connected;
        info!("line channel connected (webhook mode)");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        self.status = ChannelStatus::Disconnected;
        info!("line channel disconnected");
        Ok(())
    }

    async fn send_message(&self, message: &Message) -> Result<()> {
        // Try reply_token first, fall back to push message
        let text = match &message.content {
            MessageContent::Text(t) => t.clone(),
            _ => {
                return Err(Error::Channel(
                    "only text messages are supported for line send".into(),
                ));
            }
        };

        if let Some(reply_token) = message
            .metadata
            .get("line_reply_token")
            .and_then(|v| v.as_str())
        {
            self.reply_message(reply_token, &text).await
        } else if let Some(to) = message
            .metadata
            .get("line_user_id")
            .and_then(|v| v.as_str())
        {
            self.push_message(to, &text).await
        } else {
            Err(Error::Channel(
                "missing line_reply_token or line_user_id in metadata".into(),
            ))
        }
    }

    fn status(&self) -> ChannelStatus {
        self.status.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_type_is_line() {
        let on_msg: LineOnMessageFn = Arc::new(|_reply, _uid, _user, _text, _delta_tx| {
            Box::pin(async { Ok("test".to_string()) })
        });
        let config = LineConfig {
            channel_access_token: "test-token".into(),
            channel_secret: "test-secret".into(),
            name: "line-teste".into(),
        };
        let channel = LineChannel::new(config, on_msg).expect("secret nao vazio");
        assert_eq!(channel.channel_type(), "line");
        assert_eq!(channel.display_name(), "LINE");
        assert_eq!(channel.status(), ChannelStatus::Disconnected);
    }

    #[tokio::test]
    async fn send_message_missing_reply_token() {
        let on_msg: LineOnMessageFn = Arc::new(|_reply, _uid, _user, _text, _delta_tx| {
            Box::pin(async { Ok("test".to_string()) })
        });
        let config = LineConfig {
            channel_access_token: "test-token".into(),
            channel_secret: "test-secret".into(),
            name: "line-teste".into(),
        };
        let channel = LineChannel::new(config, on_msg).expect("secret nao vazio");
        let msg = Message::text(
            garraia_common::types::SessionId::from_string("test-session"),
            garraia_common::types::ChannelId::from_string("test-channel"),
            garraia_common::types::UserId::from_string("test-user"),
            garraia_common::MessageDirection::Outgoing,
            "hello",
        );
        let result = channel.send_message(&msg).await;
        assert!(result.is_err());
    }

    #[test]
    fn initial_status_is_disconnected() {
        let on_msg: LineOnMessageFn = Arc::new(|_reply, _uid, _user, _text, _delta_tx| {
            Box::pin(async { Ok("test".to_string()) })
        });
        let config = LineConfig {
            channel_access_token: "token".into(),
            channel_secret: "secret".into(),
            name: "line-teste".into(),
        };
        let channel = LineChannel::new(config, on_msg).expect("secret nao vazio");
        assert_eq!(channel.status(), ChannelStatus::Disconnected);
    }

    /// #1051: sem `channel_secret` o canal nao existe. Um LineChannel
    /// construido assim aceitaria qualquer POST forjado.
    #[test]
    fn sem_channel_secret_o_canal_nao_e_construido() {
        let on_msg: LineOnMessageFn = Arc::new(|_reply, _uid, _user, _text, _delta_tx| {
            Box::pin(async { Ok("test".to_string()) })
        });
        for vazio in ["", "   "] {
            let config = LineConfig {
                channel_access_token: "t".into(),
                channel_secret: vazio.into(),
                name: "line-teste".into(),
            };
            assert!(
                LineChannel::new(config, on_msg.clone()).is_err(),
                "channel_secret {vazio:?} deveria ser recusado"
            );
        }
    }

    #[test]
    fn display_name_is_line() {
        let on_msg: LineOnMessageFn = Arc::new(|_reply, _uid, _user, _text, _delta_tx| {
            Box::pin(async { Ok("test".to_string()) })
        });
        let config = LineConfig {
            channel_access_token: "t".into(),
            channel_secret: "s".into(),
            name: "line-teste".into(),
        };
        let channel = LineChannel::new(config, on_msg).expect("secret nao vazio");
        assert_eq!(channel.display_name(), "LINE");
    }
}
