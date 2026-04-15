//! GAR-208: Background context summarization for long sessions.
//!
//! `maybe_trigger_summarization` is called after each turn is persisted.
//! It checks whether the session has accumulated enough new messages (per
//! `summarize_threshold`) and, if so, summarizes the older portion of the
//! conversation with a cheap LLM call and stores the result in `chat_summaries`.
//!
//! The function is designed to be called inside a `tokio::spawn` so that it
//! never blocks the response path.

use std::sync::Arc;

use garraia_agents::{ChatMessage, ChatRole, MessagePart, context_policy::summarize_messages};
use tracing::{info, warn};

/// After a turn is persisted, check if summarization should fire and run it.
///
/// * `state`      — shared app state (access to session store + agent runtime).
/// * `session_id` — the session that just received a new turn.
///
/// This is intentionally synchronous-looking but must be called from a spawned task.
pub async fn maybe_trigger_summarization(state: Arc<crate::state::AppState>, session_id: String) {
    let policy = state.agents.context_policy().clone();
    let threshold = match policy.summarize_threshold {
        Some(t) => t,
        None => return, // summarization disabled
    };

    let Some(store) = &state.session_store else {
        return;
    };

    // Read DB counts under a short lock.
    let (total_count, last_summary_count) = {
        let guard = store.lock().await;
        let total = match guard.get_message_count(&session_id) {
            Ok(n) => n,
            Err(e) => {
                warn!(error = %e, session = %session_id, "context_summarizer: get_message_count failed");
                return;
            }
        };
        let last = match guard.get_latest_session_summary(&session_id) {
            Ok(Some((_, count))) => count,
            Ok(None) => 0,
            Err(e) => {
                warn!(error = %e, session = %session_id, "context_summarizer: get_latest_session_summary failed");
                0
            }
        };
        (total, last)
    };

    if !policy.should_summarize(total_count, last_summary_count) {
        return;
    }

    // Load the messages that are not yet covered by the current summary.
    // We summarize from message `last_summary_count` up to `total_count - threshold/2`
    // (leave the most recent half-window for the live context).
    let offset = last_summary_count as usize;
    let end = (total_count as usize).saturating_sub(threshold / 2);
    if end <= offset {
        return; // nothing to summarize
    }
    let limit = end - offset;

    let raw_messages = {
        let guard = store.lock().await;
        match guard.load_older_messages(&session_id, offset, limit) {
            Ok(msgs) => msgs,
            Err(e) => {
                warn!(error = %e, session = %session_id, "context_summarizer: load_older_messages failed");
                return;
            }
        }
    };

    if raw_messages.is_empty() {
        return;
    }

    // Convert to ChatMessage for the summarizer.
    let chat_messages: Vec<ChatMessage> = raw_messages
        .into_iter()
        .filter_map(|m| match m.direction.as_str() {
            "user" => Some(ChatMessage {
                role: ChatRole::User,
                content: MessagePart::Text(m.content),
            }),
            "assistant" => Some(ChatMessage {
                role: ChatRole::Assistant,
                content: MessagePart::Text(m.content),
            }),
            _ => None,
        })
        .collect();

    if chat_messages.is_empty() {
        return;
    }

    // Pick a provider + optional model override.
    let cfg = state.current_config();
    let summarizer_model = cfg.agent.summarizer_model.as_deref();

    // Resolve the primary provider from the runtime.
    let provider = {
        let providers = state.agents.list_providers();
        if providers.is_empty() {
            warn!(session = %session_id, "context_summarizer: no LLM provider available");
            return;
        }
        // If a specific model is requested, try to find the matching provider.
        if let Some(model) = summarizer_model {
            if let Some((prefix, _)) = model.split_once('/') {
                providers
                    .into_iter()
                    .find(|p| p.provider_id() == prefix)
                    .unwrap_or_else(|| state.agents.list_providers().into_iter().next().unwrap())
            } else {
                providers.into_iter().next().unwrap()
            }
        } else {
            providers.into_iter().next().unwrap()
        }
    };

    info!(
        session = %session_id,
        messages = chat_messages.len(),
        "context_summarizer: running summarization"
    );

    let summary = summarize_messages(provider, summarizer_model, &chat_messages).await;

    if let Some(text) = summary {
        let message_count = total_count;
        let guard = store.lock().await;
        if let Err(e) = guard.save_session_summary(&session_id, &text, message_count) {
            warn!(error = %e, session = %session_id, "context_summarizer: save_session_summary failed");
        } else {
            info!(session = %session_id, "context_summarizer: summary saved ({message_count} msgs)");
        }
    }
}
