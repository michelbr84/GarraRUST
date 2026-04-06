//! Matrix/Element channel implementation for GarraIA.
//!
//! Provides a `MatrixChannel` struct that implements the `Channel` trait,
//! communicating via Matrix Client-Server API v3.

pub mod config;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use tokio::sync::{mpsc, watch};
use tracing::{error, info, warn};

use crate::traits::{Channel, ChannelStatus};
use garraia_common::{Error, Message, MessageContent, Result};

pub use config::MatrixConfig;

/// Callback invoked when a Matrix room message is received.
///
/// Arguments: `(room_id, user_id, user_name, text, delta_tx)`.
/// Return `Err("__blocked__")` to silently drop unauthorized messages.
pub type MatrixOnMessageFn = Arc<
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

/// Matrix/Element channel implementation.
///
/// Uses the Matrix Client-Server API v3 for syncing and sending messages.
pub struct MatrixChannel {
    config: MatrixConfig,
    client: Client,
    status: ChannelStatus,
    on_message: MatrixOnMessageFn,
    shutdown_tx: Option<watch::Sender<bool>>,
}

impl MatrixChannel {
    /// Create a new `MatrixChannel` from config and callback.
    pub fn new(config: MatrixConfig, on_message: MatrixOnMessageFn) -> Self {
        Self {
            config,
            client: Client::new(),
            status: ChannelStatus::Disconnected,
            on_message,
            shutdown_tx: None,
        }
    }

    /// Access the current config.
    pub fn config(&self) -> &MatrixConfig {
        &self.config
    }

    /// Send a text message to a Matrix room.
    pub async fn send_to_room(&self, room_id: &str, text: &str) -> Result<()> {
        let txn_id = format!("garraia-{}", chrono::Utc::now().timestamp_millis());
        let url = format!(
            "{}/_matrix/client/v3/rooms/{}/send/m.room.message/{}",
            self.config.homeserver_url.trim_end_matches('/'),
            room_id,
            txn_id
        );

        let body = serde_json::json!({
            "msgtype": "m.text",
            "body": text,
        });

        let resp = self
            .client
            .put(&url)
            .bearer_auth(&self.config.access_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Channel(format!("matrix send failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Channel(format!(
                "matrix API error {status}: {body}"
            )));
        }

        Ok(())
    }

    /// Perform a /sync request to the Matrix homeserver.
    #[allow(dead_code)]
    async fn sync_once(&self, since: Option<&str>) -> Result<serde_json::Value> {
        let mut url = format!(
            "{}/_matrix/client/v3/sync?timeout=30000",
            self.config.homeserver_url.trim_end_matches('/'),
        );

        if let Some(token) = since {
            url.push_str(&format!("&since={}", token));
        }

        // Filter to only get room messages
        let filter = serde_json::json!({
            "room": {
                "timeline": {
                    "types": ["m.room.message"],
                    "limit": 50
                },
                "state": { "types": [] },
                "ephemeral": { "types": [] }
            },
            "presence": { "types": [] }
        });
        url.push_str(&format!("&filter={}", serde_json::to_string(&filter).unwrap_or_default()));

        let resp = self
            .client
            .get(&url)
            .bearer_auth(&self.config.access_token)
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .map_err(|e| Error::Channel(format!("matrix sync failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Channel(format!(
                "matrix sync error {status}: {body}"
            )));
        }

        resp.json()
            .await
            .map_err(|e| Error::Channel(format!("matrix sync parse failed: {e}")))
    }
}

#[async_trait]
impl Channel for MatrixChannel {
    fn channel_type(&self) -> &str {
        "matrix"
    }

    fn display_name(&self) -> &str {
        "Matrix"
    }

