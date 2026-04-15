use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::{BoxStream, StreamExt, TryStreamExt};
use garraia_common::{Error, Result};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::providers::{
    ChatRole, ContentBlock, LlmProvider, LlmRequest, LlmResponse, MessagePart, Usage,
};

const DEFAULT_BASE_URL: &str = "http://localhost:8080";
const DEFAULT_MODEL: &str = "default";

/// KV cache type for TurboQuant+ compression.
///
/// Reference: <https://github.com/TheTom/turboquant_plus>
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KvCacheType {
    /// Standard 8-bit quantization (default llama.cpp).
    Q8_0,
    /// Standard 4-bit quantization.
    Q4_0,
    /// TurboQuant 4-bit (3.8× compression).
    Turbo4,
    /// TurboQuant 3-bit (4.6–5.1× compression).
    Turbo3,
    /// TurboQuant 2-bit (6.4× compression).
    Turbo2,
    /// Full 16-bit (no compression).
    F16,
}

impl KvCacheType {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Q8_0 => "q8_0",
            Self::Q4_0 => "q4_0",
            Self::Turbo4 => "turbo4",
            Self::Turbo3 => "turbo3",
            Self::Turbo2 => "turbo2",
            Self::F16 => "f16",
        }
    }

    pub fn from_str_lossy(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "q8_0" | "q8" => Self::Q8_0,
            "q4_0" | "q4" => Self::Q4_0,
            "turbo4" | "t4" => Self::Turbo4,
            "turbo3" | "t3" => Self::Turbo3,
            "turbo2" | "t2" => Self::Turbo2,
            "f16" | "fp16" => Self::F16,
            _ => {
                warn!("unknown KV cache type '{s}', falling back to q8_0");
                Self::Q8_0
            }
        }
    }

    /// Approximate compression ratio vs f16.
    pub fn compression_ratio(&self) -> f64 {
        match self {
            Self::F16 => 1.0,
            Self::Q8_0 => 2.0,
            Self::Q4_0 => 4.0,
            Self::Turbo4 => 3.8,
            Self::Turbo3 => 5.1,
            Self::Turbo2 => 6.4,
        }
    }
}

/// Configuration for the llama.cpp TurboQuant+ provider.
#[derive(Debug, Clone)]
pub struct LlamaCppConfig {
    /// KV cache type for keys (recommended: turbo3 or q8_0).
    pub cache_type_k: KvCacheType,
    /// KV cache type for values (recommended: turbo2 — "V compression is free").
    pub cache_type_v: KvCacheType,
    /// Context window size (tokens).
    pub context_size: usize,
    /// Enable flash attention.
    pub flash_attention: bool,
}

impl Default for LlamaCppConfig {
    fn default() -> Self {
        Self {
            cache_type_k: KvCacheType::Q8_0,
            cache_type_v: KvCacheType::Q8_0,
            context_size: 8192,
            flash_attention: true,
        }
    }
}

/// Provider for llama.cpp server with TurboQuant+ KV cache compression.
///
/// Communicates via the OpenAI-compatible API that `llama-server` exposes
/// (`/v1/chat/completions`, `/v1/models`, `/health`).
///
/// The TurboQuant+ cache types (`turbo2`, `turbo3`, `turbo4`) are set at server
/// start-up time via `--cache-type-k` and `--cache-type-v` CLI flags.
/// This provider simply records the config for observability and health checks.
#[derive(Clone)]
pub struct LlamaCppProvider {
    base_url: String,
    model: String,
    client: Client,
    config: LlamaCppConfig,
}

impl LlamaCppProvider {
    pub fn new(
        model: Option<String>,
        base_url: Option<String>,
        config: Option<LlamaCppConfig>,
    ) -> Self {
        Self {
            base_url: base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            model: model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            client: Client::new(),
            config: config.unwrap_or_default(),
        }
    }

