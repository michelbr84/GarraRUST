use garraia_config::AppConfig;
use tracing::{info, warn};

/// Build configured channels that can be initialized before state is wrapped in Arc.
///
/// `.env` loading used to happen here, but `Server::run` calls
/// `build_agent_runtime` — which reads the provider API-key env vars — *before*
/// it calls this function. That meant `.env` provisioned channels while leaving
/// providers dead for anyone embedding `Server` directly. The load now happens
/// at the top of `Server::run` instead.
pub async fn build_channels(config: &AppConfig) -> garraia_channels::ChannelRegistry {
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
