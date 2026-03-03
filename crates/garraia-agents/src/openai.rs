use std::pin::Pin;

use async_trait::async_trait;
use futures::StreamExt;
use futures::stream::Stream;
use garraia_common::{Error, Result};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, instrument};

use crate::providers::{
    ChatMessage, ChatRole, ContentBlock, LlmProvider, LlmRequest, LlmResponse, MessagePart,
    StreamEvent, Usage,
};

const DEFAULT_MODEL: &str = "gpt-4o";
const DEFAULT_BASE_URL: &str = "https://api.openai.com";

/// OpenAI Chat Completions provider.
/// Also works with OpenAI-compatible APIs (Azure, local models) via `base_url`.
/// For OpenRouter, it automatically adds the required HTTP-Referer header.
pub struct OpenAiProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
    name: Option<String>,
    is_openrouter: bool,
}

impl OpenAiProvider {
    pub fn new(
        api_key: impl Into<String>,
        model: Option<String>,
        base_url: Option<String>,
    ) -> Self {
        let base_url_str = base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string());
        let is_openrouter = base_url_str.contains("openrouter.ai");
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
            model: model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            base_url: base_url_str,
            name: None,
            is_openrouter,
        }
    }

    pub fn with_client(mut self, client: reqwest::Client) -> Self {
        self.client = client;
        self
    }

    /// Override the provider ID returned by `provider_id()`.
    /// Useful for OpenAI-compatible APIs (e.g. Sansa) that should register
    /// under a distinct name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        let name_str = name.into();
        // Also detect OpenRouter from the name
        if name_str.to_lowercase().contains("openrouter") {
            self.is_openrouter = true;
        }
        self.name = Some(name_str);
        self
    }

    fn endpoint(&self) -> String {
        // Remove /v1 or /v1/ from end of base_url if present, to avoid duplication
        let base = self.base_url.trim_end_matches('/');
        // Remove /v1 from end if present (e.g., api/v1 -> api)
        let base = if base.ends_with("/v1") {
            &base[..base.len() - 3]
        } else {
            base
        };
        format!("{}/v1/chat/completions", base)
    }

    /// List models available from OpenRouter API
    /// Returns a curated list of popular models to avoid overwhelming the UI
    async fn list_models(&self) -> Result<Vec<String>> {
        if !self.is_openrouter {
            return Ok(Vec::new());
        }

        let base = self.base_url.trim_end_matches('/');
        let url = format!("{}/models", base);

        tracing::info!("Fetching models from OpenRouter: {}", url);

        let mut req = self
            .client
            .get(&url)
            .header("authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json");

        if self.is_openrouter {
            req = req
                .header("HTTP-Referer", "https://garraia.org")
                .header("X-Title", "GarraIA");
        }

        let response = req
            .send()
            .await
            .map_err(|e| Error::Agent(format!("failed to list models: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            tracing::warn!("Failed to list models: status={}, body={}", status, body);
            return Err(Error::Agent(format!(
                "failed to list models: status={}, body={}",
                status, body
            )));
        }

        #[derive(Deserialize)]
        struct OpenRouterModelsResponse {
            data: Vec<OpenRouterModel>,
        }

        #[derive(Deserialize)]
        struct OpenRouterModel {
            id: String,
        }

        let models_response: OpenRouterModelsResponse = response
            .json()
            .await
            .map_err(|e| Error::Agent(format!("failed to parse models response: {e}")))?;

        let all_models: Vec<String> = models_response.data.into_iter().map(|m| m.id).collect();

        tracing::info!("OpenRouter total models: {}", all_models.len());

        // Return popular models for UI display
        let popular_models = vec![
            "openai/gpt-4o".to_string(),
            "openai/gpt-4o-mini".to_string(),
            "openai/gpt-4".to_string(),
            "openai/gpt-3.5-turbo".to_string(),
            "anthropic/claude-sonnet-4.5".to_string(),
            "anthropic/claude-opus-4.5".to_string(),
            "anthropic/claude-haiku-4.5".to_string(),
            "google/gemini-2.5-pro".to_string(),
            "google/gemini-2.5-flash".to_string(),
            "meta-llama/llama-3.1-70b-instruct".to_string(),
            "meta-llama/llama-3.3-70b-instruct".to_string(),
            "deepseek/deepseek-r1".to_string(),
            "mistralai/mistral-large".to_string(),
            "qwen/qwen-plus".to_string(),
            "moonshotai/kimi-k2".to_string(),
            "openrouter/auto".to_string(),
        ];

        // Filter to only include models that exist in the available models
        let models: Vec<String> = popular_models
            .into_iter()
            .filter(|m| all_models.contains(m))
            .collect();

        tracing::info!("OpenRouter popular models count: {}", models.len());

        Ok(models)
    }

    fn build_request(&self, request: &LlmRequest) -> OpenAiRequest {
        let model = if request.model.is_empty() {
            self.model.clone()
        } else {
            request.model.clone()
        };

        let mut messages: Vec<OpenAiMessage> = Vec::new();

        // System message from the request
        if let Some(system) = &request.system {
            messages.push(OpenAiMessage {
                role: "system".to_string(),
                content: Some(system.clone()),
                tool_calls: None,
                tool_call_id: None,
            });
        }

        // Convert chat messages
        for msg in &request.messages {
            match (&msg.role, &msg.content) {
                // User messages with tool results expand to multiple "tool" messages
                (ChatRole::User, MessagePart::Parts(blocks))
                    if blocks
                        .iter()
                        .any(|b| matches!(b, ContentBlock::ToolResult { .. })) =>
                {
                    for block in blocks {
                        if let ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                        } = block
                        {
                            messages.push(OpenAiMessage {
                                role: "tool".to_string(),
                                content: Some(content.clone()),
                                tool_calls: None,
                                tool_call_id: Some(tool_use_id.clone()),
                            });
                        }
                    }
                }
                // Assistant messages with tool_use blocks
                (ChatRole::Assistant, MessagePart::Parts(blocks)) => {
                    let text_content: String = blocks
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");

                    let tool_calls: Vec<OpenAiToolCall> = blocks
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::ToolUse { id, name, input } => Some(OpenAiToolCall {
                                id: id.clone(),
                                r#type: "function".to_string(),
                                function: OpenAiFunctionCall {
                                    name: name.clone(),
                                    arguments: serde_json::to_string(input).unwrap_or_default(),
                                },
                            }),
                            _ => None,
                        })
                        .collect();

                    messages.push(OpenAiMessage {
                        role: "assistant".to_string(),
                        content: if text_content.is_empty() {
                            None
                        } else {
                            Some(text_content)
                        },
                        tool_calls: if tool_calls.is_empty() {
                            None
                        } else {
                            Some(tool_calls)
                        },
                        tool_call_id: None,
                    });
                }
                // Simple text messages
                (role, MessagePart::Text(text)) => {
                    let role_str = match role {
                        ChatRole::User => "user",
                        ChatRole::Assistant => "assistant",
                        ChatRole::System => "system",
                        ChatRole::Tool => "tool",
                    };
                    messages.push(OpenAiMessage {
                        role: role_str.to_string(),
                        content: Some(text.clone()),
                        tool_calls: None,
                        tool_call_id: None,
                    });
                }
                // User messages with non-tool-result parts
                (role, MessagePart::Parts(blocks)) => {
                    let text: String = blocks
                        .iter()
                        .filter_map(|b| match b {
                            ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");

                    let role_str = match role {
                        ChatRole::User => "user",
                        ChatRole::Assistant => "assistant",
                        ChatRole::System => "system",
                        ChatRole::Tool => "tool",
                    };
                    messages.push(OpenAiMessage {
                        role: role_str.to_string(),
                        content: if text.is_empty() { None } else { Some(text) },
                        tool_calls: None,
                        tool_call_id: None,
                    });
                }
            }
        }

        let tools: Vec<OpenAiTool> = request
            .tools
            .iter()
            .map(|t| OpenAiTool {
                r#type: "function".to_string(),
                function: OpenAiFunction {
                    name: t.name.clone(),
                    description: t.description.clone(),
                    parameters: t.input_schema.clone(),
                },
            })
            .collect();

        OpenAiRequest {
            model,
            messages,
            max_tokens: request.max_tokens,
            temperature: request.temperature,
            tools: if tools.is_empty() { None } else { Some(tools) },
        }
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    fn provider_id(&self) -> &str {
        self.name.as_deref().unwrap_or("openai")
    }

    fn configured_model(&self) -> Option<&str> {
        Some(&self.model)
    }

    #[instrument(skip(self, request), fields(model))]
    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        let body = self.build_request(request);

        tracing::Span::current().record("model", body.model.as_str());
        debug!("openai request: model={}", body.model);

        let mut req = self
            .client
            .post(self.endpoint())
            .header("authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json");

        // Debug log the request details
        tracing::debug!(
            "openai request: endpoint={}, is_openrouter={}, model={}, provider_id={}",
            self.endpoint(),
            self.is_openrouter,
            body.model,
            self.provider_id()
        );

        // OpenRouter requires Referer header
        if self.is_openrouter {
            req = req
                .header("HTTP-Referer", "https://garraia.org")
                .header("X-Title", "GarraIA");
        }

        let response = req
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Agent(format!("openai request failed: {e}")))?;

        let status = response.status();

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            // Log more details for debugging
            tracing::warn!(
                "openai API error: status={}, body_truncated={}, endpoint={}",
                status,
                &body[..body.len().min(500)],
                self.endpoint()
            );
            return Err(Error::Agent(format!(
                "openai API error: status={status}, body={body}"
            )));
        }

        // Get status code before consuming the response
        let status_code = status;

        // Check Content-Type to handle SSE vs JSON responses
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let body_bytes = response
            .bytes()
            .await
            .map_err(|e| Error::Agent(format!("failed to read response body: {e}")))?;

        let body_str = String::from_utf8_lossy(&body_bytes);

        // If Content-Type indicates SSE but we expected JSON, this might be an error
        // Some providers return text/event-stream even for non-streaming requests
        // OpenRouter can also send comment payloads like ": OPENROUTER PROCESSING"
        if content_type.contains("text/event-stream") {
            // First check for comment payloads (lines starting with colon)
            let has_only_comments = body_str.lines()
                .all(|line| line.trim().is_empty() || line.trim().starts_with(':'));
            
            if has_only_comments {
                // All lines are comments - this is likely a processing message, not an error
                tracing::debug!(
                    "openai response contains only SSE comments, treating as empty response: endpoint={}",
                    self.endpoint()
                );
            } else if body_str.starts_with("data: ") {
                // Parse SSE to extract potential error
                for line in body_str.lines() {
                    // Skip comment lines (OpenRouter can send ": OPENROUTER PROCESSING")
                    let trimmed = line.trim();
                    if trimmed.starts_with(':') {
                        continue;
                    }
                    
                    if let Some(data) = trimmed.strip_prefix("data: ") {
                        if !data.is_empty() && data != "[DONE]" {
                            // Try to extract error message from JSON
                            if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(data) {
                                let err = json_val.get("error")
                                    .or_else(|| json_val.get("message"))
                                    .and_then(|e| e.as_str())
                                    .map(|s| s.to_string());
                                if let Some(err_msg) = err {
                                    tracing::warn!(
                                        "openai API returned SSE error: status={}, error={}, endpoint={}",
                                        status_code,
                                        &err_msg[..err_msg.len().min(200)],
                                        self.endpoint()
                                    );
                                    return Err(Error::Agent(format!(
                                        "openai API error (SSE): status={}, error={}",
                                        status_code, err_msg
                                    )));
                                }
                            }
                        }
                    }
                }
            }
        }

        // Some providers (e.g. OpenRouter) return 200 OK with Content-Type:
        // application/json but send an error object instead of a completion.
        // The body may also have leading whitespace/newlines (leftover SSE
        // keep-alive events). Detect this before attempting full parse.
        let trimmed_body = body_str.trim();
        if let Ok(json_val) = serde_json::from_str::<serde_json::Value>(trimmed_body) {
            if let Some(error) = json_val.get("error") {
                let msg = error
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown error");
                let code = error
                    .get("code")
                    .and_then(|c| c.as_u64())
                    .map(|c| format!(", code={c}"))
                    .unwrap_or_default();
                tracing::warn!(
                    "openai API returned error body with 200 status: {}{}, endpoint={}",
                    msg,
                    code,
                    self.endpoint()
                );
                return Err(Error::Agent(format!("openai API error: {msg}")));
            }
        }

        // Try to parse as JSON, with detailed error logging
        match serde_json::from_slice::<OpenAiResponse>(&body_bytes) {
            Ok(resp) => Ok(from_openai_response(resp)),
            Err(e) => {
                let truncated_body = &body_str[..body_str.len().min(1000)];
                tracing::warn!(
                    "failed to parse openai response: {}\nstatus={}, content-type={}, body_preview={}",
                    e,
                    status_code,
                    content_type,
                    truncated_body
                );
                Err(Error::Agent(format!(
                    "failed to parse openai response: {}",
                    e
                )))
            }
        }
    }

    #[instrument(skip(self, request), fields(model))]
    async fn stream_complete(
        &self,
        request: &LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        let body = self.build_request(request);

        tracing::Span::current().record("model", body.model.as_str());
        debug!("openai stream request: model={}", body.model);

        // Inject stream=true into the serialized request
        let mut body_value = serde_json::to_value(&body)
            .map_err(|e| Error::Agent(format!("failed to serialize request: {e}")))?;
        body_value["stream"] = serde_json::Value::Bool(true);

        let mut req = self
            .client
            .post(self.endpoint())
            .header("authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json");

        // OpenRouter requires Referer header
        if self.is_openrouter {
            req = req
                .header("HTTP-Referer", "https://garraia.org")
                .header("X-Title", "GarraIA");
        }

        let response = req
            .json(&body_value)
            .send()
            .await
            .map_err(|e| Error::Agent(format!("openai stream request failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::Agent(format!(
                "openai API error: status={status}, body={body}"
            )));
        }

        let byte_stream: Pin<
            Box<dyn Stream<Item = std::result::Result<bytes::Bytes, reqwest::Error>> + Send>,
        > = Box::pin(response.bytes_stream());

        let event_stream = futures::stream::unfold(
            (byte_stream, String::new()),
            |(mut stream, mut buffer)| async move {
                loop {
                    // Try to consume a complete SSE event from the buffer
                    if let Some(pos) = buffer.find("\n\n") {
                        let event_str = buffer[..pos].to_string();
                        buffer = buffer[pos + 2..].to_string();

                        // Extract the data: line
                        // Skip comment lines (OpenRouter can send ": OPENROUTER PROCESSING")
                        let mut data_line = None;
                        for line in event_str.lines() {
                            let trimmed = line.trim();
                            // Skip empty lines and comment lines (starting with colon)
                            if trimmed.is_empty() || trimmed.starts_with(':') {
                                continue;
                            }
                            if let Some(d) = trimmed.strip_prefix("data: ") {
                                data_line = Some(d.to_string());
                            }
                        }

                        if let Some(data) = data_line {
                            // OpenAI sends "data: [DONE]" as the final event
                            if data == "[DONE]" {
                                return Some((Ok(StreamEvent::MessageStop), (stream, buffer)));
                            }

                            if let Some(events) = parse_stream_chunk(&data) {
                                // Yield the first event; remaining events get pushed back
                                // into the buffer as synthetic SSE blocks so the loop
                                // picks them up on the next iteration.
                                let mut iter = events.into_iter();
                                let first = iter.next().unwrap();
                                for extra in iter.rev() {
                                    // Push synthetic SSE event back into buffer
                                    let json =
                                        serde_json::to_string(&SyntheticEvent(extra)).unwrap();
                                    buffer = format!("data: {json}\n\n{buffer}");
                                }
                                return Some((Ok(first), (stream, buffer)));
                            }
                            continue;
                        }
                        continue;
                    }

                    // Need more data from the byte stream
                    match stream.next().await {
                        Some(Ok(bytes)) => {
                            buffer.push_str(&String::from_utf8_lossy(&bytes));
                        }
                        Some(Err(e)) => {
                            return Some((
                                Err(Error::Agent(format!("stream read error: {e}"))),
                                (stream, buffer),
                            ));
                        }
                        None => return None,
                    }
                }
            },
        );

        Ok(Box::pin(event_stream))
    }

    async fn health_check(&self) -> Result<bool> {
        let request = LlmRequest {
            model: self.model.clone(),
            messages: vec![ChatMessage {
                role: ChatRole::User,
                content: MessagePart::Text("ping".to_string()),
            }],
            system: None,
            max_tokens: Some(1),
            temperature: None,
            tools: vec![],
        };

        match self.complete(&request).await {
            Ok(_) => Ok(true),
            Err(e) => {
                info!("openai health check failed: {e}");
                Ok(false)
            }
        }
    }

    async fn available_models(&self) -> Result<Vec<String>> {
        if self.is_openrouter {
            self.list_models().await
        } else {
            Ok(Vec::new())
        }
    }
}