    /// Build from the `extra` field of an `LlmProviderConfig`.
    pub fn from_extra(
        model: Option<String>,
        base_url: Option<String>,
        extra: &std::collections::HashMap<String, serde_json::Value>,
    ) -> Self {
        let cache_type_k = extra
            .get("cache_type_k")
            .and_then(|v| v.as_str())
            .map(KvCacheType::from_str_lossy)
            .unwrap_or(KvCacheType::Q8_0);

        let cache_type_v = extra
            .get("cache_type_v")
            .and_then(|v| v.as_str())
            .map(KvCacheType::from_str_lossy)
            .unwrap_or(KvCacheType::Q8_0);

        let context_size = extra
            .get("context_size")
            .and_then(|v| v.as_u64())
            .map(|v| v as usize)
            .unwrap_or(8192);

        let flash_attention = extra
            .get("flash_attention")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        Self::new(
            model,
            base_url,
            Some(LlamaCppConfig {
                cache_type_k,
                cache_type_v,
                context_size,
                flash_attention,
            }),
        )
    }

    /// Returns the KV cache configuration for observability.
    pub fn kv_cache_config(&self) -> &LlamaCppConfig {
        &self.config
    }

    fn build_request_body(&self, request: &LlmRequest, stream: bool) -> Value {
        let model = if request.model.is_empty() {
            self.model.clone()
        } else {
            request.model.clone()
        };

        let mut messages: Vec<Value> = Vec::new();

        if let Some(system) = &request.system {
            messages.push(serde_json::json!({
                "role": "system",
                "content": system,
            }));
        }

        for msg in &request.messages {
            let content = match &msg.content {
                MessagePart::Text(text) => serde_json::json!(text),
                MessagePart::Parts(parts) => {
                    let text_parts: Vec<&str> = parts
                        .iter()
                        .filter_map(|p| match p {
                            ContentBlock::Text { text } => Some(text.as_str()),
                            ContentBlock::ToolResult { content, .. } => Some(content.as_str()),
                            _ => None,
                        })
                        .collect();
                    serde_json::json!(text_parts.join("\n"))
                }
            };

            messages.push(serde_json::json!({
                "role": match msg.role {
                    ChatRole::System => "system",
                    ChatRole::User => "user",
                    ChatRole::Assistant => "assistant",
                    ChatRole::Tool => "tool",
                },
                "content": content,
            }));
        }

        let mut body = serde_json::json!({
            "model": model,
            "messages": messages,
            "stream": stream,
        });

        if let Some(temp) = request.temperature {
            body["temperature"] = serde_json::json!(temp);
        }
        if let Some(max_tokens) = request.max_tokens {
            body["max_tokens"] = serde_json::json!(max_tokens);
        }

        // Serialize tools using OpenAI format (llama-server supports it)
        if !request.tools.is_empty() {
            let tools: Vec<Value> = request
                .tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.input_schema,
                        }
                    })
                })
                .collect();
            body["tools"] = serde_json::json!(tools);
            info!(
                "sending {} tool definitions to llama.cpp (TurboQuant+ K={} V={})",
                request.tools.len(),
                self.config.cache_type_k.as_str(),
                self.config.cache_type_v.as_str(),
            );
        }

        body
    }

    pub async fn stream_complete(
        &self,
        request: &LlmRequest,
    ) -> Result<BoxStream<'static, Result<LlmResponse>>> {
        let body = self.build_request_body(request, true);
        let url = format!("{}/v1/chat/completions", self.base_url);

        let res = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Agent(format!("llama.cpp request failed: {e}")))?;

        if !res.status().is_success() {
            let status = res.status();
            let body_text = res.text().await.unwrap_or_default();
            return Err(Error::Agent(format!(
                "llama.cpp error status: {status} — {body_text}"
            )));
        }

        let stream = res
            .bytes_stream()
            .map_err(|e| Error::Agent(format!("stream error: {e}")));
        let stream: BoxStream<'static, std::result::Result<Bytes, Error>> = Box::pin(stream);

        let lines = futures::stream::unfold(
            (stream, Vec::new()),
            |(mut stream, mut buffer): (
                BoxStream<'static, std::result::Result<Bytes, Error>>,
                Vec<u8>,
            )| async move {
                loop {
                    if let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                        let line_bytes: Vec<u8> = buffer.drain(0..=pos).collect();
                        let line = String::from_utf8_lossy(&line_bytes[..line_bytes.len() - 1])
                            .to_string();
                        if !line.is_empty() {
                            return Some((Ok(line), (stream, buffer)));
                        }
                        continue;
                    }

                    match stream.next().await {
                        Some(Ok(chunk)) => buffer.extend_from_slice(&chunk),
                        Some(Err(e)) => return Some((Err(e), (stream, buffer))),
                        None => {
                            if !buffer.is_empty() {
                                let line = String::from_utf8_lossy(&buffer).to_string();
                                if !line.is_empty() {
                                    return Some((Ok(line), (stream, Vec::new())));
                                }
                            }
                            return None;
                        }
                    }
                }
            },
        );

        let output = lines
            .map(|line_res: Result<String>| {
                let line = line_res?;

                // SSE format: "data: {...}" or "data: [DONE]"
                let json_str = line.strip_prefix("data: ").unwrap_or(&line);
                if json_str == "[DONE]" {
                    return Ok(None);
                }

                let chunk: OpenAiStreamChunk = serde_json::from_str(json_str)
                    .map_err(|e| Error::Agent(format!("failed to parse stream chunk: {e}")))?;

                let delta = chunk.choices.first().and_then(|c| c.delta.as_ref());

                let content_text = delta.and_then(|d| d.content.as_deref()).unwrap_or("");

                let finish_reason = chunk
                    .choices
                    .first()
                    .and_then(|c| c.finish_reason.as_deref());

                Ok(Some(LlmResponse {
                    content: vec![ContentBlock::Text {
                        text: content_text.to_string(),
                    }],
                    model: chunk.model,
                    usage: chunk.usage.map(|u| Usage {
                        input_tokens: u.prompt_tokens,
                        output_tokens: u.completion_tokens,
                    }),
                    stop_reason: finish_reason.map(|s| s.to_string()),
                }))
            })
            .try_filter_map(|x| async move { Ok(x) });

        Ok(Box::pin(output))
    }
}

