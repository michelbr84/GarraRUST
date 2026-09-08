//! Ligação do canal Matrix (#1050).
//!
//! O `MatrixChannel` tem `impl Channel`, sync loop contra o homeserver e
//! envio de resposta desde antes desta issue. Faltava o call-site.
//!
//! Molde: `bootstrap/slack.rs`. O callback do Matrix tem exatamente a mesma
//! forma — `(room_id, user_id, user_name, text, delta_tx)` — então o que
//! muda aqui é a origem das credenciais e o prefixo do `session_id`.

use std::sync::{Arc, Mutex};

use garraia_agents::ChatMessage;
use garraia_config::AppConfig;
use garraia_security::{Allowlist, PairingManager};
use tracing::{info, warn};

use crate::state::SharedState;

use super::config::{default_allowlist_path, resolve_api_key};

/// Constrói os canais Matrix a partir da config. Chamado depois de o estado
/// virar `Arc`, para o callback poder capturar um `SharedState`.
pub fn build_matrix_channels(
    config: &AppConfig,
    state: &SharedState,
) -> Vec<Box<dyn garraia_channels::Channel>> {
    let mut channels = Vec::new();

    for (name, channel_config) in &config.channels {
        if channel_config.channel_type != "matrix" || channel_config.enabled == Some(false) {
            continue;
        }

        let homeserver_url = channel_config
            .settings
            .get("homeserver_url")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| std::env::var("MATRIX_HOMESERVER_URL").ok());

        let Some(homeserver_url) = homeserver_url else {
            warn!(
                "matrix channel '{name}' has no homeserver_url, skipping \
                 (set homeserver_url in config or the MATRIX_HOMESERVER_URL env var)"
            );
            continue;
        };

        let access_token = channel_config
            .settings
            .get("access_token")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| resolve_api_key(None, "MATRIX_ACCESS_TOKEN", "MATRIX_ACCESS_TOKEN"));

        let Some(access_token) = access_token else {
            warn!(
                "matrix channel '{name}' has no access_token, skipping \
                 (set access_token in config or the MATRIX_ACCESS_TOKEN env var)"
            );
            continue;
        };

        let room_ids: Vec<String> = channel_config
            .settings
            .get("room_ids")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        let allowlist = Arc::new(Mutex::new(Allowlist::load_or_create(
            &default_allowlist_path(),
        )));
        let pairing = Arc::new(Mutex::new(PairingManager::new(
            std::time::Duration::from_secs(300),
        )));

        let state_for_cb = Arc::clone(state);
        let allowlist_for_cb = Arc::clone(&allowlist);
        let pairing_for_cb = Arc::clone(&pairing);

        let on_message: garraia_channels::MatrixOnMessageFn = Arc::new(
            move |room_id: String,
                  user_id: String,
                  user_name: String,
                  text: String,
                  delta_tx: Option<tokio::sync::mpsc::Sender<String>>| {
                let state = Arc::clone(&state_for_cb);
                let allowlist = Arc::clone(&allowlist_for_cb);
                let pairing = Arc::clone(&pairing_for_cb);
                Box::pin(async move {
                    {
                        let mut list = allowlist.lock().unwrap();
                        if list.needs_owner() {
                            list.claim_owner(&user_id);
                            info!("matrix: auto-paired owner {user_name} ({user_id})");
                            return Ok(format!(
                                "Welcome, {user_name}! You are now the owner of this GarraIA \
                                 bot.\n\nUse /pair to generate a code for adding other users.\n\
                                 Use /help for available commands."
                            ));
                        }

                        if !list.is_allowed(&user_id) {
                            let trimmed = text.trim();
                            if trimmed.len() == 6 && trimmed.chars().all(|c| c.is_ascii_digit()) {
                                let claimed = pairing.lock().unwrap().claim(trimmed, &user_id);
                                if claimed.is_some() {
                                    list.add(&user_id);
                                    info!("matrix: paired user {user_name} ({user_id}) via code");
                                    return Ok(format!(
                                        "Welcome, {user_name}! You now have access to this bot."
                                    ));
                                }
                            }

                            warn!("matrix: unauthorized user {user_name} ({user_id}) in {room_id}");
                            return Err("__blocked__".to_string());
                        }
                    }

                    let session_id = format!("matrix-{room_id}");

                    let text = garraia_security::InputValidator::sanitize(&text);
                    if garraia_security::InputValidator::check_prompt_injection(&text) {
                        return Err(
                            "input rejected: potential prompt injection detected".to_string()
                        );
                    }

                    state
                        .hydrate_session_history(&session_id, Some("matrix"), Some(&user_id))
                        .await;
                    let history: Vec<ChatMessage> = state.session_history(&session_id);
                    let continuity_key = state.continuity_key();
                    // `exec_context_for`, nunca o wrapper `_with_context`: o
                    // segundo passa `ExecContext::default()` e faria o `/mode`
                    // do usuario nao valer neste canal (#988).
                    let exec = state.exec_context_for(&session_id, Some(&user_id)).await;

                    let response = if let Some(delta_sender) = delta_tx {
                        state
                            .agents
                            .process_message_streaming_with_agent_config(
                                &session_id,
                                &text,
                                &history,
                                delta_sender,
                                continuity_key.as_deref(),
                                Some(&user_id),
                                None,
                                None,
                                None,
                                None,
                                &exec,
                            )
                            .await
                    } else {
                        state
                            .agents
                            .process_message_with_agent_config(
                                &session_id,
                                &text,
                                &history,
                                continuity_key.as_deref(),
                                Some(&user_id),
                                None,
                                None,
                                None,
                                None,
                                &exec,
                            )
                            .await
                    }
                    .map_err(|e| e.to_string())?;

                    state
                        .persist_turn(
                            &session_id,
                            Some("matrix"),
                            Some(&user_id),
                            &text,
                            &response,
                        )
                        .await;

                    Ok(response)
                })
            },
        );

        let channel = garraia_channels::MatrixChannel::new(
            garraia_channels::MatrixConfig {
                homeserver_url,
                access_token,
                room_ids,
            },
            on_message,
        );
        channels.push(Box::new(channel) as Box<dyn garraia_channels::Channel>);
        info!("configured matrix channel: {name}");
    }

    channels
}
