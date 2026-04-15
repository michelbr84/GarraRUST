//! GAR-208: Context window policy and auto-summarization helpers.
//!
//! `ContextPolicy` is a lightweight value type that knows two things:
//!   1. How many recent messages to forward to the LLM (sliding window).
//!   2. When to trigger a background summarization pass.
//!
//! Summarization is always optional — if the LLM call fails or the feature is
//! disabled the runtime degrades gracefully to the plain window.

use crate::providers::{ChatMessage, ChatRole, LlmProvider, LlmRequest, MessagePart};
use std::sync::Arc;
use tracing::warn;

/// Controls the history presented to the LLM and when to summarize old turns.
#[derive(Debug, Clone, Default)]
pub struct ContextPolicy {
    /// Keep only the last N messages sent to the LLM.
    /// `None` means forward everything (bounded later by token budget).
    pub max_history_messages: Option<usize>,
    /// Trigger summarization when `(total_msgs - last_summary_msg_count) >= threshold`.
    /// `None` means summarization is disabled.
    pub summarize_threshold: Option<usize>,
}

impl ContextPolicy {
    pub fn new(max_history_messages: Option<usize>, summarize_threshold: Option<usize>) -> Self {
        Self {
            max_history_messages,
            summarize_threshold,
        }
    }

    /// Return the slice of `history` that should be forwarded to the LLM.
    ///
    /// If `max_history_messages` is set and `history.len() > limit`, the oldest
    /// messages are dropped.  The slice always ends with the caller's user message
    /// (callers push that *after* calling this, so we just slice).
    pub fn apply_window<'a>(&self, history: &'a [ChatMessage]) -> &'a [ChatMessage] {
        match self.max_history_messages {
            Some(limit) if history.len() > limit => &history[history.len() - limit..],
            _ => history,
        }
    }

    /// Returns `true` when a summarization pass should be triggered.
    ///
    /// * `current_db_count`  — total messages persisted to DB for this session.
    /// * `last_summary_count` — `message_count` stored in the latest `chat_summaries` row
    ///   (`0` if no summary exists yet).
    pub fn should_summarize(&self, current_db_count: i32, last_summary_count: i32) -> bool {
        match self.summarize_threshold {
            Some(threshold) => {
                let new_since = current_db_count.saturating_sub(last_summary_count);
                new_since >= threshold as i32
            }
            None => false,
        }
    }
}

/// Call the LLM to produce a short summary of `messages`.
///
/// Returns `None` if the provider call fails (caller degrades gracefully).
pub async fn summarize_messages(
    provider: Arc<dyn LlmProvider>,
    model: Option<&str>,
    messages: &[ChatMessage],
) -> Option<String> {
    if messages.is_empty() {
        return None;
    }

    // Build a condensed transcript for the summarizer prompt
    let transcript: String = messages
        .iter()
        .map(|m| {
            let role = match m.role {
                ChatRole::User => "User",
                ChatRole::Assistant => "Assistant",
                ChatRole::System => "System",
                ChatRole::Tool => "Tool",
            };
            let text = match &m.content {
                MessagePart::Text(t) => t.clone(),
                MessagePart::Parts(parts) => parts
                    .iter()
                    .filter_map(|p| match p {
                        crate::providers::ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            };
            format!("{role}: {text}")
        })
        .collect::<Vec<_>>()
        .join("\n");

    let summary_prompt = format!(
        "Summarize the following conversation in 3-5 concise sentences, \
         preserving key facts, decisions, and context. Output only the summary.\n\n{transcript}"
    );

    let req = LlmRequest {
        messages: vec![ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text(summary_prompt),
        }],
        system: Some("You are a concise summarizer. Produce short, dense summaries.".to_string()),
        max_tokens: Some(512),
        temperature: Some(0.2),
        tools: vec![],
        model: model.unwrap_or("").to_string(),
    };

    match provider.complete(&req).await {
        Ok(resp) => {
            let text: String = resp
                .content
                .iter()
                .filter_map(|b| match b {
                    crate::providers::ContentBlock::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("")
                .trim()
                .to_string();
            if text.is_empty() { None } else { Some(text) }
        }
        Err(e) => {
            warn!(error = %e, "context_policy: summarization LLM call failed, skipping");
            None
        }
    }
}