#[async_trait]
impl LlmProvider for LlamaCppProvider {
    fn provider_id(&self) -> &str {
        "llama-cpp"
    }

    fn configured_model(&self) -> Option<&str> {
        Some(&self.model)
    }

    async fn available_models(&self) -> Result<Vec<String>> {
        let url = format!("{}/v1/models", self.base_url);
        let res = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| Error::Agent(format!("failed to list models: {e}")))?;

        if !res.status().is_success() {
            return Err(Error::Agent(format!(
                "llama.cpp error status: {}",
                res.status()
            )));
        }

        let models_res: OpenAiModelsResponse = res
            .json()
            .await
            .map_err(|e| Error::Agent(format!("failed to parse models response: {e}")))?;

        Ok(models_res.data.into_iter().map(|m| m.id).collect())
    }

    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        let body = self.build_request_body(request, false);
        let url = format!("{}/v1/chat/completions", self.base_url);

        debug!(
            "llama.cpp complete: KV cache K={} V={}, context={}",
            self.config.cache_type_k.as_str(),
            self.config.cache_type_v.as_str(),
            self.config.context_size,
        );

        let res = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Agent(format!("llama.cpp request failed: {e}")))?;

        if !res.status().is_success() {
            let status = res.status();
            let body_text = res.text().await.unwrap_or_default();
            return Err(Error::Agent(format!(
                "llama.cpp error status: {status} — {body_text}"
            )));
        }

        let oai_res: OpenAiResponse = res
            .json()
            .await
            .map_err(|e| Error::Agent(format!("failed to parse llama.cpp response: {e}")))?;

        let choice = oai_res
            .choices
            .first()
            .ok_or_else(|| Error::Agent("no choices in llama.cpp response".to_string()))?;

        let mut content = Vec::new();
        if let Some(text) = &choice.message.content {
            content.push(ContentBlock::Text { text: text.clone() });
        }

        // Parse tool calls if present
        if let Some(tool_calls) = &choice.message.tool_calls {
            for tc in tool_calls {
                let arguments: serde_json::Value =
                    serde_json::from_str(&tc.function.arguments).unwrap_or(Value::Null);
                content.push(ContentBlock::ToolUse {
                    id: tc.id.clone(),
                    name: tc.function.name.clone(),
                    input: arguments,
                });
            }
        }

        if content.is_empty() {
            content.push(ContentBlock::Text {
                text: String::new(),
            });
        }

        Ok(LlmResponse {
            content,
            model: oai_res.model,
            usage: oai_res.usage.map(|u| Usage {
                input_tokens: u.prompt_tokens,
                output_tokens: u.completion_tokens,
            }),
            stop_reason: choice.finish_reason.clone(),
        })
    }

    async fn health_check(&self) -> Result<bool> {
        let url = format!("{}/health", self.base_url);
        match self.client.get(&url).send().await {
            Ok(res) => Ok(res.status().is_success()),
            Err(_) => Ok(false),
        }
    }
}

