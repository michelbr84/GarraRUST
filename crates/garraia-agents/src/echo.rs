//! Deterministic, feature-gated echo LLM provider used exclusively by
//! development and CI smoke tests.
//!
//! This module is only compiled when the `dev-echo-provider` feature is
//! enabled on `garraia-agents` (default OFF). It is never linked into
//! production release builds.

use async_trait::async_trait;
use garraia_common::Result;

use crate::providers::{
    ChatRole, ContentBlock, LlmProvider, LlmRequest, LlmResponse, MessagePart, Usage,
};

const DEFAULT_MODEL: &str = "echo-stub";
const ECHO_PREFIX: &str = "[echo] ";

/// Deterministic LLM provider that replies with `[echo] <last-user-text>`.
///
/// Stateless, zero external dependencies, never logs prompt content.
#[derive(Debug, Clone)]
pub struct EchoProvider {
    model: String,
}

impl EchoProvider {
    /// Build a new echo provider. `model` defaults to `"echo-stub"` when
    /// `None` — same label the provider advertises via `configured_model`.
    pub fn new(model: Option<String>) -> Self {
        Self {
            model: model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
        }
    }

    /// Extract the flattened text of the last `ChatRole::User` message in
    /// the request. `MessagePart::Parts` blocks are joined with spaces;
    /// non-text content blocks are skipped. Returns an empty string when
    /// no user message is present.
    fn last_user_text(request: &LlmRequest) -> String {
        request
            .messages
            .iter()
            .rev()
            .find(|msg| matches!(msg.role, ChatRole::User))
            .map(|msg| flatten_message(&msg.content))
            .unwrap_or_default()
    }
}

fn flatten_message(content: &MessagePart) -> String {
    match content {
        MessagePart::Text(s) => s.clone(),
        MessagePart::Parts(parts) => {
            // Join adjacent text blocks with a single space so the flattened
            // string matches the "joined with spaces" contract documented on
            // `EchoProvider::last_user_text`. Non-text blocks (Image,
            // ToolUse, ToolResult) are skipped deliberately — the echo
            // provider is a text-only stub.
            let mut out = String::new();
            for block in parts {
                if let ContentBlock::Text { text } = block {
                    if !out.is_empty() {
                        out.push(' ');
                    }
                    out.push_str(text);
                }
            }
            out
        }
    }
}

#[async_trait]
impl LlmProvider for EchoProvider {
    fn provider_id(&self) -> &str {
        "echo"
    }

    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        let mut text = String::with_capacity(ECHO_PREFIX.len() + 32);
        text.push_str(ECHO_PREFIX);
        text.push_str(&Self::last_user_text(request));

        Ok(LlmResponse {
            content: vec![ContentBlock::Text { text }],
            model: self.model.clone(),
            usage: Some(Usage {
                input_tokens: 0,
                output_tokens: 0,
            }),
            stop_reason: Some("stop".to_string()),
        })
    }

    fn configured_model(&self) -> Option<&str> {
        Some(&self.model)
    }

    async fn available_models(&self) -> Result<Vec<String>> {
        Ok(vec![self.model.clone()])
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{
        ChatMessage, ChatRole, ContentBlock, LlmProvider, LlmRequest, MessagePart,
    };

    fn user_text_request(text: &str) -> LlmRequest {
        LlmRequest {
            model: "auto".to_string(),
            messages: vec![ChatMessage {
                role: ChatRole::User,
                content: MessagePart::Text(text.to_string()),
            }],
            system: None,
            max_tokens: None,
            temperature: None,
            tools: Vec::new(),
        }
    }

    #[tokio::test]
    async fn echoes_last_user_text_message() {
        let provider = EchoProvider::new(None);

        let response = provider
            .complete(&user_text_request("Olá, este é um teste E2E."))
            .await
            .expect("echo provider should always succeed");

        assert_eq!(response.content.len(), 1);
        match &response.content[0] {
            ContentBlock::Text { text } => {
                assert_eq!(text, "[echo] Olá, este é um teste E2E.");
            }
            other => panic!("expected Text content block, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn flattens_parts_content_blocks() {
        let provider = EchoProvider::new(None);

        let request = LlmRequest {
            model: "auto".to_string(),
            messages: vec![ChatMessage {
                role: ChatRole::User,
                content: MessagePart::Parts(vec![
                    ContentBlock::Text {
                        text: "primeiro".to_string(),
                    },
                    ContentBlock::Text {
                        text: "segundo".to_string(),
                    },
                ]),
            }],
            system: None,
            max_tokens: None,
            temperature: None,
            tools: Vec::new(),
        };

        let response = provider
            .complete(&request)
            .await
            .expect("echo provider should always succeed");

        match &response.content[0] {
            ContentBlock::Text { text } => {
                // Single space inserted by `flatten_message` between blocks —
                // asserted without leading/trailing whitespace in the
                // fixtures so the test validates the join contract, not an
                // accident of how the fixture was written.
                assert_eq!(text, "[echo] primeiro segundo");
            }
            other => panic!("expected Text content block, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn picks_last_user_turn_when_history_has_assistant_replies() {
        let provider = EchoProvider::new(None);

        let request = LlmRequest {
            model: "auto".to_string(),
            messages: vec![
                ChatMessage {
                    role: ChatRole::User,
                    content: MessagePart::Text("primeira pergunta".to_string()),
                },
                ChatMessage {
                    role: ChatRole::Assistant,
                    content: MessagePart::Text("[echo] primeira pergunta".to_string()),
                },
                ChatMessage {
                    role: ChatRole::User,
                    content: MessagePart::Text("qual foi a sua resposta anterior?".to_string()),
                },
            ],
            system: None,
            max_tokens: None,
            temperature: None,
            tools: Vec::new(),
        };

        let response = provider
            .complete(&request)
            .await
            .expect("echo provider should always succeed");

        match &response.content[0] {
            ContentBlock::Text { text } => {
                assert_eq!(text, "[echo] qual foi a sua resposta anterior?");
            }
            other => panic!("expected Text content block, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn health_check_is_always_ok() {
        let provider = EchoProvider::new(None);
        let healthy = provider
            .health_check()
            .await
            .expect("echo provider health check must not fail");
        assert!(healthy);
    }

    #[test]
    fn provider_id_is_stable() {
        let provider = EchoProvider::new(None);
        assert_eq!(provider.provider_id(), "echo");
    }

    #[test]
    fn configured_model_defaults_when_unset() {
        let provider = EchoProvider::new(None);
        assert_eq!(provider.configured_model(), Some("echo-stub"));
    }

    #[test]
    fn configured_model_preserves_override() {
        let provider = EchoProvider::new(Some("ci-echo-v1".to_string()));
        assert_eq!(provider.configured_model(), Some("ci-echo-v1"));
    }
}
