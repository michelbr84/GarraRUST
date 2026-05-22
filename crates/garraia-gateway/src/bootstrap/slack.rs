use std::sync::{Arc, Mutex};

use garraia_agents::ChatMessage;
use garraia_config::AppConfig;
use garraia_security::{Allowlist, PairingManager};
use tracing::{info, warn};

use crate::state::SharedState;

use super::config::{default_allowlist_path, resolve_api_key};

/// Build Slack channels from config. Must be called after state is
/// wrapped in `Arc` so the message callback can capture a `SharedState`.
pub fn build_slack_channels(
    config: &AppConfig,
    state: &SharedState,
) -> Vec<Box<dyn garraia_channels::Channel>> {
    let mut channels = Vec::new();

    for (name, channel_config) in &config.channels {
        if channel_config.channel_type != "slack" || channel_config.enabled == Some(false) {
            continue;
        }

        let bot_token = channel_config
            .settings
            .get("bot_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let bot_token =
            bot_token.or_else(|| resolve_api_key(None, "SLACK_BOT_TOKEN", "SLACK_BOT_TOKEN"));

        let Some(bot_token) = bot_token else {
            warn!(
                "slack channel '{name}' has no bot_token, skipping \
                 (set bot_token in config or SLACK_BOT_TOKEN env var)"
            );
            continue;
        };

        let app_token = channel_config
            .settings
            .get("app_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let app_token =
            app_token.or_else(|| resolve_api_key(None, "SLACK_APP_TOKEN", "SLACK_APP_TOKEN"));

        let Some(app_token) = app_token else {
            warn!(
                "slack channel '{name}' has no app_token, skipping \
                 (set app_token in config or SLACK_APP_TOKEN env var)"
            );
            continue;
        };

        let allowlist = Arc::new(Mutex::new(Allowlist::load_or_create(
            &default_allowlist_path(),
        )));

        let pairing = Arc::new(Mutex::new(PairingManager::new(
            std::time::Duration::from_secs(300),
        )));

        let state_for_cb = Arc::clone(state);
        let allowlist_for_cb = Arc::clone(&allowlist);
        let pairing_for_cb = Arc::clone(&pairing);

        let on_message: garraia_channels::SlackOnMessageFn = Arc::new(
            move |channel_id: String,
                  user_id: String,
                  user_name: String,
                  text: String,
                  delta_tx: Option<tokio::sync::mpsc::Sender<String>>| {
                let state = Arc::clone(&state_for_cb);
                let allowlist = Arc::clone(&allowlist_for_cb);
                let pairing = Arc::clone(&pairing_for_cb);
                Box::pin(async move {
                    // Allowlist / pairing check
                    {
                        let mut list = allowlist.lock().unwrap();
                        if list.needs_owner() {
                            list.claim_owner(&user_id);
                            info!("slack: auto-paired owner {} ({})", user_name, user_id);
                            return Ok(format!(
                                "Welcome, {}! You are now the owner of this GarraIA bot.\n\n\
                                 Use /pair to generate a code for adding other users.\n\
                                 Use /help for available commands.",
                                user_name
                            ));
                        }

                        if !list.is_allowed(&user_id) {
                            let trimmed = text.trim();
                            if trimmed.len() == 6 && trimmed.chars().all(|c| c.is_ascii_digit()) {
                                let claimed = pairing.lock().unwrap().claim(trimmed, &user_id);
                                if claimed.is_some() {
                                    list.add(&user_id);
                                    info!(
                                        "slack: paired user {} ({}) via code",
                                        user_name, user_id
                                    );
                                    return Ok(format!(
                                        "Welcome, {}! You now have access to this bot.",
                                        user_name
                                    ));
                                }
                            }

                            warn!(
                                "slack: unauthorized user {} ({}) in channel {}",
                                user_name, user_id, channel_id
                            );
                            return Err("__blocked__".to_string());
                        }
                    }

                    let session_id = format!("slack-{channel_id}");

                    let text = garraia_security::InputValidator::sanitize(&text);
                    if garraia_security::InputValidator::check_prompt_injection(&text) {
                        return Err(
                            "input rejected: potential prompt injection detected".to_string()
                        );
                    }

                    state
                        .hydrate_session_history(&session_id, Some("slack"), Some(&user_id))
                        .await;
                    let history: Vec<ChatMessage> = state.session_history(&session_id);
                    let continuity_key = state.continuity_key(Some(&user_id));

                    let response = if let Some(delta_sender) = delta_tx {
                        state
                            .agents
                            .process_message_streaming_with_context(
                                &session_id,
                                &text,
                                &history,
                                delta_sender,
                                continuity_key.as_deref(),
                                Some(&user_id),
                                None,
                            )
                            .await
                    } else {
                        state
                            .agents
                            .process_message_with_context(
                                &session_id,
                                &text,
                                &history,
                                continuity_key.as_deref(),
                                Some(&user_id),
                            )
                            .await
                    }
                    .map_err(|e| e.to_string())?;

                    state
                        .persist_turn(&session_id, Some("slack"), Some(&user_id), &text, &response)
                        .await;

                    Ok(response)
                })
            },
        );

        let channel = garraia_channels::SlackChannel::new(bot_token, app_token, on_message);
        channels.push(Box::new(channel) as Box<dyn garraia_channels::Channel>);
        info!("configured slack channel: {name}");
    }

    channels
}