// --- OpenAI Wire Types (private) ---

#[derive(Debug, Serialize)]
struct OpenAiRequest {
    model: String,
    messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAiTool>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiToolCall {
    id: String,
    r#type: String,
    function: OpenAiFunctionCall,
}

#[derive(Debug, Serialize, Deserialize)]
struct OpenAiFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Serialize)]
struct OpenAiTool {
    r#type: String,
    function: OpenAiFunction,
}

#[derive(Debug, Serialize)]
struct OpenAiFunction {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct OpenAiResponse {
    choices: Vec<OpenAiChoice>,
    model: String,
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

// --- OpenAI Streaming Wire Types (private) ---

#[derive(Debug, Deserialize)]
struct OpenAiStreamChunk {
    choices: Vec<OpenAiStreamChoice>,
    #[allow(dead_code)]
    model: Option<String>,
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamChoice {
    #[allow(dead_code)]
    index: usize,
    delta: OpenAiStreamDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamDelta {
    #[allow(dead_code)]
    role: Option<String>,
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiStreamToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamToolCallDelta {
    index: usize,
    id: Option<String>,
    #[allow(dead_code)]
    r#type: Option<String>,
    function: Option<OpenAiStreamFunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct OpenAiStreamFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

/// Wrapper to allow serializing a StreamEvent back into a synthetic SSE data line
/// when a single chunk produces multiple events.
struct SyntheticEvent(StreamEvent);

impl serde::Serialize for SyntheticEvent {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        match &self.0 {
            StreamEvent::TextDelta(text) => {
                map.serialize_entry("_synthetic", "text_delta")?;
                map.serialize_entry("text", text)?;
            }
            StreamEvent::ToolUseStart { index, id, name } => {
                map.serialize_entry("_synthetic", "tool_use_start")?;
                map.serialize_entry("index", index)?;
                map.serialize_entry("id", id)?;
                map.serialize_entry("name", name)?;
            }
            StreamEvent::InputJsonDelta(json) => {
                map.serialize_entry("_synthetic", "input_json_delta")?;
                map.serialize_entry("json", json)?;
            }
            StreamEvent::ContentBlockStop { index } => {
                map.serialize_entry("_synthetic", "content_block_stop")?;
                map.serialize_entry("index", index)?;
            }
            StreamEvent::MessageDelta { stop_reason, .. } => {
                map.serialize_entry("_synthetic", "message_delta")?;
                map.serialize_entry("stop_reason", stop_reason)?;
            }
            StreamEvent::MessageStop => {
                map.serialize_entry("_synthetic", "message_stop")?;
            }
        }
        map.end()
    }
}

fn parse_synthetic_event(value: &serde_json::Value) -> Option<StreamEvent> {
    let kind = value.get("_synthetic")?.as_str()?;
    match kind {
        "text_delta" => Some(StreamEvent::TextDelta(
            value.get("text")?.as_str()?.to_string(),
        )),
        "tool_use_start" => Some(StreamEvent::ToolUseStart {
            index: value.get("index")?.as_u64()? as usize,
            id: value.get("id")?.as_str()?.to_string(),
            name: value.get("name")?.as_str()?.to_string(),
        }),
        "input_json_delta" => Some(StreamEvent::InputJsonDelta(
            value.get("json")?.as_str()?.to_string(),
        )),
        "content_block_stop" => Some(StreamEvent::ContentBlockStop {
            index: value.get("index")?.as_u64()? as usize,
        }),
        "message_delta" => Some(StreamEvent::MessageDelta {
            stop_reason: value
                .get("stop_reason")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            usage: None,
        }),
        "message_stop" => Some(StreamEvent::MessageStop),
        _ => None,
    }
}

/// Parse an OpenAI streaming chunk into one or more StreamEvents.
fn parse_stream_chunk(data: &str) -> Option<Vec<StreamEvent>> {
    let value: serde_json::Value = serde_json::from_str(data).ok()?;

    // Handle synthetic events we pushed back into the buffer
    if value.get("_synthetic").is_some() {
        return parse_synthetic_event(&value).map(|e| vec![e]);
    }

    let chunk: OpenAiStreamChunk = serde_json::from_value(value).ok()?;

    let choice = chunk.choices.first()?;
    let mut events = Vec::new();

    // Text content delta
    if let Some(content) = &choice.delta.content
        && !content.is_empty()
    {
        events.push(StreamEvent::TextDelta(content.clone()));
    }

    // Tool call deltas
    if let Some(tool_calls) = &choice.delta.tool_calls {
        for tc in tool_calls {
            // First chunk for a tool call includes id and name
            if let Some(id) = &tc.id {
                let name = tc
                    .function
                    .as_ref()
                    .and_then(|f| f.name.as_ref())
                    .cloned()
                    .unwrap_or_default();
                events.push(StreamEvent::ToolUseStart {
                    index: tc.index,
                    id: id.clone(),
                    name,
                });
            }

            // Argument fragments
            if let Some(func) = &tc.function
                && let Some(args) = &func.arguments
                && !args.is_empty()
            {
                events.push(StreamEvent::InputJsonDelta(args.clone()));
            }
        }
    }

    // finish_reason signals end of generation
    if let Some(reason) = &choice.finish_reason {
        let stop_reason = match reason.as_str() {
            "stop" => "end_turn".to_string(),
            "tool_calls" => "tool_use".to_string(),
            other => other.to_string(),
        };

        let usage = chunk.usage.map(|u| Usage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
        });

        // OpenAI-compatible chat-completions streams signal tool calling via `finish_reason: "tool_calls"`,
        // but do not emit an explicit content-block stop event. Emit one so the runtime can flush the tool call.
        if reason.as_str() == "tool_calls" {
            events.push(StreamEvent::ContentBlockStop { index: 0 });
        }

        events.push(StreamEvent::MessageDelta {
            stop_reason: Some(stop_reason),
            usage,
        });
    }

    if events.is_empty() {
        None
    } else {
        Some(events)
    }
}

// --- Conversion ---

fn from_openai_response(response: OpenAiResponse) -> LlmResponse {
    let choice = response.choices.into_iter().next();

    let (content, stop_reason) = match choice {
        Some(c) => {
            let mut blocks = Vec::new();

            if let Some(text) = &c.message.content
                && !text.is_empty()
            {
                blocks.push(ContentBlock::Text { text: text.clone() });
            }

            if let Some(tool_calls) = c.message.tool_calls {
                for tc in tool_calls {
                    let input: serde_json::Value =
                        serde_json::from_str(&tc.function.arguments).unwrap_or_default();
                    blocks.push(ContentBlock::ToolUse {
                        id: tc.id,
                        name: tc.function.name,
                        input,
                    });
                }
            }

            // Map OpenAI finish_reason to Anthropic-style stop_reason
            let stop = c.finish_reason.map(|r| match r.as_str() {
                "stop" => "end_turn".to_string(),
                "tool_calls" => "tool_use".to_string(),
                other => other.to_string(),
            });

            (blocks, stop)
        }
        None => (vec![], None),
    };

    LlmResponse {
        content,
        model: response.model,
        usage: response.usage.map(|u| Usage {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
        }),
        stop_reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::ToolDefinition;

    #[test]
    fn builds_request_with_default_model() {
        let provider = OpenAiProvider::new("test-key", None, None);
        let request = LlmRequest {
            model: String::new(),
            messages: vec![ChatMessage {
                role: ChatRole::User,
                content: MessagePart::Text("hello".to_string()),
            }],
            system: Some("You are helpful".to_string()),
            max_tokens: Some(1024),
            temperature: None,
            tools: vec![],
        };

        let openai_req = provider.build_request(&request);
        assert_eq!(openai_req.model, DEFAULT_MODEL);
        // System message should be first
        assert_eq!(openai_req.messages[0].role, "system");
        assert_eq!(
            openai_req.messages[0].content,
            Some("You are helpful".to_string())
        );
        assert_eq!(openai_req.messages[1].role, "user");
        assert!(openai_req.tools.is_none());
    }

    #[test]
    fn serializes_request_correctly() {
        let req = OpenAiRequest {
            model: "gpt-4o".to_string(),
            messages: vec![OpenAiMessage {
                role: "user".to_string(),
                content: Some("Hello".to_string()),
                tool_calls: None,
                tool_call_id: None,
            }],
            max_tokens: Some(1024),
            temperature: None,
            tools: None,
        };

        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["model"], "gpt-4o");
        assert_eq!(json["max_tokens"], 1024);
        assert_eq!(json["messages"][0]["role"], "user");
        assert_eq!(json["messages"][0]["content"], "Hello");
        assert!(json.get("temperature").is_none());
        assert!(json.get("tools").is_none());
    }

    #[test]
    fn deserializes_text_response() {
        let json = r#"{
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Hello! How can I help?"
                },
                "finish_reason": "stop"
            }],
            "model": "gpt-4o",
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 20
            }
        }"#;

