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

/// Reject a signal-cli base URL that would put traffic on the wire in the clear.
///
/// Closes CodeQL `rust/cleartext-transmission` (alerts 143/144). Every request
/// this channel makes carries the account's phone number in the path, and
/// `send_text` / `send_to_group` carry the message body — so an unencrypted hop
/// to anything but the local machine exposes both to the network.
///
/// `signal_cli_url` is operator config with no default (the
/// `http://localhost:8080` in the docs and in the tests below is an example,
/// not a fallback), so nothing stops it pointing at a remote box over `http`.
/// The rule: `https` anywhere, `http` only to loopback.
///
/// The host is resolved rather than string-matched — `localhost` is not the only
/// spelling of loopback, and a name like `signal.internal` can resolve off-box.
/// Every resolved address must be loopback; one public answer rejects the URL.
fn vet_signal_cli_url(raw: &str) -> Result<()> {
    let url = reqwest::Url::parse(raw)
        .map_err(|e| Error::Config(format!("signal_cli_url is not a valid URL: {e}")))?;

    match url.scheme() {
        "https" => Ok(()),
        "http" => {
            let host = url
                .host_str()
                .ok_or_else(|| Error::Config("signal_cli_url has no host".into()))?;
            let port = url.port_or_known_default().unwrap_or(80);

            // `host_str` keeps the brackets on an IPv6 literal (`[::1]`), which
            // ToSocketAddrs cannot parse — strip them. Domains are unaffected.
            let bare = host
                .strip_prefix('[')
                .and_then(|h| h.strip_suffix(']'))
                .unwrap_or(host);

            // A bare IP literal needs no DNS; ToSocketAddrs handles both forms.
            let addrs: Vec<std::net::SocketAddr> =
                std::net::ToSocketAddrs::to_socket_addrs(&(bare, port))
                    .map_err(|e| {
                        Error::Config(format!("signal_cli_url host {host:?} did not resolve: {e}"))
                    })?
                    .collect();

            if addrs.is_empty() {
                return Err(Error::Config(format!(
                    "signal_cli_url host {host:?} resolved to no addresses"
                )));
            }
            if addrs.iter().all(|a| a.ip().is_loopback()) {
                Ok(())
            } else {
                Err(Error::Config(format!(
                    "signal_cli_url uses http:// with non-loopback host {host:?}; \
                     the phone number and message bodies would cross the network \
                     unencrypted. Use https://, or run signal-cli on localhost."
                )))
            }
        }
        other => Err(Error::Config(format!(
            "signal_cli_url scheme {other:?} is not supported; use https:// \
             (or http:// to loopback)"
        ))),
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

        // Fail closed before any request goes out. `SignalChannel::new` cannot
        // do this (it returns `Self`), and connect is what the channel manager
        // calls before the first send, so this is the earliest fallible gate.
        vet_signal_cli_url(&self.config.signal_cli_url)?;

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
        if let Some(group_id) = message
            .metadata
            .get("signal_group_id")
            .and_then(|v| v.as_str())
        {
            self.send_to_group(group_id, &text).await
        } else if let Some(recipient) = message
            .metadata
            .get("signal_recipient")
            .and_then(|v| v.as_str())
        {
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
            Arc::new(|_from, _name, _text, _delta_tx| Box::pin(async { Ok("test".to_string()) }));
        let config = SignalConfig {
            signal_cli_url: "http://localhost:8080".into(),
            phone_number: "+1234567890".into(),
        };
        let channel = SignalChannel::new(config, on_msg);
        assert_eq!(channel.channel_type(), "signal");
        assert_eq!(channel.display_name(), "Signal");
        assert_eq!(channel.status(), ChannelStatus::Disconnected);
    }

    fn vets(url: &str) -> Result<()> {
        super::vet_signal_cli_url(url)
    }

    #[test]
    fn https_is_always_accepted() {
        assert!(vets("https://signal.example.com:8080").is_ok());
        assert!(vets("https://10.0.0.5").is_ok());
    }

    #[test]
    fn http_to_loopback_is_accepted() {
        assert!(vets("http://localhost:8080").is_ok());
        assert!(vets("http://127.0.0.1:8080").is_ok());
        assert!(vets("http://[::1]:8080").is_ok());
    }

    #[test]
    fn http_to_remote_host_is_rejected() {
        // The whole point of the guard: phone number and message bodies would
        // otherwise cross the network in the clear.
        let err = vets("http://10.0.0.5:8080").expect_err("must reject");
        assert!(
            err.to_string().contains("non-loopback"),
            "unexpected error: {err}"
        );
        assert!(vets("http://93.184.216.34:8080").is_err());
    }

    #[test]
    fn non_http_schemes_are_rejected() {
        assert!(vets("file:///etc/passwd").is_err());
        assert!(vets("ftp://localhost:8080").is_err());
        assert!(vets("not a url").is_err());
    }

    #[tokio::test]
    async fn connect_refuses_cleartext_to_remote_host() {
        let config = SignalConfig {
            signal_cli_url: "http://10.0.0.5:8080".into(),
            phone_number: "+15550001111".into(),
        };
        let on_msg: SignalOnMessageFn =
            Arc::new(|_, _, _, _| Box::pin(async { Ok(String::new()) }));
        let mut channel = SignalChannel::new(config, on_msg);
        let err = channel
            .connect()
            .await
            .expect_err("connect must fail closed");
        assert!(
            err.to_string().contains("non-loopback"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn send_message_missing_recipient() {
        let on_msg: SignalOnMessageFn =
            Arc::new(|_from, _name, _text, _delta_tx| Box::pin(async { Ok("test".to_string()) }));
        let config = SignalConfig {
            signal_cli_url: "http://localhost:8080".into(),
            phone_number: "+1234567890".into(),
        };
        let channel = SignalChannel::new(config, on_msg);
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
        let on_msg: SignalOnMessageFn =
            Arc::new(|_from, _name, _text, _delta_tx| Box::pin(async { Ok("test".to_string()) }));
        let config = SignalConfig {
            signal_cli_url: "http://localhost:8080".into(),
            phone_number: "+0".into(),
        };
        let channel = SignalChannel::new(config, on_msg);
        assert_eq!(channel.status(), ChannelStatus::Disconnected);
    }

    #[test]
    fn display_name_is_signal() {
        let on_msg: SignalOnMessageFn =
            Arc::new(|_from, _name, _text, _delta_tx| Box::pin(async { Ok("test".to_string()) }));
        let config = SignalConfig {
            signal_cli_url: "http://localhost:8080".into(),
            phone_number: "+0".into(),
        };
        let channel = SignalChannel::new(config, on_msg);
        assert_eq!(channel.display_name(), "Signal");
    }
}
