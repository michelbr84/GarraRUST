pub mod commands;
pub mod metrics;
pub mod protocol;
pub mod registry;
#[cfg(feature = "telegram")]
pub mod telegram;
#[cfg(feature = "telegram")]
pub mod telegram_fmt;
pub mod traits;

#[cfg(feature = "discord")]
pub mod discord;
#[cfg(feature = "google_chat")]
pub mod google_chat;
#[cfg(all(target_os = "macos", feature = "imessage"))]
pub mod imessage;
#[cfg(feature = "irc")]
pub mod irc;
#[cfg(feature = "line")]
pub mod line_channel;
#[cfg(feature = "matrix")]
pub mod matrix;
#[cfg(feature = "openclaw")]
pub mod openclaw;
#[cfg(feature = "signal")]
pub mod signal;
#[cfg(feature = "slack")]
pub mod slack;
#[cfg(feature = "teams")]
pub mod teams;
#[cfg(feature = "voice")]
pub mod voice_channel;
#[cfg(feature = "whatsapp")]
pub mod whatsapp;

pub use commands::{
    CommandContext, CommandError, CommandRegistry, CommandResult, Role, SlashCommand,
    builtins::register_builtins,
};
#[cfg(all(target_os = "macos", feature = "imessage"))]
pub use imessage::{IMessageChannel, IMessageOnMessageFn};
pub use metrics::{AtomicChannelMetrics, ChannelMetrics, MetricsRegistry};
pub use protocol::{
    CONNECTOR_PROTOCOL_VERSION, ConnectorCapability, ConnectorFrame, ConnectorHandshake,
    MAX_CONNECTOR_FRAME_BYTES,
};
pub use registry::ChannelRegistry;
pub use traits::{Channel, ChannelEvent, ChannelStatus};

// ─── Channel re-exports ─────────────────────────────────────────────

#[cfg(feature = "discord")]
pub use discord::{DiscordChannel, DiscordOnMessageFn};
#[cfg(feature = "google_chat")]
pub use google_chat::{GoogleChatChannel, GoogleChatConfig, GoogleChatOnMessageFn};
#[cfg(feature = "irc")]
pub use irc::{IrcChannel, IrcConfig, IrcOnMessageFn};
#[cfg(feature = "line")]
pub use line_channel::{LineChannel, LineConfig, LineOnMessageFn};
#[cfg(feature = "matrix")]
pub use matrix::{MatrixChannel, MatrixConfig, MatrixOnMessageFn};
#[cfg(feature = "openclaw")]
pub use openclaw::{OpenClawClient, OpenClawConfig};
#[cfg(feature = "signal")]
pub use signal::{SignalChannel, SignalConfig, SignalOnMessageFn};
#[cfg(feature = "slack")]
pub use slack::{SlackChannel, SlackOnMessageFn};
#[cfg(feature = "teams")]
pub use teams::{TeamsChannel, TeamsConfig, TeamsOnMessageFn};
#[cfg(feature = "telegram")]
pub use telegram::{OnMessageFn, OnVoiceFn, TelegramChannel};
#[cfg(feature = "voice")]
pub use voice_channel::{
    AudioFormat, SttProvider, TtsProvider, VoiceChannel, VoiceConfig, VoiceOnMessageFn,
};
#[cfg(feature = "whatsapp")]
pub use whatsapp::{WhatsAppChannel, WhatsAppOnMessageFn};

// ─── Fallback routing ───────────────────────────────────────────────

use garraia_common::{Error, Message, Result};
use tracing::warn;

/// Send a message through a channel with fallback to OpenClaw bridge.
///
/// If the primary channel fails and the `openclaw` feature is enabled,
/// attempts to route through the OpenClaw bridge as a fallback.
pub async fn send_with_fallback(
    registry: &ChannelRegistry,
    channel_type: &str,
    message: &Message,
    #[cfg(feature = "openclaw")] openclaw_client: Option<&OpenClawClient>,
) -> Result<()> {
    // Try the native channel first
    if let Some(channel) = registry.get(channel_type) {
        match channel.send_message(message).await {
            Ok(()) => return Ok(()),
            Err(e) => {
                warn!(
                    "native channel '{}' failed: {}, attempting fallback",
                    channel_type, e
                );

                // Try OpenClaw bridge if available
                #[cfg(feature = "openclaw")]
                if let Some(_client) = openclaw_client {
                    // Convert and forward through OpenClaw bridge
                    let openclaw_msg = openclaw::to_openclaw_message(message);
                    warn!(
                        "openclaw bridge fallback for '{}': forwarding {} bytes",
                        channel_type,
                        serde_json::to_string(&openclaw_msg)
                            .unwrap_or_default()
                            .len()
                    );
                    // TODO: implement actual OpenClaw send when client supports it
                    return Err(Error::Channel(format!(
                        "channel '{}' failed and openclaw bridge not yet implemented: {}",
                        channel_type, e
                    )));
                }

                return Err(e);
            }
        }
    }

    Err(Error::Channel(format!(
        "channel '{}' not registered",
        channel_type
    )))
}

/// Send a message through a channel with fallback (no-openclaw version).
///
/// Used when the `openclaw` feature is disabled.
#[cfg(not(feature = "openclaw"))]
pub async fn send_with_fallback_simple(
    registry: &ChannelRegistry,
    channel_type: &str,
    message: &Message,
) -> Result<()> {
    if let Some(channel) = registry.get(channel_type) {
        channel.send_message(message).await
    } else {
        Err(Error::Channel(format!(
            "channel '{}' not registered",
            channel_type
        )))
    }
}
