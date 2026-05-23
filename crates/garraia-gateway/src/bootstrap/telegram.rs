use std::sync::{Arc, Mutex};

use garraia_agents::ChatMessage;
use garraia_channels::{OnVoiceFn, TelegramChannel};
use garraia_config::AppConfig;
use garraia_security::{Allowlist, PairingManager};
use teloxide::net::Download;
use teloxide::prelude::Requester;
use tracing::{error, info, warn};

use crate::state::SharedState;

use super::config::{default_allowlist_path, resolve_api_key};

pub fn build_telegram_voice_handler(state: &SharedState) -> Option<OnVoiceFn> {
    // Check if voice clients are available
    let stt_client = state.stt_client.clone()?;
    let voice_client = state.voice_client.clone()?;

    let state_for_handler = Arc::clone(state);

    Some(Arc::new(
        move |bot: teloxide::Bot, msg: teloxide::types::Message| {
            let stt = Arc::clone(&stt_client);
            let tts = Arc::clone(&voice_client);
            let state = Arc::clone(&state_for_handler);

            Box::pin(async move {
                // Extract required data from message
                let chat_id = msg.chat.id.0;
                let user = msg.from.as_ref().ok_or("missing user info")?;
                let user_id = user.id.0.to_string();
                let user_name = user.first_name.clone();

                // Get the voice file ID
                let voice = msg.voice().ok_or("no voice in message")?;
                let file_id = voice.file.id.clone();

                info!(
                    "telegram voice: processing voice from {} [uid={}] in chat {}",
                    user_name, user_id, chat_id
                );

                // Download the voice file
                let voice_file = bot.get_file(file_id).await.map_err(|e| {
                    error!("failed to get voice file: {}", e);
                    e.to_string()
                })?;

                // Download to a temporary file
                let temp_dir = std::env::temp_dir();
                let temp_path =
                    temp_dir.join(format!("garraia_voice_{}.ogg", uuid::Uuid::new_v4()));

                let mut file = tokio::fs::File::create(&temp_path).await.map_err(|e| {
                    error!("failed to create temp file: {}", e);
                    e.to_string()
                })?;

                bot.download_file(&voice_file.path, &mut file)
                    .await
                    .map_err(|e| {
                        error!("failed to download voice: {}", e);
                        e.to_string()
                    })?;

                // Transcribe with Whisper
                let text = stt.transcribe(&temp_path).await.map_err(|e| {
                    error!("STT failed: {}", e);
                    e.to_string()
                })?;

                info!("telegram voice: transcribed: {}", text);

                // Clean up temp file
                let _ = tokio::fs::remove_file(&temp_path).await;

                // Check allowlist
                {
                    let allowlist = state.allowlist.lock().unwrap();
                    if !allowlist.is_allowed(&user_id) && !allowlist.is_owner(&user_id) {
                        warn!(
                            "telegram voice: unauthorized user {} in chat {}",
                            user_id, chat_id
                        );
                        return Err("__blocked__".to_string());
                    }
                }

                // GAR-202: Resolve session via UUID-based key (replaces guessable telegram-{chat_id})
                let session_id = if let Some(mgr) = &state.chat_session_manager {
                    let hints = garraia_db::SessionHints::from_telegram(chat_id, None);
                    mgr.resolve_session(&hints)
                        .await
                        .unwrap_or_else(|_| format!("telegram-{chat_id}"))
                } else {
                    format!("telegram-{chat_id}")
                };
                state
                    .hydrate_session_history(&session_id, Some("telegram"), Some(&user_id))
                    .await;
                let history: Vec<ChatMessage> = state.session_history(&session_id);
                let continuity_key = state.continuity_key(Some(&user_id));

                // Send typing indicator
                let _ = bot
                    .send_chat_action(
                        teloxide::types::ChatId(chat_id),
                        teloxide::types::ChatAction::Typing,
                    )
                    .await;

                // Process with LLM
                let response = state
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
                    )
                    .await
                    .map_err(|e| {
                        error!("LLM processing failed: {}", e);
                        e.to_string()
                    })?;

                info!(
                    "telegram voice: LLM response ({} chars) for user {}",
                    response.len(),
                    user_id
                );

                // Persist the turn
                state
                    .persist_turn(
                        &session_id,
                        Some("telegram"),
                        Some(&user_id),
                        &text,
                        &response,
                    )
                    .await;

                // Try to synthesize voice response
                match tts.synthesize_bytes(&response, "default").await {
                    Ok(audio_data) => {
                        // Save audio to temp file
                        let audio_path =
                            temp_dir.join(format!("garraia_response_{}.wav", uuid::Uuid::new_v4()));
                        if let Err(e) = tokio::fs::write(&audio_path, &audio_data).await {
                            error!("failed to write audio file: {}", e);
                            // Fallback to text
                            let _ = bot
                                .send_message(teloxide::types::ChatId(chat_id), &response)
                                .await;
                        } else {
                            // Send voice message
                            let input_file = teloxide::types::InputFile::file(&audio_path);
                            if let Err(e) = bot
                                .send_voice(teloxide::types::ChatId(chat_id), input_file)
                                .await
                            {
                                error!("failed to send voice: {}", e);
                                // Fallback to text
                                let _ = bot
                                    .send_message(teloxide::types::ChatId(chat_id), &response)
                                    .await;
                            }
                            // Clean up audio file
                            let _ = tokio::fs::remove_file(&audio_path).await;
                        }
                    }
                    Err(e) => {
                        error!("TTS failed: {}", e);
                        // Fallback to text if TTS fails
                        let _ = bot
                            .send_message(teloxide::types::ChatId(chat_id), &response)
                            .await;
                    }
                }

                info!(
                    "telegram voice: successfully processed voice from {}",
                    user_name
                );
                Ok(())
            })
        },
    ))
}