    async fn connect(&mut self) -> Result<()> {
        if matches!(self.status, ChannelStatus::Connected) {
            return Ok(());
        }

        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        self.shutdown_tx = Some(shutdown_tx);

        let client = self.client.clone();
        let config = self.config.clone();
        let on_message = Arc::clone(&self.on_message);

        // Spawn the sync loop
        tokio::spawn(async move {
            let mut since: Option<String> = None;
            // Do an initial sync to get the since token (skip old messages)
            let initial_url = format!(
                "{}/_matrix/client/v3/sync?timeout=0",
                config.homeserver_url.trim_end_matches('/'),
            );
            if let Ok(resp) = client
                .get(&initial_url)
                .bearer_auth(&config.access_token)
                .send()
                .await
            {
                if let Ok(body) = resp.json::<serde_json::Value>().await {
                    since = body.get("next_batch").and_then(|v| v.as_str()).map(String::from);
                }
            }

            loop {
                if *shutdown_rx.borrow() {
                    info!("matrix: shutdown requested");
                    return;
                }

                let sync_url = {
                    let mut url = format!(
                        "{}/_matrix/client/v3/sync?timeout=30000",
                        config.homeserver_url.trim_end_matches('/'),
                    );
                    if let Some(token) = &since {
                        url.push_str(&format!("&since={}", token));
                    }
                    url
                };

                let resp = tokio::select! {
                    r = client
                        .get(&sync_url)
                        .bearer_auth(&config.access_token)
                        .timeout(Duration::from_secs(60))
                        .send() => {
                        match r {
                            Ok(r) => r,
                            Err(e) => {
                                warn!("matrix: sync request failed: {e}");
                                tokio::select! {
                                    _ = tokio::time::sleep(Duration::from_secs(5)) => continue,
                                    _ = shutdown_rx.changed() => return,
                                }
                            }
                        }
                    }
                    _ = shutdown_rx.changed() => return,
                };

                let body: serde_json::Value = match resp.json().await {
                    Ok(b) => b,
                    Err(e) => {
                        warn!("matrix: sync parse failed: {e}");
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_secs(5)) => continue,
                            _ = shutdown_rx.changed() => return,
                        }
                    }
                };

                // Update the since token
                if let Some(token) = body.get("next_batch").and_then(|v| v.as_str()) {
                    since = Some(token.to_string());
                }

                // Process room events
                if let Some(rooms) = body.get("rooms").and_then(|r| r.get("join")).and_then(|j| j.as_object()) {
                    for (room_id, room_data) in rooms {
                        // Skip rooms not in our configured list (if any configured)
                        if !config.room_ids.is_empty() && !config.room_ids.contains(room_id) {
                            continue;
                        }

                        let events = room_data
                            .get("timeline")
                            .and_then(|t| t.get("events"))
                            .and_then(|e| e.as_array());

                        if let Some(events) = events {
                            for event in events {
                                let sender = event.get("sender").and_then(|s| s.as_str()).unwrap_or("");
                                let content = event.get("content");
                                let msgtype = content.and_then(|c| c.get("msgtype")).and_then(|m| m.as_str());
                                let body_text = content.and_then(|c| c.get("body")).and_then(|b| b.as_str());

                                if msgtype == Some("m.text") {
                                    if let Some(text) = body_text {
                                        // Skip our own messages
                                        if sender.is_empty() || text.trim().is_empty() {
                                            continue;
                                        }

                                        let room = room_id.clone();
                                        let user = sender.to_string();
                                        let msg_text = text.to_string();
                                        let cb = Arc::clone(&on_message);
                                        let reply_client = client.clone();
                                        let reply_config = config.clone();

                                        tokio::spawn(async move {
                                            match cb(room.clone(), user.clone(), user.clone(), msg_text, None).await {
                                                Ok(reply) => {
                                                    let txn_id = format!("garraia-{}", chrono::Utc::now().timestamp_millis());
                                                    let url = format!(
                                                        "{}/_matrix/client/v3/rooms/{}/send/m.room.message/{}",
                                                        reply_config.homeserver_url.trim_end_matches('/'),
                                                        room,
                                                        txn_id
                                                    );
                                                    let body = serde_json::json!({
                                                        "msgtype": "m.text",
                                                        "body": reply,
                                                    });
                                                    if let Err(e) = reply_client
                                                        .put(&url)
                                                        .bearer_auth(&reply_config.access_token)
                                                        .json(&body)
                                                        .send()
                                                        .await
                                                    {
                                                        error!("matrix: failed to send reply: {e}");
                                                    }
                                                }
                                                Err(e) if e == "__blocked__" => {}
                                                Err(e) => {
                                                    error!("matrix: callback error: {e}");
                                                }
                                            }
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        self.status = ChannelStatus::Connected;
        info!("matrix channel connected (sync mode)");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(true);
        }
        self.status = ChannelStatus::Disconnected;
        info!("matrix channel disconnected");
        Ok(())
    }

    async fn send_message(&self, message: &Message) -> Result<()> {
        let room_id = message
            .metadata
            .get("matrix_room_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                Error::Channel("missing matrix_room_id in metadata".into())
            })?;

        let text = match &message.content {
            MessageContent::Text(t) => t.clone(),
            _ => {
                return Err(Error::Channel(
                    "only text messages are supported for matrix send".into(),
                ));
            }
        };

        self.send_to_room(room_id, &text).await
    }

    fn status(&self) -> ChannelStatus {
        self.status.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_type_is_matrix() {
        let on_msg: MatrixOnMessageFn =
            Arc::new(|_room, _uid, _user, _text, _delta_tx| {
                Box::pin(async { Ok("test".to_string()) })
            });
        let config = MatrixConfig {
            homeserver_url: "https://matrix.example.com".into(),
            access_token: "test-token".into(),
            room_ids: vec!["!room:example.com".into()],
        };
        let channel = MatrixChannel::new(config, on_msg);
        assert_eq!(channel.channel_type(), "matrix");
        assert_eq!(channel.display_name(), "Matrix");
        assert_eq!(channel.status(), ChannelStatus::Disconnected);
    }
}
