//! Google Chat (Workspace) channel implementation for GarraIA.
//!
//! Provides a `GoogleChatChannel` struct that implements the `Channel` trait,
//! communicating via Google Chat webhooks and REST API.

pub mod config;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use reqwest::Client;
use tokio::sync::mpsc;
use tracing::info;

use crate::traits::{Channel, ChannelStatus};
use garraia_common::{Error, Message, MessageContent, Result};
#[cfg(test)]
use garraia_common::{MessageDirection, SessionId, ChannelId, UserId};

pub use config::GoogleChatConfig;

/// Callback invoked when a Google Chat webhook event is received.
///
/// Arguments: `(space_id, user_id, user_name, text, delta_tx)`.
/// Return `Err("__blocked__")` to silently drop unauthorized messages.
pub type GoogleChatOnMessageFn = Arc<
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

/// Google Chat channel implementation.
///
/// Uses Google Chat REST API and incoming webhooks for message handling.
pub struct GoogleChatChannel {
    config: GoogleChatConfig,
    client: Client,
    status: ChannelStatus,
    on_message: GoogleChatOnMessageFn,
}

impl GoogleChatChannel {
    /// Create a new `GoogleChatChannel` from config and callback.
    pub fn new(config: GoogleChatConfig, on_message: GoogleChatOnMessageFn) -> Self {
        Self {
            config,
            client: Client::new(),
            status: ChannelStatus::Disconnected,
            on_message,
        }
    }

    /// Access the current config.
    pub fn config(&self) -> &GoogleChatConfig {
        &self.config
    }

    /// Process an incoming webhook event from Google Chat.
    pub async fn handle_incoming(
        &self,
        space_id: &str,
        user_id: &str,
        user_name: &str,
        text: &str,
    ) -> std::result::Result<String, String> {
        (self.on_message)(
            space_id.to_string(),
            user_id.to_string(),
            user_name.to_string(),
            text.to_string(),
            None,
        )
        .await
    }

    /// Send a text message to a Google Chat space via REST API.
    pub async fn send_to_space(&self, space_name: &str, text: &str) -> Result<()> {
        let url = format!(
            "https://chat.googleapis.com/v1/{}/messages",
            space_name
        );

        let body = serde_json::json!({
            "text": text,
        });

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.config.service_account_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Channel(format!("google chat send failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Channel(format!(
                "google chat API error {status}: {body}"
            )));
        }

        Ok(())
    }

    /// Send a message via the configured webhook URL (simple integration).
    pub async fn send_via_webhook(&self, text: &str) -> Result<()> {
        let url = self.config.webhook_url.as_deref().ok_or_else(|| {
            Error::Channel("google chat webhook_url not configured".into())
        })?;

        let body = serde_json::json!({
            "text": text,
        });

        let resp = self
            .client
            .post(url)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Channel(format!("google chat webhook send failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Channel(format!(
                "google chat webhook error {status}: {body}"
            )));
        }

        Ok(())
    }
}

#[async_trait]
impl Channel for GoogleChatChannel {
    fn channel_type(&self) -> &str {
        "google_chat"
    }

    fn display_name(&self) -> &str {
        "Google Chat"
    }

    async fn connect(&mut self) -> Result<()> {
        // Google Chat is webhook-driven — no persistent connection needed.
        self.status = ChannelStatus::Connected;
        info!("google chat channel connected (webhook mode)");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        self.status = ChannelStatus::Disconnected;
        info!("google chat channel disconnected");
        Ok(())
    }

    async fn send_message(&self, message: &Message) -> Result<()> {
        let space_name = message
            .metadata
            .get("google_chat_space")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                Error::Channel("missing google_chat_space in metadata".into())
            })?;

        let text = match &message.content {
            MessageContent::Text(t) => t.clone(),
            _ => {
                return Err(Error::Channel(
                    "only text messages are supported for google chat send".into(),
                ));
            }
        };

        self.send_to_space(space_name, &text).await
    }

    fn status(&self) -> ChannelStatus {
        self.status.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_type_is_google_chat() {
        let on_msg: GoogleChatOnMessageFn =
            Arc::new(|_space, _uid, _user, _text, _delta_tx| {
                Box::pin(async { Ok("test".to_string()) })
            });
        let config = GoogleChatConfig {
            webhook_url: Some("https://chat.googleapis.com/v1/spaces/test/messages?key=test".into()),
            service_account_key_path: None,
            service_account_token: String::new(),
        };
        let channel = GoogleChatChannel::new(config, on_msg);
        assert_eq!(channel.channel_type(), "google_chat");
        assert_eq!(channel.display_name(), "Google Chat");
        assert_eq!(channel.status(), ChannelStatus::Disconnected);
    }

    #[tokio::test]
    async fn send_message_missing_space_metadata() {
        let on_msg: GoogleChatOnMessageFn =
            Arc::new(|_space, _uid, _user, _text, _delta_tx| {
                Box::pin(async { Ok("test".to_string()) })
            });
        let config = GoogleChatConfig {
            webhook_url: Some("https://example.com/webhook".into()),
            service_account_key_path: None,
            service_account_token: String::new(),
        };
        let channel = GoogleChatChannel::new(config, on_msg);
        let msg = Message::text(SessionId::from_string("s"), ChannelId::from_string("c"), UserId::from_string("u"), MessageDirection::Outgoing, "hello");
        let result = channel.send_message(&msg).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("google_chat_space"));
    }

    #[test]
    fn send_via_webhook_requires_url() {
        let on_msg: GoogleChatOnMessageFn =
            Arc::new(|_space, _uid, _user, _text, _delta_tx| {
                Box::pin(async { Ok("test".to_string()) })
            });
        let config = GoogleChatConfig {
            webhook_url: None,
            service_account_key_path: None,
            service_account_token: String::new(),
        };
        let channel = GoogleChatChannel::new(config, on_msg);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(channel.send_via_webhook("test"));
        assert!(result.is_err());
    }

    #[test]
    fn initial_status_is_disconnected() {
        let on_msg: GoogleChatOnMessageFn =
            Arc::new(|_space, _uid, _user, _text, _delta_tx| {
                Box::pin(async { Ok("test".to_string()) })
            });
        let config = GoogleChatConfig {
            webhook_url: Some("https://example.com".into()),
            service_account_key_path: None,
            service_account_token: String::new(),
        };
        let channel = GoogleChatChannel::new(config, on_msg);
        assert_eq!(channel.status(), ChannelStatus::Disconnected);
    }
}