        let response: OpenAiResponse = serde_json::from_str(json).unwrap();
        let llm_response = from_openai_response(response);

        assert_eq!(llm_response.content.len(), 1);
        match &llm_response.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "Hello! How can I help?"),
            _ => panic!("expected text block"),
        }
        assert_eq!(llm_response.stop_reason, Some("end_turn".to_string()));
        assert_eq!(llm_response.usage.as_ref().unwrap().input_tokens, 10);
        assert_eq!(llm_response.usage.as_ref().unwrap().output_tokens, 20);
    }

    #[test]
    fn deserializes_tool_call_response() {
        let json = r#"{
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Let me check that.",
                    "tool_calls": [{
                        "id": "call_123",
                        "type": "function",
                        "function": {
                            "name": "bash",
                            "arguments": "{\"command\":\"echo hello\"}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "model": "gpt-4o",
            "usage": {"prompt_tokens": 50, "completion_tokens": 30}
        }"#;

        let response: OpenAiResponse = serde_json::from_str(json).unwrap();
        let llm_response = from_openai_response(response);

        assert_eq!(llm_response.content.len(), 2);
        assert_eq!(llm_response.stop_reason, Some("tool_use".to_string()));

        match &llm_response.content[1] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "call_123");
                assert_eq!(name, "bash");
                assert_eq!(input["command"], "echo hello");
            }
            _ => panic!("expected tool_use block"),
        }
    }

    #[test]
    fn converts_tool_result_to_tool_messages() {
        let provider = OpenAiProvider::new("test-key", None, None);
        let request = LlmRequest {
            model: String::new(),
            messages: vec![
                ChatMessage {
                    role: ChatRole::Assistant,
                    content: MessagePart::Parts(vec![ContentBlock::ToolUse {
                        id: "call_123".to_string(),
                        name: "bash".to_string(),
                        input: serde_json::json!({"command": "echo hi"}),
                    }]),
                },
                ChatMessage {
                    role: ChatRole::User,
                    content: MessagePart::Parts(vec![ContentBlock::ToolResult {
                        tool_use_id: "call_123".to_string(),
                        content: "hi\n".to_string(),
                    }]),
                },
            ],
            system: None,
            max_tokens: None,
            temperature: None,
            tools: vec![],
        };

        let openai_req = provider.build_request(&request);
        // Assistant message with tool_calls
        assert_eq!(openai_req.messages[0].role, "assistant");
        assert!(openai_req.messages[0].tool_calls.is_some());
        // Tool result as role=tool message
        assert_eq!(openai_req.messages[1].role, "tool");
        assert_eq!(
            openai_req.messages[1].tool_call_id,
            Some("call_123".to_string())
        );
        assert_eq!(openai_req.messages[1].content, Some("hi\n".to_string()));
    }

    #[test]
    fn endpoint_openrouter_no_duplication() {
        let provider = OpenAiProvider::new(
            "test-key",
            None,
            Some("https://openrouter.ai/api/v1".to_string()),
        );
        let endpoint = provider.endpoint();
        assert!(
            !endpoint.contains("/v1/v1/"),
            "endpoint should not have /v1/v1/ duplication: {}",
            endpoint
        );
        assert_eq!(endpoint, "https://openrouter.ai/api/v1/chat/completions");
    }

    #[test]
    fn endpoint_strips_trailing_slash() {
        let provider =
            OpenAiProvider::new("key", None, Some("https://api.example.com/".to_string()));
        assert_eq!(
            provider.endpoint(),
            "https://api.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn parses_text_stream_chunk() {
        let data = r#"{"id":"chatcmpl-abc","object":"chat.completion.chunk","model":"gpt-4o","choices":[{"index":0,"delta":{"role":"assistant","content":"Hello"},"finish_reason":null}]}"#;
        let events = parse_stream_chunk(data).unwrap();
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], StreamEvent::TextDelta(t) if t == "Hello"));
    }

    #[test]
    fn parses_tool_call_stream_chunks() {
        // First chunk: tool_use start with id and name
        let data1 = r#"{"id":"chatcmpl-abc","object":"chat.completion.chunk","model":"gpt-4o","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_123","type":"function","function":{"name":"bash","arguments":""}}]},"finish_reason":null}]}"#;
        let events1 = parse_stream_chunk(data1).unwrap();
        assert!(
            matches!(&events1[0], StreamEvent::ToolUseStart { id, name, .. } if id == "call_123" && name == "bash")
        );

        // Subsequent chunk: argument fragment
        let data2 = r#"{"id":"chatcmpl-abc","object":"chat.completion.chunk","model":"gpt-4o","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"cmd\":"}}]},"finish_reason":null}]}"#;
        let events2 = parse_stream_chunk(data2).unwrap();
        assert!(matches!(&events2[0], StreamEvent::InputJsonDelta(s) if s == r#"{"cmd":"#));
    }

    #[test]
    fn parses_finish_reason_stop() {
        let data = r#"{"id":"chatcmpl-abc","object":"chat.completion.chunk","model":"gpt-4o","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#;
        let events = parse_stream_chunk(data).unwrap();
        assert!(
            matches!(&events[0], StreamEvent::MessageDelta { stop_reason: Some(r), .. } if r == "end_turn")
        );
    }

    #[test]
    fn parses_finish_reason_tool_calls() {
        let data = r#"{"id":"chatcmpl-abc","object":"chat.completion.chunk","model":"gpt-4o","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#;
        let events = parse_stream_chunk(data).unwrap();
        assert!(
            matches!(&events[0], StreamEvent::ContentBlockStop { .. }),
            "expected a ContentBlockStop event to flush tool calls in streaming mode"
        );
        assert!(
            matches!(&events[1], StreamEvent::MessageDelta { stop_reason: Some(r), .. } if r == "tool_use")
        );
    }

    #[test]
    fn done_sentinel_returns_none() {
        // [DONE] is not valid JSON, so parse_stream_chunk returns None
        assert!(parse_stream_chunk("[DONE]").is_none());
    }

    #[test]
    fn request_includes_tools_when_provided() {
        let provider = OpenAiProvider::new("test-key", None, None);
        let request = LlmRequest {
            model: String::new(),
            messages: vec![],
            system: None,
            max_tokens: None,
            temperature: None,
            tools: vec![ToolDefinition {
                name: "bash".to_string(),
                description: "Run a command".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {"command": {"type": "string"}}
                }),
            }],
        };

        let openai_req = provider.build_request(&request);
        let tools = openai_req.tools.unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].function.name, "bash");
        assert_eq!(tools[0].r#type, "function");
    }

    #[test]
    fn error_body_json_is_detected_before_choices_parse() {
        // Reproduces the OpenRouter case: 200 OK but body is an error object,
        // possibly preceded by SSE keep-alive newlines.
        let bodies = [
            r#"{"error":{"message":"Internal Server Error","code":500}}"#,
            "\n\n\n{\"error\":{\"message\":\"rate limited\",\"code\":429}}",
            "   \n{\"error\":{\"message\":\"model not found\"}}  ",
        ];
        for raw in &bodies {
            let trimmed = raw.trim();
            let val: serde_json::Value = serde_json::from_str(trimmed).unwrap();
            assert!(
                val.get("error").is_some(),
                "body should contain error field: {raw}"
            );
            let msg = val["error"]
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("");
            assert!(!msg.is_empty(), "error message should not be empty");
        }
    }
}