// ─── OpenAI-compatible response types ──────────────────────────────────────

#[derive(Deserialize)]
struct OpenAiResponse {
    model: String,
    choices: Vec<OpenAiChoice>,
    usage: Option<OpenAiUsage>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiMessage {
    content: Option<String>,
    tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Deserialize)]
struct OpenAiToolCall {
    id: String,
    function: OpenAiFunctionCall,
}

#[derive(Deserialize)]
struct OpenAiFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
struct OpenAiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

#[derive(Deserialize)]
struct OpenAiModelsResponse {
    data: Vec<OpenAiModelEntry>,
}

#[derive(Deserialize)]
struct OpenAiModelEntry {
    id: String,
}

// ─── Streaming response types ──────────────────────────────────────────────

#[derive(Deserialize)]
struct OpenAiStreamChunk {
    model: String,
    choices: Vec<OpenAiStreamChoice>,
    usage: Option<OpenAiUsage>,
}

#[derive(Deserialize)]
struct OpenAiStreamChoice {
    delta: Option<OpenAiStreamDelta>,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiStreamDelta {
    content: Option<String>,
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kv_cache_type_round_trip() {
        assert_eq!(KvCacheType::from_str_lossy("turbo3").as_str(), "turbo3");
        assert_eq!(KvCacheType::from_str_lossy("turbo4").as_str(), "turbo4");
        assert_eq!(KvCacheType::from_str_lossy("turbo2").as_str(), "turbo2");
        assert_eq!(KvCacheType::from_str_lossy("q8_0").as_str(), "q8_0");
        assert_eq!(KvCacheType::from_str_lossy("q4_0").as_str(), "q4_0");
        assert_eq!(KvCacheType::from_str_lossy("f16").as_str(), "f16");
        // fallback
        assert_eq!(KvCacheType::from_str_lossy("unknown").as_str(), "q8_0");
    }

    #[test]
    fn compression_ratios_are_sane() {
        assert!(KvCacheType::Turbo2.compression_ratio() > KvCacheType::Turbo3.compression_ratio());
        assert!(KvCacheType::Turbo3.compression_ratio() > KvCacheType::Turbo4.compression_ratio());
        assert!(KvCacheType::Q4_0.compression_ratio() > KvCacheType::Q8_0.compression_ratio());
    }

    #[test]
    fn from_extra_parses_config() {
        let mut extra = std::collections::HashMap::new();
        extra.insert("cache_type_k".to_string(), serde_json::json!("turbo3"));
        extra.insert("cache_type_v".to_string(), serde_json::json!("turbo2"));
        extra.insert("context_size".to_string(), serde_json::json!(32768));
        extra.insert("flash_attention".to_string(), serde_json::json!(true));

        let provider = LlamaCppProvider::from_extra(
            Some("qwen3".to_string()),
            Some("http://localhost:9090".to_string()),
            &extra,
        );

        assert_eq!(provider.config.cache_type_k, KvCacheType::Turbo3);
        assert_eq!(provider.config.cache_type_v, KvCacheType::Turbo2);
        assert_eq!(provider.config.context_size, 32768);
        assert!(provider.config.flash_attention);
        assert_eq!(provider.provider_id(), "llama-cpp");
    }

    #[test]
    fn request_body_uses_openai_format() {
        let provider = LlamaCppProvider::new(None, None, None);
        let req = LlmRequest {
            model: "qwen3".to_string(),
            messages: vec![crate::providers::ChatMessage {
                role: ChatRole::User,
                content: MessagePart::Text("Hello".to_string()),
            }],
            system: Some("You are helpful.".to_string()),
            max_tokens: Some(200),
            temperature: Some(0.5),
            tools: vec![],
        };

        let body = provider.build_request_body(&req, false);

        assert_eq!(body["model"], "qwen3");
        assert_eq!(body["stream"], false);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], "You are helpful.");
        assert_eq!(body["messages"][1]["role"], "user");
        assert_eq!(body["messages"][1]["content"], "Hello");
        assert_eq!(body["max_tokens"], 200);
        assert_eq!(body["temperature"], 0.5);
    }
}
