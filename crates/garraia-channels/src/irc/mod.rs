//! IRC channel implementation for GarraIA.
//!
//! Provides an `IrcChannel` struct that implements the `Channel` trait,
//! using raw TCP/TLS connections with tokio for IRC protocol handling.

pub mod config;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, watch, Mutex};
use tracing::{error, info};

use crate::traits::{Channel, ChannelStatus};
use garraia_common::{Error, Message, MessageContent, Result};

pub use config::IrcConfig;

/// Callback invoked when a PRIVMSG is received from IRC.
///
/// Arguments: `(channel_name, nick, user_name, text, delta_tx)`.
/// Return `Err("__blocked__")` to silently drop unauthorized messages.
pub type IrcOnMessageFn = Arc<
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

/// Type alias for the shared writer half of the TCP connection.
type SharedWriter = Arc<Mutex<Option<tokio::io::WriteHalf<TcpStream>>>>;

/// IRC channel implementation.
///
/// Uses tokio TCP for persistent connection, handles IRC protocol commands
/// (NICK, USER, JOIN, PRIVMSG, PING/PONG).
pub struct IrcChannel {
    config: IrcConfig,
    status: ChannelStatus,
    on_message: IrcOnMessageFn,
    shutdown_tx: Option<watch::Sender<bool>>,
    writer: SharedWriter,
}

impl IrcChannel {
    /// Create a new `IrcChannel` from config and callback.
    pub fn new(config: IrcConfig, on_message: IrcOnMessageFn) -> Self {
        Self {
            config,
            status: ChannelStatus::Disconnected,
            on_message,
            shutdown_tx: None,
            writer: Arc::new(Mutex::new(None)),
        }
    }

    /// Access the current config.
    pub fn config(&self) -> &IrcConfig {
        &self.config
    }

    /// Send a raw IRC line (appends \r\n).
    async fn send_raw(writer: &SharedWriter, line: &str) -> Result<()> {
        let mut guard = writer.lock().await;
        let w = guard.as_mut().ok_or_else(|| {
            Error::Channel("irc: not connected".into())
        })?;
        let data = format!("{}\r\n", line);
        w.write_all(data.as_bytes())
            .await
            .map_err(|e| Error::Channel(format!("irc write failed: {e}")))?;
        w.flush()
            .await
            .map_err(|e| Error::Channel(format!("irc flush failed: {e}")))?;
        Ok(())
    }

    /// Send a PRIVMSG to a target (channel or user).
    pub async fn send_privmsg(&self, target: &str, text: &str) -> Result<()> {
        // IRC messages have a max length of ~512 bytes. Split if needed.
        let max_len = 400; // Conservative limit after protocol overhead
        for chunk in text.as_bytes().chunks(max_len) {
            let chunk_str = String::from_utf8_lossy(chunk);
            let line = format!("PRIVMSG {} :{}", target, chunk_str);
            Self::send_raw(&self.writer, &line).await?;
        }
        Ok(())
    }
}

#[async_trait]
impl Channel for IrcChannel {
    fn channel_type(&self) -> &str {
        "irc"
    }

    fn display_name(&self) -> &str {
        "IRC"
    }

