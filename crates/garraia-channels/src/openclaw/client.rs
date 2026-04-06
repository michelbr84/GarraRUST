use std::sync::Arc;
use std::time::Duration;

use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use tokio::sync::{Mutex, mpsc, watch};
use tokio_tungstenite::tungstenite::Message as WsMessage;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async};
use tracing::{error, info, warn};

use crate::traits::ChannelStatus;
use super::config::OpenClawConfig;
use super::convert;

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// Persistent WebSocket client for the OpenClaw daemon.
pub struct OpenClawClient {
    config: OpenClawConfig,
    status: watch::Sender<ChannelStatus>,
    status_rx: watch::Receiver<ChannelStatus>,
    /// Outgoing messages to send through the WebSocket.
    tx: mpsc::Sender<serde_json::Value>,
    /// Incoming messages received from OpenClaw (reserved for future poll API).
    _incoming_rx: Mutex<mpsc::Receiver<garraia_common::Message>>,
}

impl OpenClawClient {
    /// Create a new client (does not connect yet — call `spawn_loop`).
    pub fn new(config: OpenClawConfig) -> (Arc<Self>, mpsc::Receiver<garraia_common::Message>) {
        let (status_tx, status_rx) = watch::channel(ChannelStatus::Disconnected);
        let (outgoing_tx, outgoing_rx) = mpsc::channel::<serde_json::Value>(256);
        let (incoming_tx, incoming_rx) = mpsc::channel::<garraia_common::Message>(256);

        let client = Arc::new(Self {
            config,
            status: status_tx,
            status_rx,
            tx: outgoing_tx,
            _incoming_rx: Mutex::new(mpsc::channel(1).1), // placeholder, real rx is returned
        });

        // Spawn the connection loop.
        let client_clone = Arc::clone(&client);
        tokio::spawn(async move {
            client_clone
                .connection_loop(outgoing_rx, incoming_tx)
                .await;
        });

        // Return the incoming receiver so the gateway can consume messages.
        let dummy_rx = incoming_rx;
        (client, dummy_rx)
    }

    /// Current connection status.
    pub fn status(&self) -> ChannelStatus {
        self.status_rx.borrow().clone()
    }

    /// Send a reply back through OpenClaw.
    pub async fn send_reply(&self, msg: &garraia_common::Message) -> garraia_common::Result<()> {
        let payload = convert::to_openclaw_message(msg);
        self.tx
            .send(payload)
            .await
            .map_err(|e| garraia_common::Error::Channel(format!("openclaw send failed: {e}")))?;
        Ok(())
    }

    /// List platforms available through OpenClaw (populated after handshake).
    pub fn available_channels(&self) -> &[String] {
        &self.config.channels
    }

    /// Main connection loop with automatic reconnection.
    async fn connection_loop(
        &self,
        mut outgoing_rx: mpsc::Receiver<serde_json::Value>,
        incoming_tx: mpsc::Sender<garraia_common::Message>,
    ) {
        let mut attempt = 0u32;
        let max_backoff = Duration::from_secs(60);

        loop {
            let _ = self.status.send(ChannelStatus::Connecting);
            info!(url = %self.config.ws_url, "OpenClaw: connecting...");

            match connect_async(&self.config.ws_url).await {
                Ok((ws_stream, _)) => {
                    let _ = self.status.send(ChannelStatus::Connected);
                    info!("OpenClaw: connected");
                    attempt = 0;

                    let (write, read) = ws_stream.split();
                    let write = Arc::new(Mutex::new(write));

                    // Run read + write tasks until one fails.
                    tokio::select! {
                        _ = Self::read_loop(read, &incoming_tx) => {
                            warn!("OpenClaw: read loop ended");
                        }
                        _ = Self::write_loop(Arc::clone(&write), &mut outgoing_rx) => {
                            warn!("OpenClaw: write loop ended");
                        }
                    }

                    let _ = self.status.send(ChannelStatus::Reconnecting);
                }
                Err(e) => {
                    error!(error = %e, "OpenClaw: connection failed");
                    let _ = self.status.send(ChannelStatus::Error(e.to_string()));
                }
            }

            // Exponential backoff with cap.
            attempt = attempt.saturating_add(1);
            let base = Duration::from_secs(self.config.reconnect_interval_secs);
            let delay = std::cmp::min(base * 2u32.saturating_pow(attempt - 1), max_backoff);
            info!(delay_secs = delay.as_secs(), "OpenClaw: reconnecting in...");
            tokio::time::sleep(delay).await;
        }
    }

    async fn read_loop(
        mut read: SplitStream<WsStream>,
        incoming_tx: &mpsc::Sender<garraia_common::Message>,
    ) {
        while let Some(frame) = read.next().await {
            match frame {
                Ok(WsMessage::Text(text)) => {
                    match serde_json::from_str::<serde_json::Value>(&text) {
                        Ok(value) => {
                            if let Some(msg) = convert::from_openclaw_message(&value)
                                && incoming_tx.send(msg).await.is_err() {
                                    break;
                                }
                        }
                        Err(e) => {
                            warn!(error = %e, "OpenClaw: invalid JSON frame");
                        }
                    }
                }
                Ok(WsMessage::Close(_)) => {
                    info!("OpenClaw: server sent close frame");
                    break;
                }
                Ok(WsMessage::Ping(data)) => {
                    // Pong is handled automatically by tungstenite.
                    let _ = data;
                }
                Ok(_) => {} // Binary, Pong — ignore.
                Err(e) => {
                    error!(error = %e, "OpenClaw: read error");
                    break;
                }
            }
        }
    }

    async fn write_loop(
        write: Arc<Mutex<SplitSink<WsStream, WsMessage>>>,
        outgoing_rx: &mut mpsc::Receiver<serde_json::Value>,
    ) {
        while let Some(payload) = outgoing_rx.recv().await {
            let text = match serde_json::to_string(&payload) {
                Ok(t) => t,
                Err(e) => {
                    error!(error = %e, "OpenClaw: failed to serialize outgoing message");
                    continue;
                }
            };
            let mut sink = write.lock().await;
            if let Err(e) = sink.send(WsMessage::Text(text.into())).await {
                error!(error = %e, "OpenClaw: write error");
                break;
            }
        }
    }
}
