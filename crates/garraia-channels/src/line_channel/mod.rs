//! LINE Messaging API channel implementation for GarraIA.
//!
//! Provides a `LineChannel` struct that implements the `Channel` trait,
//! communicating via the LINE Messaging API webhooks and REST endpoints.

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

pub use config::LineConfig;

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
    pub fn new(config: LineConfig, on_message: LineOnMessageFn) -> Self {
        Self {
            config,
            client: Client::new(),
            status: ChannelStatus::Disconnected,
            on_message,
        }
    }

    /// Access the current config.
    pub fn config(&self) -> &LineConfig {
        &self.config
    }

    /// Get the channel secret for webhook signature verification.
    pub fn channel_secret(&self) -> &str {
        &self.config.channel_secret
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
            return Err(Error::Channel(format!(
                "line reply error {status}: {body}"
            )));
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
            return Err(Error::Channel(format!(
                "line push error {status}: {body}"
            )));
        }

        Ok(())
    }

    /// Validate a webhook signature using HMAC-SHA256.
    pub fn validate_signature(&self, body: &[u8], signature: &str) -> bool {
        use std::io::Write;
        // HMAC-SHA256 validation of LINE webhook signature.
        // In production, use ring or hmac crate. Stub returns true for now.
        let _ = (body, signature);
        let _ = std::io::sink().write(b"");
        // TODO: implement HMAC-SHA256 validation with ring
        true
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

        if let Some(reply_token) = message.metadata.get("line_reply_token").and_then(|v| v.as_str()) {
            self.reply_message(reply_token, &text).await
        } else if let Some(to) = message.metadata.get("line_user_id").and_then(|v| v.as_str()) {
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
        let on_msg: LineOnMessageFn =
            Arc::new(|_reply, _uid, _user, _text, _delta_tx| {
                Box::pin(async { Ok("test".to_string()) })
            });
        let config = LineConfig {
            channel_access_token: "test-token".into(),
            channel_secret: "test-secret".into(),
        };
        let channel = LineChannel::new(config, on_msg);
        assert_eq!(channel.channel_type(), "line");
        assert_eq!(channel.display_name(), "LINE");
        assert_eq!(channel.status(), ChannelStatus::Disconnected);
    }
}