/// Build Telegram channels from config. Must be called after state is
/// wrapped in `Arc` so the message callback can capture a `SharedState`.
pub fn build_telegram_channels(
    config: &AppConfig,
    state: &SharedState,
) -> Vec<Box<dyn garraia_channels::Channel>> {
    let mut channels = Vec::new();

    for (name, channel_config) in &config.channels {
        if channel_config.channel_type != "telegram" || channel_config.enabled == Some(false) {
            continue;
        }

        let bot_token = channel_config
            .settings
            .get("bot_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let bot_token =
            bot_token.or_else(|| resolve_api_key(None, "TELEGRAM_BOT_TOKEN", "TELEGRAM_BOT_TOKEN"));

        let Some(bot_token) = bot_token else {
            warn!(
                "telegram channel '{name}' has no bot_token, skipping \
                 (set bot_token in config or TELEGRAM_BOT_TOKEN env var)"
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

        let on_message: garraia_channels::OnMessageFn = Arc::new(
            move |chat_id: i64,
                  user_id: String,
                  user_name: String,
                  text: String,
                  delta_tx: Option<tokio::sync::mpsc::Sender<String>>| {
                let state = Arc::clone(&state_for_cb);
                let allowlist = Arc::clone(&allowlist_for_cb);
                let pairing = Arc::clone(&pairing_for_cb);
                Box::pin(async move {
                    if let Some(cmd) = text.strip_prefix('/') {
                        let _cmd = cmd.split_whitespace().next().unwrap_or("");
                        return handle_command(&text, &user_id, &user_name, chat_id, &state);
                    }

                    {
                        let mut list = allowlist.lock().unwrap();
                        if list.needs_owner() {
                            list.claim_owner(&user_id);
                            info!("telegram: auto-paired owner {} ({})", user_name, user_id);
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
                                        "telegram: paired user {} ({}) via code",
                                        user_name, user_id
                                    );
                                    return Ok(format!(
                                        "Welcome, {}! You now have access to this bot.",
                                        user_name
                                    ));
                                }
                            }

                            warn!(
                                "telegram: unauthorized user {} ({}) in chat {}",
                                user_name, user_id, chat_id
                            );
                            return Err("__blocked__".to_string());
                        }
                    }

                    // GAR-202: UUID-based session ID (replaces guessable telegram-{chat_id})
                    let session_id = if let Some(mgr) = &state.chat_session_manager {
                        let uid_i64 = user_id.parse::<i64>().ok();
                        let hints = garraia_db::SessionHints::from_telegram(chat_id, uid_i64);
                        mgr.resolve_session(&hints)
                            .await
                            .unwrap_or_else(|_| format!("telegram-{chat_id}"))
                    } else {
                        format!("telegram-{chat_id}")
                    };

                    let text = garraia_security::InputValidator::sanitize(&text);
                    if garraia_security::InputValidator::check_prompt_injection(&text) {
                        return Err(
                            "input rejected: potential prompt injection detected".to_string()
                        );
                    }

                    state
                        .hydrate_session_history(&session_id, Some("telegram"), Some(&user_id))
                        .await;
                    let history: Vec<ChatMessage> = state.session_history(&session_id);
                    let continuity_key = state.continuity_key(Some(&user_id));

                    let model_override = state
                        .channel_models
                        .get(&session_id)
                        .map(|r| r.value().clone());

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
                                model_override.as_deref(),
                                None,
                                None,
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
                                model_override.as_deref(),
                                None,
                                None,
                            )
                            .await
                    }
                    .map_err(|e| e.to_string())?;

                    state
                        .persist_turn(
                            &session_id,
                            Some("telegram"),
                            Some(&user_id),
                            &text,
                            &response,
                        )
                        .await;

                    Ok(response)
                })
            },
        );

        let channel = TelegramChannel::new(bot_token, on_message);

        // Add voice handler if voice mode is enabled
        let voice_handler = build_telegram_voice_handler(state);
        let channel = if let Some(handler) = voice_handler {
            info!("telegram: voice handler enabled for channel {}", name);
            channel.with_voice_handler(handler)
        } else {
            channel
        };

        // Build commands for Telegram menu from the registry
        let menu_commands: Vec<(String, String)> = state
            .command_registry
            .read()
            .unwrap()
            .telegram_commands()
            .into_iter()
            .map(|(name, desc)| (name.to_string(), desc.to_string()))
            .collect();
        let channel = channel.with_commands(menu_commands);

        channels.push(Box::new(channel) as Box<dyn garraia_channels::Channel>);
        info!("configured telegram channel: {name}");
    }

    channels
}

