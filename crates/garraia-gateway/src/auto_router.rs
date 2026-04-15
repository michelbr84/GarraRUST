//! GAR-227: Micro-LLM Auto Mode Router
//!
//! Automatically selects an [`AgentMode`] for an incoming message using a
//! two-stage pipeline:
//!
//! 1. **Heuristic** — Fast keyword-based classification. Returns a mode when
//!    the message clearly belongs to one category, or `None` when ambiguous.
//! 2. **LLM classify** — When the heuristic is ambiguous and
//!    `agent.auto_router_llm_enabled = true`, a single short LLM request
//!    (≤5 output tokens, 5 s timeout) classifies the intent.
//!
//! Falls back to `None` (no auto-mode override) if both stages fail or the
//! feature flag is disabled.

use std::time::Duration;

use garraia_agents::{AgentMode, AgentRuntime, ChatMessage, ChatRole, LlmRequest, MessagePart};
use tracing::{debug, warn};

// ── Public entry point ────────────────────────────────────────────────────────

/// Attempt to auto-classify the user's message into an [`AgentMode`].
///
/// Returns `None` if no mode could be determined (caller keeps the current mode
/// or uses the default). Never panics; all errors are logged and swallowed.
pub async fn auto_classify(
    text: &str,
    llm_enabled: bool,
    model_override: Option<&str>,
    runtime: Option<&AgentRuntime>,
) -> Option<AgentMode> {
    let heuristic = classify_heuristic(text);

    if heuristic.is_some() {
        debug!(mode = ?heuristic, "auto_router: heuristic match");
        return heuristic;
    }

    // Heuristic was ambiguous — try LLM if enabled and a runtime is available.
    if llm_enabled && let Some(rt) = runtime {
        let mode = classify_with_llm(text, rt, model_override).await;
        if mode.is_some() {
            debug!(mode = ?mode, "auto_router: LLM classify match");
            return mode;
        }
    }

    None
}

// ── Heuristic classifier ──────────────────────────────────────────────────────

/// Keyword-based mode classification.
///
/// Returns a clear mode when the message strongly signals a single intent, or
/// `None` when ambiguous (e.g., "how do I fix this code?").
fn classify_heuristic(text: &str) -> Option<AgentMode> {
    let lower = text.to_lowercase();
    let t = lower.as_str();

    // Score each mode
    let mut scores: Vec<(AgentMode, u8)> = vec![
        (AgentMode::Code, score_code(t)),
        (AgentMode::Debug, score_debug(t)),
        (AgentMode::Review, score_review(t)),
        (AgentMode::Search, score_search(t)),
        (AgentMode::Architect, score_architect(t)),
        (AgentMode::Ask, score_ask(t)),
    ];

    scores.sort_by(|a, b| b.1.cmp(&a.1));
    let (winner_mode, winner_score) = scores[0];
    let runner_score = scores[1].1;

    // Require a minimum score AND a clear lead to avoid ambiguous results.
    if winner_score >= 2 && winner_score >= runner_score + 2 {
        Some(winner_mode)
    } else {
        None
    }
}

fn score_code(t: &str) -> u8 {
    let keywords = [
        "implement",
        "write a function",
        "create a class",
        "fix the bug",
        "add feature",
        "refactor",
        "unit test",
        "write test",
        "add test",
        "compile error",
        "syntax error",
        "runtime error",
        "stack trace",
        "write code",
        "code for",
        "function that",
        "method that",
    ];
    keywords.iter().filter(|&&k| t.contains(k)).count().min(4) as u8
}

fn score_debug(t: &str) -> u8 {
    let keywords = [
        "why does",
        "why is",
        "not working",
        "broken",
        "crash",
        "panic",
        "exception",
        "traceback",
        "diagnose",
        "why my",
        "fails with",
        "error when",
        "bug in",
        "undefined",
        "null pointer",
    ];
    keywords.iter().filter(|&&k| t.contains(k)).count().min(4) as u8
}

fn score_review(t: &str) -> u8 {
    let keywords = [
        "review",
        "check my",
        "look at this",
        "feedback on",
        "audit",
        "is this correct",
        "improve this",
        "what's wrong with",
        "critique",
        "assess",
        "evaluate",
    ];
    keywords.iter().filter(|&&k| t.contains(k)).count().min(4) as u8
}

fn score_search(t: &str) -> u8 {
    let keywords = [
        "find ",
        "search for",
        "where is",
        "locate",
        "grep",
        "look for",
        "which file",
        "list all",
        "show me all",
    ];
    keywords.iter().filter(|&&k| t.contains(k)).count().min(4) as u8
}

