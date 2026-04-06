//! Microsoft Teams (Graph API) channel implementation for GarraIA.
//!
//! Provides a `TeamsChannel` struct that implements the `Channel` trait,
//! communicating via Microsoft Bot Framework and Graph API.

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

pub use config::TeamsConfig;

/// Callback invoked when a Teams Bot Framework webhook event is received.
///
/// Arguments: `(conversation_id, user_id, user_name, text, delta_tx)`.
/// Return `Err("__blocked__")` to silently drop unauthorized messages.
pub type TeamsOnMessageFn = Arc<
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

/// Microsoft Teams channel implementation.
///
/// Uses the Bot Framework webhook model for receiving messages and
/// the Graph API for sending replies.
pub struct TeamsChannel {
    config: TeamsConfig,
    client: Client,
    status: ChannelStatus,
    on_message: TeamsOnMessageFn,
    /// OAuth2 access token for Graph API calls, obtained at runtime.
    access_token: Option<String>,
}

impl TeamsChannel {
    /// Create a new `TeamsChannel` from config and callback.
    pub fn new(config: TeamsConfig, on_message: TeamsOnMessageFn) -> Self {
        Self {
            config,
            client: Client::new(),
            status: ChannelStatus::Disconnected,
            on_message,
            access_token: None,
        }
    }

    /// Access the current config.
    pub fn config(&self) -> &TeamsConfig {
        &self.config
    }

    /// Process an incoming Bot Framework activity.
    pub async fn handle_incoming(
        &self,
        conversation_id: &str,
        user_id: &str,
        user_name: &str,
        text: &str,
    ) -> std::result::Result<String, String> {
        (self.on_message)(
            conversation_id.to_string(),
            user_id.to_string(),
            user_name.to_string(),
            text.to_string(),
            None,
        )
        .await
    }

    /// Obtain an OAuth2 access token from Azure AD using client credentials.
    pub async fn authenticate(&mut self) -> Result<()> {
        let url = format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
            self.config.tenant_id
        );

        let params = [
            ("client_id", self.config.app_id.as_str()),
            ("client_secret", self.config.app_secret.as_str()),
            ("scope", "https://graph.microsoft.com/.default"),
            ("grant_type", "client_credentials"),
        ];

        let resp = self
            .client
            .post(&url)
            .form(&params)
            .send()
            .await
            .map_err(|e| Error::Channel(format!("teams auth failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Channel(format!(
                "teams auth error {status}: {body}"
            )));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| Error::Channel(format!("teams auth parse failed: {e}")))?;

        let token = body
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Channel("teams: missing access_token in auth response".into()))?
            .to_string();

        self.access_token = Some(token);
        Ok(())
    }

    /// Send a reply to a Teams conversation via Bot Framework REST API.
    pub async fn send_activity(
        &self,
        service_url: &str,
        conversation_id: &str,
        text: &str,
    ) -> Result<()> {
        let token = self.access_token.as_deref().ok_or_else(|| {
            Error::Channel("teams: not authenticated, call authenticate() first".into())
        })?;

        let url = format!(
            "{}/v3/conversations/{}/activities",
            service_url.trim_end_matches('/'),
            conversation_id
        );

        let body = serde_json::json!({
            "type": "message",
            "text": text,
        });

        let resp = self
            .client
            .post(&url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Channel(format!("teams send failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Channel(format!(
                "teams send error {status}: {body}"
            )));
        }

        Ok(())
    }
}

#[async_trait]
impl Channel for TeamsChannel {
    fn channel_type(&self) -> &str {
        "teams"
    }

    fn display_name(&self) -> &str {
        "Microsoft Teams"
    }

    async fn connect(&mut self) -> Result<()> {
        // Authenticate with Azure AD to get an access token.
        self.authenticate().await?;
        self.status = ChannelStatus::Connected;
        info!("teams channel connected (bot framework mode)");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        self.access_token = None;
        self.status = ChannelStatus::Disconnected;
        info!("teams channel disconnected");
        Ok(())
    }

    async fn send_message(&self, message: &Message) -> Result<()> {
        let service_url = message
            .metadata
            .get("teams_service_url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                Error::Channel("missing teams_service_url in metadata".into())
            })?;

        let conversation_id = message
            .metadata
            .get("teams_conversation_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                Error::Channel("missing teams_conversation_id in metadata".into())
            })?;

        let text = match &message.content {
            MessageContent::Text(t) => t.clone(),
            _ => {
                return Err(Error::Channel(
                    "only text messages are supported for teams send".into(),
                ));
            }
        };

        self.send_activity(service_url, conversation_id, &text).await
    }

    fn status(&self) -> ChannelStatus {
        self.status.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_type_is_teams() {
        let on_msg: TeamsOnMessageFn =
            Arc::new(|_conv, _uid, _user, _text, _delta_tx| {
                Box::pin(async { Ok("test".to_string()) })
            });
        let config = TeamsConfig {
            app_id: "test-app-id".into(),
            app_secret: "test-secret".into(),
            tenant_id: "test-tenant".into(),
        };
        let channel = TeamsChannel::new(config, on_msg);
        assert_eq!(channel.channel_type(), "teams");
        assert_eq!(channel.display_name(), "Microsoft Teams");
        assert_eq!(channel.status(), ChannelStatus::Disconnected);
    }
}