    async fn connect(&mut self) -> Result<()> {
        if matches!(self.status, ChannelStatus::Connected) {
            return Ok(());
        }

        self.status = ChannelStatus::Connecting;

        let addr = format!("{}:{}", self.config.server, self.config.port);
        info!("irc: connecting to {}", addr);

        // Note: TLS support would require tokio-native-tls or tokio-rustls.
        // For now, plain TCP only. Set use_tls=false.
        if self.config.use_tls {
            return Err(Error::Channel(
                "irc: TLS not yet implemented, set use_tls=false".into(),
            ));
        }

        let stream = TcpStream::connect(&addr)
            .await
            .map_err(|e| Error::Channel(format!("irc: connect failed: {e}")))?;

        let (reader, writer) = tokio::io::split(stream);

        // Store the writer for sending messages
        {
            let mut guard = self.writer.lock().await;
            *guard = Some(writer);
        }

        let writer_ref = Arc::clone(&self.writer);

        // Send NICK and USER
        let nick = self.config.nick.clone();
        Self::send_raw(&writer_ref, &format!("NICK {}", nick)).await?;
        Self::send_raw(
            &writer_ref,
            &format!("USER {} 0 * :GarraIA Bot", nick),
        )
        .await?;

        // Join channels after registration
        let channels = self.config.channels.clone();
        let join_writer = Arc::clone(&writer_ref);

        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        self.shutdown_tx = Some(shutdown_tx);

        let on_message = Arc::clone(&self.on_message);
        let ping_writer = Arc::clone(&writer_ref);

        // Spawn the read loop
        tokio::spawn(async move {
            let mut buf_reader = BufReader::new(reader);
            let mut line = String::new();
            let mut registered = false;

            loop {
                line.clear();

                tokio::select! {
                    result = buf_reader.read_line(&mut line) => {
                        match result {
                            Ok(0) => {
                                info!("irc: connection closed by server");
                                return;
                            }
                            Ok(_) => {
                                let trimmed = line.trim_end();

                                // Handle PING/PONG
                                if trimmed.starts_with("PING ") {
                                    let pong = trimmed.replacen("PING", "PONG", 1);
                                    if let Err(e) = IrcChannel::send_raw(&ping_writer, &pong).await {
                                        error!("irc: failed to send PONG: {e}");
                                        return;
                                    }
                                    continue;
                                }

                                // Detect end of MOTD (RPL_ENDOFMOTD or ERR_NOMOTD)
                                if !registered && (trimmed.contains(" 376 ") || trimmed.contains(" 422 ")) {
                                    registered = true;
                                    // Join configured channels
                                    for ch in &channels {
                                        if let Err(e) = IrcChannel::send_raw(
                                            &join_writer,
                                            &format!("JOIN {}", ch),
                                        ).await {
                                            error!("irc: failed to join {}: {e}", ch);
                                        } else {
                                            info!("irc: joined {}", ch);
                                        }
                                    }
                                }

                                // Handle PRIVMSG
                                // Format: :nick!user@host PRIVMSG #channel :message text
                                if let Some(privmsg_data) = parse_privmsg(trimmed) {
                                    let (source_nick, target, text) = privmsg_data;
                                    if text.trim().is_empty() {
                                        continue;
                                    }

                                    info!("irc: PRIVMSG from {} in {}: {} chars", source_nick, target, text.len());

                                    let cb = Arc::clone(&on_message);
                                    let reply_writer = Arc::clone(&ping_writer);
                                    let reply_target = if target.starts_with('#') {
                                        target.clone()
                                    } else {
                                        source_nick.clone()
                                    };

                                    tokio::spawn(async move {
                                        match cb(target, source_nick.clone(), source_nick, text, None).await {
                                            Ok(reply) => {
                                                // Split reply into lines for IRC
                                                for reply_line in reply.lines() {
                                                    if reply_line.trim().is_empty() {
                                                        continue;
                                                    }
                                                    let msg = format!("PRIVMSG {} :{}", reply_target, reply_line);
                                                    if let Err(e) = IrcChannel::send_raw(&reply_writer, &msg).await {
                                                        error!("irc: failed to send reply: {e}");
                                                        break;
                                                    }
                                                }
                                            }
                                            Err(e) if e == "__blocked__" => {}
                                            Err(e) => {
                                                error!("irc: callback error: {e}");
                                            }
                                        }
                                    });
                                }
                            }
                            Err(e) => {
                                error!("irc: read error: {e}");
                                return;
                            }
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        if *shutdown_rx.borrow() {
                            info!("irc: shutdown requested");
                            let _ = IrcChannel::send_raw(&ping_writer, "QUIT :GarraIA shutting down").await;
                            return;
                        }
                    }
                }
            }
        });

        self.status = ChannelStatus::Connected;
        info!("irc channel connected");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(true);
        }

        // Clear the writer
        {
            let mut guard = self.writer.lock().await;
            *guard = None;
        }

        self.status = ChannelStatus::Disconnected;
        info!("irc channel disconnected");
        Ok(())
    }

    async fn send_message(&self, message: &Message) -> Result<()> {
        let target = message
            .metadata
            .get("irc_target")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                Error::Channel("missing irc_target in metadata".into())
            })?;

        let text = match &message.content {
            MessageContent::Text(t) => t.clone(),
            _ => {
                return Err(Error::Channel(
                    "only text messages are supported for irc send".into(),
                ));
            }
        };

        self.send_privmsg(target, &text).await
    }

    fn status(&self) -> ChannelStatus {
        self.status.clone()
    }
}

/// Parse a PRIVMSG line into (nick, target, text).
///
/// Format: `:nick!user@host PRIVMSG #channel :message text`
fn parse_privmsg(line: &str) -> Option<(String, String, String)> {
    if !line.starts_with(':') {
        return None;
    }

    let parts: Vec<&str> = line.splitn(4, ' ').collect();
    if parts.len() < 4 {
        return None;
    }

    // parts[0] = ":nick!user@host"
    // parts[1] = "PRIVMSG"
    // parts[2] = "#channel" or "nick"
    // parts[3] = ":message text"

    if parts[1] != "PRIVMSG" {
        return None;
    }

    let nick = parts[0]
        .trim_start_matches(':')
        .split('!')
        .next()?
        .to_string();

    let target = parts[2].to_string();
    let text = parts[3].trim_start_matches(':').to_string();

    Some((nick, target, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_type_is_irc() {
        let on_msg: IrcOnMessageFn =
            Arc::new(|_ch, _nick, _user, _text, _delta_tx| {
                Box::pin(async { Ok("test".to_string()) })
            });
        let config = IrcConfig {
            server: "irc.libera.chat".into(),
            port: 6667,
            nick: "garrabot".into(),
            channels: vec!["#garraia".into()],
            use_tls: false,
        };
        let channel = IrcChannel::new(config, on_msg);
        assert_eq!(channel.channel_type(), "irc");
        assert_eq!(channel.display_name(), "IRC");
        assert_eq!(channel.status(), ChannelStatus::Disconnected);
    }

    #[test]
    fn parse_privmsg_valid() {
        let line = ":user!name@host PRIVMSG #channel :hello world";
        let result = parse_privmsg(line);
        assert!(result.is_some());
        let (nick, target, text) = result.expect("should parse");
        assert_eq!(nick, "user");
        assert_eq!(target, "#channel");
        assert_eq!(text, "hello world");
    }

    #[test]
    fn parse_privmsg_private() {
        let line = ":user!name@host PRIVMSG botname :hello";
        let result = parse_privmsg(line);
        assert!(result.is_some());
        let (nick, target, text) = result.expect("should parse");
        assert_eq!(nick, "user");
        assert_eq!(target, "botname");
        assert_eq!(text, "hello");
    }

    #[test]
    fn parse_privmsg_not_privmsg() {
        let line = ":server NOTICE * :Welcome";
        assert!(parse_privmsg(line).is_none());
    }

    #[test]
    fn parse_privmsg_invalid() {
        let line = "not a valid irc line";
        assert!(parse_privmsg(line).is_none());
    }
}