fn score_architect(t: &str) -> u8 {
    let keywords = [
        "design",
        "architecture",
        "plan",
        "how should i structure",
        "best way to",
        "approach for",
        "system design",
        "data model",
        "schema for",
        "diagram",
    ];
    keywords.iter().filter(|&&k| t.contains(k)).count().min(4) as u8
}

fn score_ask(t: &str) -> u8 {
    let keywords = [
        "what is",
        "explain",
        "tell me",
        "how does",
        "what does",
        "describe",
        "difference between",
        "compare",
        "definition of",
        "what are",
    ];
    keywords.iter().filter(|&&k| t.contains(k)).count().min(4) as u8
}

// ── LLM classifier ────────────────────────────────────────────────────────────

const ROUTER_SYSTEM: &str = "\
You are a routing classifier. Read the user message and output exactly ONE word \
from the following list that best describes the user's intent:\n\
code, debug, review, search, architect, ask\n\
Output only the single word. No punctuation, no explanation.";

const ROUTER_MAX_TOKENS: u32 = 5;
const ROUTER_TIMEOUT_SECS: u64 = 5;

/// Make a single short LLM call to classify the user's intent.
async fn classify_with_llm(
    text: &str,
    runtime: &AgentRuntime,
    model_override: Option<&str>,
) -> Option<AgentMode> {
    let provider = runtime.default_provider()?;

    // Use only the first 400 chars to keep the classify call cheap.
    let snippet = &text[..text.len().min(400)];

    let model = model_override
        .map(|m| m.to_string())
        .unwrap_or_else(|| provider.provider_id().to_string());

    let request = LlmRequest {
        model,
        messages: vec![ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text(snippet.to_string()),
        }],
        system: Some(ROUTER_SYSTEM.to_string()),
        max_tokens: Some(ROUTER_MAX_TOKENS),
        temperature: Some(0.0),
        tools: vec![],
    };

    let result = tokio::time::timeout(
        Duration::from_secs(ROUTER_TIMEOUT_SECS),
        provider.complete(&request),
    )
    .await;

    match result {
        Ok(Ok(response)) => {
            let raw = response
                .content
                .iter()
                .filter_map(|b| {
                    if let garraia_agents::ContentBlock::Text { text } = b {
                        Some(text.trim().to_lowercase())
                    } else {
                        None
                    }
                })
                .next()
                .unwrap_or_default();

            // Extract the first word in case the model added extra text.
            let word = raw.split_whitespace().next().unwrap_or("");
            let mode = AgentMode::from_str(word);
            if mode.is_none() {
                warn!(raw = %raw, "auto_router: LLM returned unexpected mode token");
            }
            mode
        }
        Ok(Err(e)) => {
            warn!(error = %e, "auto_router: LLM classify call failed");
            None
        }
        Err(_) => {
            warn!("auto_router: LLM classify call timed out after {ROUTER_TIMEOUT_SECS}s");
            None
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heuristic_code() {
        // "implement" + "function that" = 2 → clear code win
        assert_eq!(
            classify_heuristic("implement a function that parses JSON and returns a struct"),
            Some(AgentMode::Code)
        );
        // "implement" + "unit test" = 2
        assert_eq!(
            classify_heuristic("implement the parser and add unit test coverage"),
            Some(AgentMode::Code)
        );
    }

    #[test]
    fn heuristic_debug() {
        // "why does" + "not working" = 2
        assert_eq!(
            classify_heuristic("why does this crash? the server is not working"),
            Some(AgentMode::Debug)
        );
        // "not working" + "exception" = 2
        assert_eq!(
            classify_heuristic("not working, fails with null pointer exception at runtime"),
            Some(AgentMode::Debug)
        );
    }

    #[test]
    fn heuristic_review() {
        // "review" + "feedback on" = 2
        assert_eq!(
            classify_heuristic("can you review this code and give feedback on it"),
            Some(AgentMode::Review)
        );
    }

    #[test]
    fn heuristic_ask() {
        // "what is" + "difference between" = 2
        assert_eq!(
            classify_heuristic("what is the difference between async and sync rust"),
            Some(AgentMode::Ask)
        );
    }

    #[test]
    fn heuristic_ambiguous_returns_none() {
        // Short or generic messages should not match strongly
        assert_eq!(classify_heuristic("hello"), None);
        assert_eq!(classify_heuristic("ok"), None);
    }
}
