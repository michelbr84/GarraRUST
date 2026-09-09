//! Ligação do canal Signal (#1050).
//!
//! O `SignalChannel` está completo desde antes desta issue — `impl Channel`,
//! polling do daemon signal-cli, e o guard `vet_signal_cli_url` que impede
//! que a URL do daemon aponte para onde não deve. Faltava o que falta em
//! todos os sete: alguém que leia a config e o registre.
//!
//! Segue o molde de `bootstrap/slack.rs`, que é o mais completo dos que já
//! existiam: allowlist com pareamento por código, sanitização, recusa de
//! injeção de prompt, e `exec_context_for` — nunca o wrapper
//! `_with_context`, que passa `ExecContext::default()` e faria o `/mode`
//! escolhido pelo usuário não valer neste canal (a assimetria silenciosa do
//! #988).

use std::sync::{Arc, Mutex};

use garraia_agents::ChatMessage;
use garraia_config::AppConfig;
use garraia_security::{Allowlist, PairingManager};
use tracing::{info, warn};

use crate::state::SharedState;

use super::config::default_allowlist_path;

/// Constrói os canais Signal a partir da config. Chamado depois de o estado
/// virar `Arc`, para o callback poder capturar um `SharedState`.
pub fn build_signal_channels(
    config: &AppConfig,
    state: &SharedState,
) -> Vec<Box<dyn garraia_channels::Channel>> {
    let mut channels = Vec::new();

    for (name, channel_config) in &config.channels {
        if channel_config.channel_type != "signal" || channel_config.enabled == Some(false) {
            continue;
        }

        // O daemon signal-cli não usa token: a credencial é o próprio número
        // registrado nele. Os dois campos são obrigatórios, e sem qualquer um
        // deles o canal não tem o que fazer.
        let signal_cli_url = channel_config
            .settings
            .get("signal_cli_url")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| std::env::var("SIGNAL_CLI_URL").ok());

        let Some(signal_cli_url) = signal_cli_url else {
            warn!(
                "signal channel '{name}' has no signal_cli_url, skipping \
                 (set signal_cli_url in config or the SIGNAL_CLI_URL env var)"
            );
            continue;
        };

        let phone_number = channel_config
            .settings
            .get("phone_number")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| std::env::var("SIGNAL_PHONE_NUMBER").ok());

        let Some(phone_number) = phone_number else {
            warn!(
                "signal channel '{name}' has no phone_number, skipping \
                 (set phone_number in config or the SIGNAL_PHONE_NUMBER env var)"
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

        let on_message: garraia_channels::SignalOnMessageFn = Arc::new(
            move |source_number: String,
                  source_name: String,
                  text: String,
                  delta_tx: Option<tokio::sync::mpsc::Sender<String>>| {
                let state = Arc::clone(&state_for_cb);
                let allowlist = Arc::clone(&allowlist_for_cb);
                let pairing = Arc::clone(&pairing_for_cb);
                Box::pin(async move {
                    // Allowlist / pareamento. O número é a identidade no
                    // Signal, então é ele que entra na lista.
                    {
                        let mut list = allowlist.lock().unwrap();
                        if list.needs_owner() {
                            list.claim_owner(&source_number);
                            info!("signal: auto-paired owner {source_name}");
                            return Ok(format!(
                                "Welcome, {source_name}! You are now the owner of this GarraIA \
                                 bot.\n\nUse /pair to generate a code for adding other users.\n\
                                 Use /help for available commands."
                            ));
                        }

                        if !list.is_allowed(&source_number) {
                            let trimmed = text.trim();
                            if trimmed.len() == 6 && trimmed.chars().all(|c| c.is_ascii_digit()) {
                                let claimed =
                                    pairing.lock().unwrap().claim(trimmed, &source_number);
                                if claimed.is_some() {
                                    list.add(&source_number);
                                    info!("signal: paired user {source_name} via code");
                                    return Ok(format!(
                                        "Welcome, {source_name}! You now have access to this bot."
                                    ));
                                }
                            }

                            warn!("signal: unauthorized sender {source_name}");
                            return Err("__blocked__".to_string());
                        }
                    }

                    let session_id = format!("signal-{source_number}");

                    let text = garraia_security::InputValidator::sanitize(&text);
                    if garraia_security::InputValidator::check_prompt_injection(&text) {
                        return Err(
                            "input rejected: potential prompt injection detected".to_string()
                        );
                    }

                    state
                        .hydrate_session_history(&session_id, Some("signal"), Some(&source_number))
                        .await;
                    let history: Vec<ChatMessage> = state.session_history(&session_id);
                    let continuity_key = state.continuity_key();
                    let exec = state
                        .exec_context_for(&session_id, Some(&source_number))
                        .await;

                    let response = if let Some(delta_sender) = delta_tx {
                        state
                            .agents
                            .process_message_streaming_with_agent_config(
                                &session_id,
                                &text,
                                &history,
                                delta_sender,
                                continuity_key.as_deref(),
                                Some(&source_number),
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
                                Some(&source_number),
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
                            Some("signal"),
                            Some(&source_number),
                            &text,
                            &response,
                        )
                        .await;

                    Ok(response)
                })
            },
        );

        let channel = garraia_channels::SignalChannel::new(
            garraia_channels::SignalConfig {
                signal_cli_url,
                phone_number,
            },
            on_message,
        );
        channels.push(Box::new(channel) as Box<dyn garraia_channels::Channel>);
        info!("configured signal channel: {name}");
    }

    channels
}