#[allow(clippy::too_many_arguments)]
fn handle_command(
    full_text: &str,
    user_id: &str,
    user_name: &str,
    chat_id: i64,
    state: &SharedState,
) -> std::result::Result<String, String> {
    let list = state.allowlist.lock().unwrap();
    let is_owner = list.is_owner(user_id);
    drop(list);

    // Build a CommandContext and dispatch via the registry
    let role = if is_owner {
        garraia_channels::Role::Owner
    } else {
        garraia_channels::Role::User
    };
    let args: Vec<String> = full_text
        .split_whitespace()
        .skip(1)
        .map(|s| s.to_string())
        .collect();
    let ctx = garraia_channels::CommandContext {
        user_id: user_id.to_string(),
        user_name: user_name.to_string(),
        chat_id,
        full_text: full_text.to_string(),
        args,
        user_role: role,
        state: Some(Arc::clone(state) as Arc<dyn std::any::Any + Send + Sync>),
    };

    match state.command_registry.read().unwrap().dispatch(&ctx) {
        Ok(response) => Ok(response),
        Err(garraia_channels::CommandError::Unauthorized(msg)) => Ok(format!("⛔ {msg}")),
        Err(garraia_channels::CommandError::InvalidArgs(msg)) => Ok(format!("❌ {msg}")),
        Err(garraia_channels::CommandError::Internal(msg)) => {
            Ok(format!("💥 Internal error: {msg}"))
        }
        Err(garraia_channels::CommandError::Blocked) => Err("__blocked__".to_string()),
    }
}
