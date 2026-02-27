pub mod commands;
pub mod protocol;
pub mod registry;
#[cfg(feature = "telegram")]
pub mod telegram;
#[cfg(feature = "telegram")]
pub mod telegram_fmt;
pub mod traits;

#[cfg(feature = "discord")]
pub mod discord;
#[cfg(all(target_os = "macos", feature = "imessage"))]
pub mod imessage;
#[cfg(feature = "slack")]
pub mod slack;
#[cfg(feature = "whatsapp")]
pub mod whatsapp;

pub use commands::{
    builtins::register_builtins, CommandContext, CommandError, CommandRegistry, CommandResult,
    Role, SlashCommand,
};
#[cfg(all(target_os = "macos", feature = "imessage"))]
pub use imessage::{IMessageChannel, IMessageOnMessageFn};
pub use protocol::{
    ConnectorCapability, ConnectorFrame, ConnectorHandshake, CONNECTOR_PROTOCOL_VERSION,
    MAX_CONNECTOR_FRAME_BYTES,
};
pub use registry::ChannelRegistry;
#[cfg(feature = "slack")]
pub use slack::{SlackChannel, SlackOnMessageFn};
#[cfg(feature = "telegram")]
pub use telegram::{OnMessageFn, OnVoiceFn, TelegramChannel};
pub use traits::{Channel, ChannelEvent, ChannelStatus};
#[cfg(feature = "whatsapp")]
pub use whatsapp::{WhatsAppChannel, WhatsAppOnMessageFn};
