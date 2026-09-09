//! Ligação do canal IRC (#1050).
//!
//! O `IrcChannel` tem `impl Channel`, conexão TCP com TLS opcional, JOIN nos
//! canais configurados e parsing de PRIVMSG desde antes desta issue. Faltava
//! o call-site.
//!
//! Molde: `bootstrap/slack.rs`. O callback do IRC tem a mesma forma —
//! `(channel_name, nick, user_name, text, delta_tx)` — então o que muda é a
//! origem da config e o prefixo do `session_id`.
//!
//! **Sem token.** No IRC a identidade é o nick, que qualquer um pode
//! reivindicar num servidor sem NickServ. Por isso a allowlist com pareamento
//! não é conveniência aqui: é o único controle de acesso que existe neste
//! canal, e vale a pena dizer isso em voz alta.

use std::sync::{Arc, Mutex};

use garraia_agents::ChatMessage;
use garraia_config::AppConfig;
use garraia_security::{Allowlist, PairingManager};
use tracing::{info, warn};

use crate::state::SharedState;

use super::config::default_allowlist_path;

/// Constrói os canais IRC a partir da config. Chamado depois de o estado
/// virar `Arc`, para o callback poder capturar um `SharedState`.
pub fn build_irc_channels(
    config: &AppConfig,
    state: &SharedState,
) -> Vec<Box<dyn garraia_channels::Channel>> {
    let mut channels = Vec::new();

    for (name, channel_config) in &config.channels {
        if channel_config.channel_type != "irc" || channel_config.enabled == Some(false) {
            continue;
        }

        let server = channel_config
            .settings
            .get("server")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| std::env::var("IRC_SERVER").ok());

        let Some(server) = server else {
            warn!(
                "irc channel '{name}' has no server, skipping \
                 (set server in config or the IRC_SERVER env var)"
            );
            continue;
        };

        let salas: Vec<String> = channel_config
            .settings
            .get("channels")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        if salas.is_empty() {
            warn!(
                "irc channel '{name}' has no channels to join, skipping \
                 (set `channels = [\"#sala\"]` under the channel settings)"
            );
            continue;
        }

        let use_tls = channel_config
            .settings
            .get("use_tls")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        // A porta default segue o `use_tls`: 6697 com TLS, 6667 sem. O default
        // do `IrcConfig` e 6667 fixo, o que daria uma conexao em claro numa
        // porta de TLS se o operador ligasse `use_tls` sem dizer a porta.
        let port = channel_config
            .settings
            .get("port")
            .and_then(serde_json::Value::as_u64)
            .and_then(|p| u16::try_from(p).ok())
            .unwrap_or(if use_tls { 6697 } else { 6667 });

        let nick = channel_config
            .settings
            .get("nick")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| "garrabot".to_string());

        let allowlist = Arc::new(Mutex::new(Allowlist::load_or_create(
            &default_allowlist_path(),
        )));
        let pairing = Arc::new(Mutex::new(PairingManager::new(
            std::time::Duration::from_secs(300),
        )));

        let state_for_cb = Arc::clone(state);
        let allowlist_for_cb = Arc::clone(&allowlist);
        let pairing_for_cb = Arc::clone(&pairing);

        let on_message: garraia_channels::IrcOnMessageFn = Arc::new(
            move |channel_name: String,
                  nick: String,
                  user_name: String,
                  text: String,
                  delta_tx: Option<tokio::sync::mpsc::Sender<String>>| {
                let state = Arc::clone(&state_for_cb);
                let allowlist = Arc::clone(&allowlist_for_cb);
                let pairing = Arc::clone(&pairing_for_cb);
                Box::pin(async move {
                    // No IRC o nick e a unica identidade que existe, e num
                    // servidor sem NickServ qualquer um pode toma-lo. A
                    // allowlist e o unico controle de acesso deste canal.
                    {
                        let mut list = allowlist.lock().unwrap();
                        if list.needs_owner() {
                            list.claim_owner(&nick);
                            info!("irc: auto-paired owner {nick}");
                            return Ok(format!(
                                "Welcome, {user_name}! You are now the owner of this GarraIA \
                                 bot. Use /pair to generate a code for adding other users."
                            ));
                        }

                        if !list.is_allowed(&nick) {
                            let trimmed = text.trim();
                            if trimmed.len() == 6 && trimmed.chars().all(|c| c.is_ascii_digit()) {
                                let claimed = pairing.lock().unwrap().claim(trimmed, &nick);
                                if claimed.is_some() {
                                    list.add(&nick);
                                    info!("irc: paired user {nick} via code");
                                    return Ok(format!(
                                        "Welcome, {user_name}! You now have access to this bot."
                                    ));
                                }
                            }

                            warn!("irc: unauthorized nick {nick} in {channel_name}");
                            return Err("__blocked__".to_string());
                        }
                    }

                    let session_id = format!("irc-{channel_name}");

                    let text = garraia_security::InputValidator::sanitize(&text);
                    if garraia_security::InputValidator::check_prompt_injection(&text) {
                        return Err(
                            "input rejected: potential prompt injection detected".to_string()
                        );
                    }

                    state
                        .hydrate_session_history(&session_id, Some("irc"), Some(&nick))
                        .await;
                    let history: Vec<ChatMessage> = state.session_history(&session_id);
                    let continuity_key = state.continuity_key();
                    // `exec_context_for`, nunca o wrapper `_with_context` (#988).
                    let exec = state.exec_context_for(&session_id, Some(&nick)).await;

                    let response = if let Some(delta_sender) = delta_tx {
                        state
                            .agents
                            .process_message_streaming_with_agent_config(
                                &session_id,
                                &text,
                                &history,
                                delta_sender,
                                continuity_key.as_deref(),
                                Some(&nick),
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
                                Some(&nick),
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
                        .persist_turn(&session_id, Some("irc"), Some(&nick), &text, &response)
                        .await;

                    Ok(response)
                })
            },
        );

        let channel = garraia_channels::IrcChannel::new(
            garraia_channels::IrcConfig {
                server,
                port,
                nick,
                channels: salas,
                use_tls,
            },
            on_message,
        );
        channels.push(Box::new(channel) as Box<dyn garraia_channels::Channel>);
        info!("configured irc channel: {name}");
    }

    channels
}
