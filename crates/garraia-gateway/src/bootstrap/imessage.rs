#![cfg(target_os = "macos")]

use std::sync::{Arc, Mutex};

use garraia_channels::{IMessageChannel, IMessageOnMessageFn};
use garraia_config::AppConfig;
use garraia_security::{Allowlist, PairingManager};
use tracing::{info, warn};

use crate::state::SharedState;

use super::config::default_allowlist_path;

/// Build iMessage channels from config. macOS-only.
///
/// Must be called after state is wrapped in `Arc` so the message callback can capture a `SharedState`.
pub fn build_imessage_channels(
    config: &AppConfig,
    state: &SharedState,
) -> Vec<Box<dyn garraia_channels::Channel>> {
    let mut channels = Vec::new();

    for (name, channel_config) in &config.channels {
        if channel_config.channel_type != "imessage" || channel_config.enabled == Some(false) {
            continue;
        }

        let poll_interval_secs: u64 = channel_config
            .settings
            .get("poll_interval_secs")
            .and_then(|v| v.as_u64())
            .unwrap_or(2);

        let allowlist = Arc::new(Mutex::new(Allowlist::load_or_create(
            &default_allowlist_path(),
        )));

        let pairing = Arc::new(Mutex::new(PairingManager::new(
            std::time::Duration::from_secs(300),
        )));

        let state_for_cb = Arc::clone(state);
        let allowlist_for_cb = Arc::clone(&allowlist);
        let pairing_for_cb = Arc::clone(&pairing);

        let on_message: IMessageOnMessageFn = Arc::new(
            move |session_key: String,
                  sender_id: String,
                  text: String,
                  _delta_tx: Option<tokio::sync::mpsc::Sender<String>>| {
                let state = Arc::clone(&state_for_cb);
                let allowlist = Arc::clone(&allowlist_for_cb);
                let pairing = Arc::clone(&pairing_for_cb);
                Box::pin(async move {
                    // Allowlist / pairing check (always against the actual sender)
                    {
                        let mut list = allowlist.lock().unwrap();
                        if list.needs_owner() {
                            list.claim_owner(&sender_id);
                            info!("imessage: auto-paired owner {sender_id}");
                            return Ok("Welcome! You are now the owner of this GarraIA bot.\n\n\
                                 Send /pair to generate a code for adding other users.\n\
                                 Send /help for available commands."
                                .to_string());
                        }

                        if !list.is_allowed(&sender_id) {
                            let trimmed = text.trim();
                            if trimmed.len() == 6 && trimmed.chars().all(|c| c.is_ascii_digit()) {
                                let claimed = pairing.lock().unwrap().claim(trimmed, &sender_id);
                                if claimed.is_some() {
                                    list.add(&sender_id);
                                    info!("imessage: paired user {sender_id} via code");
                                    return Ok(
                                        "Welcome! You now have access to this bot.".to_string()
                                    );
                                }
                            }

                            warn!("imessage: unauthorized user {sender_id}");
                            return Err("__blocked__".to_string());
                        }
                    }

                    // session_key is group_name for groups, sender handle for DMs
                    let session_id = format!("imessage-{session_key}");

                    let text = garraia_security::InputValidator::sanitize(&text);
                    if garraia_security::InputValidator::check_prompt_injection(&text) {
                        return Err(
                            "input rejected: potential prompt injection detected".to_string()
                        );
                    }

                    state
                        .hydrate_session_history(&session_id, Some("imessage"), Some(&sender_id))
                        .await;
                    let history: Vec<garraia_agents::ChatMessage> =
                        state.session_history(&session_id);
                    let continuity_key = state.continuity_key(Some(&sender_id));

                    let response = state
                        .agents
                        .process_message_with_context(
                            &session_id,
                            &text,
                            &history,
                            continuity_key.as_deref(),
                            Some(&sender_id),
                        )
                        .await
                        .map_err(|e| e.to_string())?;

                    state
                        .persist_turn(
                            &session_id,
                            Some("imessage"),
                            Some(&sender_id),
                            &text,
                            &response,
                        )
                        .await;

                    Ok(response)
                })
            },
        );

        let channel = IMessageChannel::new(poll_interval_secs, on_message);
        channels.push(Box::new(channel) as Box<dyn garraia_channels::Channel>);
        info!("configured imessage channel: {name}");
    }

    channels
}
