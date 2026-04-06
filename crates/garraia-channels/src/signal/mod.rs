//! Signal channel implementation for GarraIA.
//!
//! Provides a `SignalChannel` struct that implements the `Channel` trait,
//! communicating via the signal-cli REST API daemon.

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

pub use config::SignalConfig;

/// Callback invoked when a Signal message is received.
///
/// Arguments: `(source_number, source_name, text, delta_tx)`.
/// Return `Err("__blocked__")` to silently drop unauthorized messages.
pub type SignalOnMessageFn = Arc<
    dyn Fn(
            String,
            String,
            String,
            Option<mpsc::Sender<String>>,
        ) -> Pin<Box<dyn Future<Output = std::result::Result<String, String>> + Send>>
        + Send
        + Sync,
>;

/// Signal channel implementation via signal-cli REST API.
///
/// Requires a running signal-cli REST API daemon (e.g., via Docker).
/// See: <https://github.com/bbernhard/signal-cli-rest-api>
pub struct SignalChannel {
    config: SignalConfig,
    client: Client,
    status: ChannelStatus,
    on_message: SignalOnMessageFn,
    shutdown_tx: Option<watch::Sender<bool>>,
}

impl SignalChannel {
    /// Create a new `SignalChannel` from config and callback.
    pub fn new(config: SignalConfig, on_message: SignalOnMessageFn) -> Self {
        Self {
            config,
            client: Client::new(),
            status: ChannelStatus::Disconnected,
            on_message,
            shutdown_tx: None,
        }
    }

    /// Access the current config.
    pub fn config(&self) -> &SignalConfig {
        &self.config
    }

    /// Send a text message to a recipient via signal-cli REST API.
    pub async fn send_text(&self, recipient: &str, text: &str) -> Result<()> {
        let url = format!(
            "{}/v2/send",
            self.config.signal_cli_url.trim_end_matches('/')
        );

        let body = serde_json::json!({
            "message": text,
            "number": self.config.phone_number,
            "recipients": [recipient],
        });

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Channel(format!("signal send failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Channel(format!(
                "signal send error {status}: {body}"
            )));
        }

        Ok(())
    }

    /// Send a message to a Signal group.
    pub async fn send_to_group(&self, group_id: &str, text: &str) -> Result<()> {
        let url = format!(
            "{}/v2/send",
            self.config.signal_cli_url.trim_end_matches('/')
        );

        let body = serde_json::json!({
            "message": text,
            "number": self.config.phone_number,
            "recipients": [],
            "group_id": group_id,
        });

        let resp = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Channel(format!("signal group send failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Channel(format!(
                "signal group send error {status}: {body}"
            )));
        }

        Ok(())
    }

    /// Poll for new messages from the signal-cli REST API.
    #[allow(dead_code)]
    async fn receive_messages(&self) -> Result<Vec<serde_json::Value>> {
        let url = format!(
            "{}/v1/receive/{}",
            self.config.signal_cli_url.trim_end_matches('/'),
            self.config.phone_number
        );

        let resp = self
            .client
            .get(&url)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| Error::Channel(format!("signal receive failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Channel(format!(
                "signal receive error {status}: {body}"
            )));
        }

        let messages: Vec<serde_json::Value> = resp
            .json()
            .await
            .map_err(|e| Error::Channel(format!("signal receive parse failed: {e}")))?;

        Ok(messages)
    }
}

#[async_trait]
impl Channel for SignalChannel {
    fn channel_type(&self) -> &str {
        "signal"
    }

