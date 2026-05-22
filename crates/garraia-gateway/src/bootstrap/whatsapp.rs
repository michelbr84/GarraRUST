use std::sync::{Arc, Mutex};

use garraia_agents::ChatMessage;
use garraia_channels::{WhatsAppChannel, WhatsAppOnMessageFn};
use garraia_config::AppConfig;
use garraia_security::{Allowlist, PairingManager};
use tracing::{info, warn};

use crate::state::SharedState;

use super::config::{default_allowlist_path, resolve_api_key};

/// Build WhatsApp channels from config. Must be called after state is
/// wrapped in `Arc` so the message callback can capture a `SharedState`.
pub fn build_whatsapp_channels(
    config: &AppConfig,
    state: &SharedState,
) -> Vec<Arc<WhatsAppChannel>> {
    let mut channels = Vec::new();

    for (name, channel_config) in &config.channels {
        if channel_config.channel_type != "whatsapp" || channel_config.enabled == Some(false) {
            continue;
        }

        let access_token = channel_config
            .settings
            .get("access_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let access_token = access_token
            .or_else(|| resolve_api_key(None, "WHATSAPP_ACCESS_TOKEN", "WHATSAPP_ACCESS_TOKEN"));

        let Some(access_token) = access_token else {
            warn!(
                "whatsapp channel '{name}' has no access_token, skipping \
                 (set access_token in config or WHATSAPP_ACCESS_TOKEN env var)"
            );
            continue;
        };

        let phone_number_id = channel_config
            .settings
            .get("phone_number_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        if phone_number_id.is_empty() {
            warn!("whatsapp channel '{name}' has no phone_number_id, skipping");
            continue;
        }

        let verify_token = channel_config
            .settings
            .get("verify_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| std::env::var("WHATSAPP_VERIFY_TOKEN").ok())
            .unwrap_or_else(|| "garraia-verify".to_string());

        let allowlist = Arc::new(Mutex::new(Allowlist::load_or_create(
            &default_allowlist_path(),
        )));

        let pairing = Arc::new(Mutex::new(PairingManager::new(
            std::time::Duration::from_secs(300),
        )));

        let state_for_cb = Arc::clone(state);
        let allowlist_for_cb = Arc::clone(&allowlist);
        let pairing_for_cb = Arc::clone(&pairing);

        let on_message: WhatsAppOnMessageFn = Arc::new(
            move |from_number: String,
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
                            list.claim_owner(&from_number);
                            info!(
                                "whatsapp: auto-paired owner {} ({})",
                                user_name, from_number
                            );
                            return Ok(format!(
                                "Welcome, {}! You are now the owner of this GarraIA bot.\n\n\
                                 Send /pair to generate a code for adding other users.\n\
                                 Send /help for available commands.",
                                user_name
                            ));
                        }

                        if !list.is_allowed(&from_number) {
                            let trimmed = text.trim();
                            if trimmed.len() == 6 && trimmed.chars().all(|c| c.is_ascii_digit()) {
                                let claimed = pairing.lock().unwrap().claim(trimmed, &from_number);
                                if claimed.is_some() {
                                    list.add(&from_number);
                                    info!(
                                        "whatsapp: paired user {} ({}) via code",
                                        user_name, from_number
                                    );
                                    return Ok(format!(
                                        "Welcome, {}! You now have access to this bot.",
                                        user_name
                                    ));
                                }
                            }

                            warn!(
                                "whatsapp: unauthorized user {} ({})",
                                user_name, from_number
                            );
                            return Err("__blocked__".to_string());
                        }
                    }

                    let session_id = format!("whatsapp-{from_number}");

                    let text = garraia_security::InputValidator::sanitize(&text);
                    if garraia_security::InputValidator::check_prompt_injection(&text) {
                        return Err(
                            "input rejected: potential prompt injection detected".to_string()
                        );
                    }

                    state
                        .hydrate_session_history(&session_id, Some("whatsapp"), Some(&from_number))
                        .await;
                    let history: Vec<ChatMessage> = state.session_history(&session_id);
                    let continuity_key = state.continuity_key(Some(&from_number));

                    let response = if let Some(delta_sender) = delta_tx {
                        state
                            .agents
                            .process_message_streaming_with_context(
                                &session_id,
                                &text,
                                &history,
                                delta_sender,
                                continuity_key.as_deref(),
                                Some(&from_number),
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
                                Some(&from_number),
                            )
                            .await
                    }
                    .map_err(|e| e.to_string())?;

                    state
                        .persist_turn(
                            &session_id,
                            Some("whatsapp"),
                            Some(&from_number),
                            &text,
                            &response,
                        )
                        .await;

                    Ok(response)
                })
            },
        );

        let channel = Arc::new(WhatsAppChannel::new(
            access_token,
            phone_number_id,
            verify_token,
            on_message,
        ));
        channels.push(channel);
        info!("configured whatsapp channel: {name}");
    }

    channels
}
