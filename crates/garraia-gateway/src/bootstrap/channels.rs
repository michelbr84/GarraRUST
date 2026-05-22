use garraia_config::AppConfig;
use tracing::{info, warn};

/// Build configured channels that can be initialized before state is wrapped in Arc.
pub async fn build_channels(config: &AppConfig) -> garraia_channels::ChannelRegistry {
    // Load .env file if present (idempotent, will not overwrite existing env vars)
    if let Err(e) = dotenvy::dotenv() {
        tracing::debug!("no .env file loaded: {e}");
    }

    let registry = garraia_channels::ChannelRegistry::new();

    for (name, channel_config) in &config.channels {
        let enabled = channel_config.enabled.unwrap_or(true);
        if !enabled {
            info!("channel {name} is disabled, skipping");
            continue;
        }

        match channel_config.channel_type.as_str() {
            "discord" => {
                // Discord channels need SharedState for callbacks, so they are started later.
                info!("discord channel {name} will be started after state initialization");
            }
            "telegram" => {
                // Telegram channels need SharedState for callbacks, so they are started later.
                info!("telegram channel {name} will be started after state initialization");
            }
            "slack" => {
                // Slack channels need SharedState for callbacks, so they are started later.
                info!("slack channel {name} will be started after state initialization");
            }
            "whatsapp" => {
                // WhatsApp channels need SharedState for callbacks, so they are started later.
                info!("whatsapp channel {name} will be started after state initialization");
            }
            "imessage" => {
                // iMessage channels need SharedState for callbacks, so they are started later.
                info!("imessage channel {name} will be started after state initialization");
            }
            other => {
                warn!("unknown channel type: {other} for channel {name}, skipping");
            }
        }
    }

    registry
}