    fn display_name(&self) -> &str {
        "Signal"
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

        // Spawn a polling loop for incoming messages
        tokio::spawn(async move {
            loop {
                if *shutdown_rx.borrow() {
                    info!("signal: shutdown requested");
                    return;
                }

                let url = format!(
                    "{}/v1/receive/{}",
                    config.signal_cli_url.trim_end_matches('/'),
                    config.phone_number
                );

                let resp = tokio::select! {
                    r = client.get(&url).timeout(Duration::from_secs(30)).send() => {
                        match r {
                            Ok(r) => r,
                            Err(e) => {
                                warn!("signal: receive failed: {e}");
                                tokio::select! {
                                    _ = tokio::time::sleep(Duration::from_secs(5)) => continue,
                                    _ = shutdown_rx.changed() => return,
                                }
                            }
                        }
                    }
                    _ = shutdown_rx.changed() => return,
                };

                let messages: Vec<serde_json::Value> = match resp.json().await {
                    Ok(m) => m,
                    Err(e) => {
                        warn!("signal: parse failed: {e}");
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_secs(5)) => continue,
                            _ = shutdown_rx.changed() => return,
                        }
                    }
                };

                for msg in messages {
                    let envelope = match msg.get("envelope") {
                        Some(e) => e,
                        None => continue,
                    };

                    let source = envelope
                        .get("sourceNumber")
                        .or_else(|| envelope.get("source"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    let source_name = envelope
                        .get("sourceName")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&source)
                        .to_string();

                    let data_message = match envelope.get("dataMessage") {
                        Some(d) => d,
                        None => continue,
                    };

                    let text = data_message
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    if text.trim().is_empty() || source.is_empty() {
                        continue;
                    }

                    info!("signal: message from {}: {} chars", source, text.len());

                    let cb = Arc::clone(&on_message);
                    let reply_client = client.clone();
                    let reply_config = config.clone();
                    let reply_to = source.clone();

                    tokio::spawn(async move {
                        match cb(reply_to.clone(), source_name, text, None).await {
                            Ok(reply) => {
                                let url = format!(
                                    "{}/v2/send",
                                    reply_config.signal_cli_url.trim_end_matches('/')
                                );
                                let body = serde_json::json!({
                                    "message": reply,
                                    "number": reply_config.phone_number,
                                    "recipients": [reply_to],
                                });
                                if let Err(e) = reply_client.post(&url).json(&body).send().await {
                                    error!("signal: failed to send reply: {e}");
                                }
                            }
                            Err(e) if e == "__blocked__" => {}
                            Err(e) => {
                                error!("signal: callback error: {e}");
                            }
                        }
                    });
                }

                // Poll interval
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(2)) => {},
                    _ = shutdown_rx.changed() => return,
                }
            }
        });

        self.status = ChannelStatus::Connected;
        info!("signal channel connected (polling mode)");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(true);
        }
        self.status = ChannelStatus::Disconnected;
        info!("signal channel disconnected");
        Ok(())
    }

    async fn send_message(&self, message: &Message) -> Result<()> {
        let text = match &message.content {
            MessageContent::Text(t) => t.clone(),
            _ => {
                return Err(Error::Channel(
                    "only text messages are supported for signal send".into(),
                ));
            }
        };

        // Try group first, then individual recipient
        if let Some(group_id) = message.metadata.get("signal_group_id").and_then(|v| v.as_str()) {
            self.send_to_group(group_id, &text).await
        } else if let Some(recipient) = message.metadata.get("signal_recipient").and_then(|v| v.as_str()) {
            self.send_text(recipient, &text).await
        } else {
            Err(Error::Channel(
                "missing signal_recipient or signal_group_id in metadata".into(),
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
    fn channel_type_is_signal() {
        let on_msg: SignalOnMessageFn =
            Arc::new(|_from, _name, _text, _delta_tx| {
                Box::pin(async { Ok("test".to_string()) })
            });
        let config = SignalConfig {
            signal_cli_url: "http://localhost:8080".into(),
            phone_number: "+1234567890".into(),
        };
        let channel = SignalChannel::new(config, on_msg);
        assert_eq!(channel.channel_type(), "signal");
        assert_eq!(channel.display_name(), "Signal");
        assert_eq!(channel.status(), ChannelStatus::Disconnected);
    }
}
