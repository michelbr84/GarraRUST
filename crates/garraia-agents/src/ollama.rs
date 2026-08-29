use async_trait::async_trait;
use bytes::Bytes;
use futures::stream::{BoxStream, StreamExt, TryStreamExt};
use garraia_common::{Error, Result};
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;
use tracing::info;

use crate::providers::{
    ChatRole, ContentBlock, LlmProvider, LlmRequest, LlmResponse, MessagePart, Usage,
};

/// Default Ollama tag for GarraIA. `qwen3.8:latest` resolves to
/// `qwen3.8:27b` (Q4_K_M, ~18 GB, 262 144-token context, vision + tools).
const DEFAULT_MODEL: &str = "qwen3.8:latest";
const DEFAULT_BASE_URL: &str = "http://localhost:11434";

/// Normalize an Ollama model reference to its explicit `name:tag` form.
///
/// Ollama defaults the tag to `latest`, so `qwen3.8` and `qwen3.8:latest`
/// name the same model — but `GET /api/tags` only ever reports the explicit
/// form, so membership tests have to normalize first.
///
/// Returns `None` for references that are *not* Ollama tags — notably
/// `provider/model` routes such as `openrouter/auto`, which
/// [`crate::runtime`] resolves to a different provider entirely. A `/`
/// only keeps the reference in Ollama territory when the first segment
/// looks like a registry host, i.e. it contains a `.` or a `:`
/// (`hf.co/…`, `registry.ollama.ai/…`, `localhost:5000/…`).
pub fn normalize_ollama_tag(model: &str) -> Option<String> {
    let model = model.trim();
    if model.is_empty() {
        return None;
    }

    if let Some((first, _)) = model.split_once('/')
        && !first.contains('.')
        && !first.contains(':')
    {
        // `openrouter/auto`, `openai/gpt-4o`, … — a provider route, not a tag.
        return None;
    }

    // Only the final path segment carries the `:tag` suffix; a registry host
    // such as `localhost:5000` has a colon that is a port, not a tag.
    let last_segment = model.rsplit('/').next().unwrap_or(model);
    if last_segment.contains(':') {
        Some(model.to_string())
    } else {
        Some(format!("{model}:latest"))
    }
}

#[derive(Clone)]
pub struct OllamaProvider {
    base_url: String,
    model: String,
    client: Client,
}

impl OllamaProvider {
    pub fn new(model: Option<String>, base_url: Option<String>) -> Self {
        Self {
            base_url: base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            model: model.unwrap_or_else(|| DEFAULT_MODEL.to_string()),
            client: Client::new(),
        }
    }

    pub fn with_client(mut self, client: Client) -> Self {
        self.client = client;
        self
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

        let user_messages: Vec<Value> = request
            .messages
            .iter()
            .map(|msg| {
                let (content, images, tool_calls_out) = match &msg.content {
                    MessagePart::Text(text) => (text.clone(), Vec::new(), Vec::new()),
                    MessagePart::Parts(parts) => {
                        let mut text_parts = Vec::new();
                        let mut images = Vec::new();
                        let mut tool_calls_out: Vec<Value> = Vec::new();

                        for part in parts {
                            match part {
                                ContentBlock::Text { text } => text_parts.push(text.clone()),
                                ContentBlock::Image { url } => {
                                    let b64 =
                                        if let Some(stripped) = url.strip_prefix("data:image/") {
                                            if let Some(idx) = stripped.find(";base64,") {
                                                stripped[idx + 8..].to_string()
                                            } else {
                                                url.clone()
                                            }
                                        } else {
                                            url.clone()
                                        };
                                    images.push(b64);
                                }
                                ContentBlock::ToolUse { id: _, name, input } => {
                                    tool_calls_out.push(serde_json::json!({
                                        "function": {
                                            "name": name,
                                            "arguments": input,
                                        }
                                    }));
                                }
                                ContentBlock::ToolResult {
                                    tool_use_id: _,
                                    content,
                                } => {
                                    text_parts.push(content.clone());
                                }
                            }
                        }

                        (text_parts.join("\n"), images, tool_calls_out)
                    }
                };

                let mut msg_obj = serde_json::json!({
                    "role": match msg.role {
                        ChatRole::System => "system",
                        ChatRole::User => "user",
                        ChatRole::Assistant => "assistant",
                        ChatRole::Tool => "tool",
                    },
                    "content": content,
                });

                if !images.is_empty() {
                    msg_obj["images"] = serde_json::json!(images);
                }
                if !tool_calls_out.is_empty() {
                    msg_obj["tool_calls"] = serde_json::json!(tool_calls_out);
                }

                msg_obj
            })
            .collect();

        messages.extend(user_messages);

        let mut body = serde_json::json!({
            "model": model,
            "messages": messages,
            "stream": stream,
        });

        let mut options = serde_json::Map::new();
        if let Some(temp) = request.temperature {
            options.insert("temperature".to_string(), serde_json::json!(temp));
        }
        if let Some(max_tokens) = request.max_tokens {
            options.insert("num_predict".to_string(), serde_json::json!(max_tokens));
        }
        if !options.is_empty()
            && let Some(obj) = body.as_object_mut()
        {
            obj.insert("options".to_string(), Value::Object(options));
        }

        // Serialize tool definitions into Ollama's tools format
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
            info!("sending {} tool definitions to Ollama", request.tools.len());
        }

        body
    }

    pub async fn stream_complete(
        &self,
        request: &LlmRequest,
    ) -> Result<BoxStream<'static, Result<LlmResponse>>> {
        let body = self.build_request_body(request, true);
        let url = format!("{}/api/chat", self.base_url);

        let res = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Agent(format!("ollama request failed: {e}")))?;

        if !res.status().is_success() {
            return Err(Error::Agent(format!(
                "ollama error status: {}",
                res.status()
            )));
        }

        let stream = res
            .bytes_stream()
            .map_err(|e| Error::Agent(format!("stream error: {e}")));
        let stream: BoxStream<'static, Result<Bytes>> = Box::pin(stream);

        let lines = futures::stream::unfold(
            (stream, Vec::new()),
            |(mut stream, mut buffer): (BoxStream<'static, Result<Bytes>>, Vec<u8>)| async move {
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
                let ollama_res: OllamaResponse = serde_json::from_str(&line)
                    .map_err(|e| Error::Agent(format!("failed to parse stream chunk: {e}")))?;

                let content = ollama_res
                    .message
                    .map(|msg| {
                        let mut blocks = Vec::new();
                        if !msg.content.is_empty() {
                            blocks.push(ContentBlock::Text { text: msg.content });
                        }
                        for tc in msg.tool_calls {
                            blocks.push(ContentBlock::ToolUse {
                                id: uuid::Uuid::new_v4().to_string(),
                                name: tc.function.name,
                                input: tc.function.arguments,
                            });
                        }
                        if blocks.is_empty() {
                            blocks.push(ContentBlock::Text {
                                text: String::new(),
                            });
                        }
                        blocks
                    })
                    .unwrap_or_default();

                let has_tool_use = content
                    .iter()
                    .any(|b| matches!(b, ContentBlock::ToolUse { .. }));

                Ok(Some(LlmResponse {
                    content,
                    model: ollama_res.model,
                    usage: if ollama_res.done {
                        Some(Usage {
                            input_tokens: ollama_res.prompt_eval_count,
                            output_tokens: ollama_res.eval_count,
                        })
                    } else {
                        None
                    },
                    stop_reason: if ollama_res.done {
                        // Check if the response ended because tools were called
                        if has_tool_use {
                            Some("tool_use".to_string())
                        } else {
                            Some("stop".to_string())
                        }
                    } else {
                        None
                    },
                }))
            })
            .try_filter_map(|x| async move { Ok(x) });

        Ok(Box::pin(output))
    }

    pub async fn list_models(&self) -> Result<Vec<String>> {
        let url = format!("{}/api/tags", self.base_url);
        let res = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| Error::Agent(format!("failed to list models: {e}")))?;

        if !res.status().is_success() {
            return Err(Error::Agent(format!(
                "ollama error status: {}",
                res.status()
            )));
        }

        let models_res: OllamaModelsResponse = res
            .json()
            .await
            .map_err(|e| Error::Agent(format!("failed to parse models response: {e}")))?;

        Ok(models_res.models.into_iter().map(|m| m.name).collect())
    }

    /// Return the locally-installed tag matching `model`, if any.
    ///
    /// * `Ok(Some(tag))` — the daemon is reachable and the model is pulled.
    ///   The returned string is the name exactly as `GET /api/tags` reports
    ///   it, so callers can show the resolved tag (`qwen3.8:latest`) rather
    ///   than what the user typed (`qwen3.8`).
    /// * `Ok(None)` — the daemon is reachable but the model is absent, or
    ///   `model` is not an Ollama-shaped reference at all.
    /// * `Err(_)` — the daemon is unreachable or answered with garbage.
    pub async fn resolve_installed_model(&self, model: &str) -> Result<Option<String>> {
        let Some(want) = normalize_ollama_tag(model) else {
            return Ok(None);
        };
        let installed = self.list_models().await?;
        Ok(installed.into_iter().find(|m| {
            m == &want || normalize_ollama_tag(m).is_some_and(|normalized| normalized == want)
        }))
    }

    /// Pull `model` via `POST /api/pull`, invoking `on_progress` for every
    /// NDJSON status line the daemon streams back.
    ///
    /// Uses the HTTP API rather than shelling out to `ollama pull` on
    /// purpose: the `ollama` binary need not be on `$PATH`, and when
    /// `base_url` points at a remote daemon a shell-out would download to
    /// the wrong host.
    pub async fn pull_model(
        &self,
        model: &str,
        mut on_progress: impl FnMut(&PullProgress),
    ) -> Result<()> {
        let tag = normalize_ollama_tag(model)
            .ok_or_else(|| Error::Agent(format!("not an Ollama model tag: {model}")))?;
        let url = format!("{}/api/pull", self.base_url);

        let res = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "model": tag, "stream": true }))
            .send()
            .await
            .map_err(|e| Error::Agent(format!("ollama pull request failed: {e}")))?;

        if !res.status().is_success() {
            return Err(Error::Agent(format!(
                "ollama pull error status: {}",
                res.status()
            )));
        }

        let mut stream = res.bytes_stream();
        let mut buffer: Vec<u8> = Vec::new();
        let mut saw_success = false;

        while let Some(chunk) = stream.next().await {
            let chunk =
                chunk.map_err(|e| Error::Agent(format!("ollama pull stream error: {e}")))?;
            buffer.extend_from_slice(&chunk);
            while let Some(pos) = buffer.iter().position(|&b| b == b'\n') {
                let line_bytes: Vec<u8> = buffer.drain(0..=pos).collect();
                let line = String::from_utf8_lossy(&line_bytes[..line_bytes.len() - 1]);
                if line.trim().is_empty() {
                    continue;
                }
                saw_success |= handle_pull_line(&line, &mut on_progress)?;
            }
        }
        if !buffer.is_empty() {
            let line = String::from_utf8_lossy(&buffer);
            if !line.trim().is_empty() {
                saw_success |= handle_pull_line(&line, &mut on_progress)?;
            }
        }

        if saw_success {
            Ok(())
        } else {
            Err(Error::Agent(format!(
                "ollama pull of {tag} ended without a success status"
            )))
        }
    }
}

/// One NDJSON status line from `POST /api/pull`.
#[derive(Debug, Clone, Deserialize)]
pub struct PullProgress {
    /// Human-readable phase, e.g. `pulling manifest`, `verifying sha256 digest`.
    pub status: String,
    /// Total bytes for the current layer, when the daemon reports one.
    #[serde(default)]
    pub total: Option<u64>,
    /// Bytes transferred so far for the current layer.
    #[serde(default)]
    pub completed: Option<u64>,
}

impl PullProgress {
    /// Completion of the current layer in percent, when both counters are present.
    pub fn percent(&self) -> Option<f64> {
        match (self.completed, self.total) {
            (Some(done), Some(total)) if total > 0 => Some(done as f64 * 100.0 / total as f64),
            _ => None,
        }
    }
}

/// Parse one `/api/pull` NDJSON line. Returns `true` when it is the
/// terminal `success` status; an `{"error": …}` line becomes an `Err`.
fn handle_pull_line(line: &str, on_progress: &mut impl FnMut(&PullProgress)) -> Result<bool> {
    let value: Value = serde_json::from_str(line)
        .map_err(|e| Error::Agent(format!("failed to parse pull status: {e}")))?;

    if let Some(err) = value.get("error").and_then(Value::as_str) {
        return Err(Error::Agent(format!("ollama pull failed: {err}")));
    }

    let progress: PullProgress = serde_json::from_value(value)
        .map_err(|e| Error::Agent(format!("failed to parse pull status: {e}")))?;
    let done = progress.status == "success";
    on_progress(&progress);
    Ok(done)
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn provider_id(&self) -> &str {
        "ollama"
    }

    fn configured_model(&self) -> Option<&str> {
        Some(&self.model)
    }

    async fn available_models(&self) -> Result<Vec<String>> {
        self.list_models().await
    }

    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        let body = self.build_request_body(request, false);
        let url = format!("{}/api/chat", self.base_url);

        let res = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Agent(format!("ollama request failed: {e}")))?;

        if !res.status().is_success() {
            return Err(Error::Agent(format!(
                "ollama error status: {}",
                res.status()
            )));
        }

        let ollama_res: OllamaResponse = res
            .json()
            .await
            .map_err(|e| Error::Agent(format!("failed to parse ollama response: {e}")))?;

        let content = ollama_res
            .message
            .map(|msg| {
                let mut blocks = Vec::new();
                if !msg.content.is_empty() {
                    blocks.push(ContentBlock::Text { text: msg.content });
                }
                for tc in msg.tool_calls {
                    blocks.push(ContentBlock::ToolUse {
                        id: uuid::Uuid::new_v4().to_string(),
                        name: tc.function.name,
                        input: tc.function.arguments,
                    });
                }
                if blocks.is_empty() {
                    blocks.push(ContentBlock::Text {
                        text: String::new(),
                    });
                }
                blocks
            })
            .unwrap_or_default();

        let has_tool_use = content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolUse { .. }));

        Ok(LlmResponse {
            content,
            model: ollama_res.model,
            usage: Some(Usage {
                input_tokens: ollama_res.prompt_eval_count,
                output_tokens: ollama_res.eval_count,
            }),
            stop_reason: if ollama_res.done {
                // Check if the response ended because tools were called
                if has_tool_use {
                    Some("tool_use".to_string())
                } else {
                    Some("stop".to_string())
                }
            } else {
                None
            },
        })
    }

    async fn health_check(&self) -> Result<bool> {
        match self.list_models().await {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}

#[derive(Deserialize)]
struct OllamaResponse {
    model: String,
    message: Option<OllamaMessage>,
    done: bool,
    #[serde(default)]
    eval_count: u32,
    #[serde(default)]
    prompt_eval_count: u32,
}

#[derive(Deserialize)]
struct OllamaMessage {
    #[serde(default)]
    content: String,
    #[serde(default)]
    tool_calls: Vec<OllamaToolCall>,
}

#[derive(Deserialize)]
struct OllamaToolCall {
    function: OllamaFunctionCall,
}

#[derive(Deserialize)]
struct OllamaFunctionCall {
    name: String,
    #[serde(default)]
    arguments: Value,
}

#[derive(Deserialize)]
struct OllamaModelsResponse {
    models: Vec<OllamaModel>,
}

#[derive(Deserialize)]
struct OllamaModel {
    name: String,
}

#[cfg(test)]
mod tests {
    use axum::{
        Json, Router,
        routing::{get, post},
    };
    use futures::StreamExt;
    use serde_json::{Value, json};
    use tokio::sync::oneshot;

    use crate::providers::{
        ChatMessage, ChatRole, ContentBlock, LlmProvider, LlmRequest, MessagePart,
    };

    use super::OllamaProvider;

    #[test]
    fn request_serialization_includes_options() {
        let provider = OllamaProvider::new(None, None);
        let req = LlmRequest {
            model: "llama3".to_string(),
            messages: vec![ChatMessage {
                role: ChatRole::User,
                content: MessagePart::Text("Hello".to_string()),
            }],
            system: None,
            max_tokens: Some(100),
            temperature: Some(0.7),
            tools: vec![],
        };

        let body = provider.build_request_body(&req, false);

        assert_eq!(body["model"], "llama3");
        assert_eq!(body["stream"], false);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "Hello");
        assert_eq!(body["options"]["temperature"], 0.7);
        assert_eq!(body["options"]["num_predict"], 100);
    }

    async fn run_mock_server() -> (String, oneshot::Sender<()>) {
        let (tx, rx) = oneshot::channel::<()>();

        let app = Router::new()
            .route(
                "/api/tags",
                get(|| async {
                    Json(json!({
                        "models": [
                            { "name": "llama3:latest" },
                            { "name": "mistral:latest" }
                        ]
                    }))
                }),
            )
            .route(
                "/api/chat",
                post(|Json(payload): Json<Value>| async move {
                    let stream = payload
                        .get("stream")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    if stream {
                        "{\"model\":\"llama3\",\"message\":{\"content\":\"Hello\"},\"done\":false}\n{\"model\":\"llama3\",\"message\":{\"content\":\" World\"},\"done\":true}".to_string()
                    } else {
                        serde_json::to_string(&json!({
                            "model": "llama3",
                            "message": { "content": "Hello World" },
                            "done": true,
                            "prompt_eval_count": 10,
                            "eval_count": 5
                        }))
                        .unwrap()
                    }
                }),
            )
            .route(
                "/api/pull",
                post(|Json(payload): Json<Value>| async move {
                    let model = payload
                        .get("model")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string();
                    if model.starts_with("boom") {
                        return "{\"error\":\"model not found\"}".to_string();
                    }
                    // Trailing line intentionally left without a newline, to
                    // exercise the final-buffer drain in `pull_model`.
                    "{\"status\":\"pulling manifest\"}\n\
                     {\"status\":\"pulling abc123\",\"total\":100,\"completed\":40}\n\
                     {\"status\":\"success\"}"
                        .to_string()
                }),
            );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{}", addr);

        tokio::spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await
                .unwrap();
        });

        (url, tx)
    }

    #[tokio::test]
    async fn list_models_works() {
        let (url, stop) = run_mock_server().await;
        let provider = OllamaProvider::new(None, Some(url));

        let models = provider.list_models().await.unwrap();
        assert_eq!(models.len(), 2);
        assert!(models.contains(&"llama3:latest".to_string()));

        let _ = stop.send(());
    }

    #[test]
    fn default_model_is_qwen38() {
        // Spec-locked: the GarraIA default Ollama tag. Changing it means
        // changing `chat::hardcoded_default_model("ollama")` and the docs
        // in lockstep.
        assert_eq!(super::DEFAULT_MODEL, "qwen3.8:latest");
        assert_eq!(
            OllamaProvider::new(None, None).configured_model(),
            Some("qwen3.8:latest")
        );
    }

    #[test]
    fn normalize_ollama_tag_appends_latest() {
        let n = super::normalize_ollama_tag;
        assert_eq!(n("qwen3.8").as_deref(), Some("qwen3.8:latest"));
        assert_eq!(n("llama3.1").as_deref(), Some("llama3.1:latest"));
        assert_eq!(n("  qwen3.8  ").as_deref(), Some("qwen3.8:latest"));
    }

    #[test]
    fn normalize_ollama_tag_preserves_explicit_tags() {
        let n = super::normalize_ollama_tag;
        assert_eq!(n("qwen3.8:27b").as_deref(), Some("qwen3.8:27b"));
        assert_eq!(n("qwen3.8:latest").as_deref(), Some("qwen3.8:latest"));
        // Idempotent.
        let once = n("qwen3.8").unwrap_or_default();
        assert_eq!(n(&once).as_deref(), Some("qwen3.8:latest"));
    }

    #[test]
    fn normalize_ollama_tag_handles_registry_hosts() {
        let n = super::normalize_ollama_tag;
        assert_eq!(
            n("hf.co/MaziyarPanahi/Qwen3-14B-GGUF:Q4_K_M").as_deref(),
            Some("hf.co/MaziyarPanahi/Qwen3-14B-GGUF:Q4_K_M")
        );
        assert_eq!(
            n("hf.co/MaziyarPanahi/Qwen3-14B-GGUF").as_deref(),
            Some("hf.co/MaziyarPanahi/Qwen3-14B-GGUF:latest")
        );
        assert_eq!(
            n("registry.ollama.ai/library/llama3").as_deref(),
            Some("registry.ollama.ai/library/llama3:latest")
        );
        // The colon here is a port, not a tag.
        assert_eq!(
            n("localhost:5000/mymodel").as_deref(),
            Some("localhost:5000/mymodel:latest")
        );
    }

    #[test]
    fn normalize_ollama_tag_rejects_provider_routes() {
        let n = super::normalize_ollama_tag;
        // `runtime::resolve_provider_from_model` owns these — they must never
        // be mistaken for Ollama tags or `--model openrouter/auto` would be
        // hijacked to a local provider.
        assert_eq!(n("openrouter/auto"), None);
        assert_eq!(n("openai/gpt-4o"), None);
        assert_eq!(n("anthropic/claude-sonnet-4-5"), None);
        assert_eq!(n(""), None);
        assert_eq!(n("   "), None);
    }

    #[tokio::test]
    async fn resolve_installed_model_matches_normalized_tags() {
        let (url, stop) = run_mock_server().await;
        let provider = OllamaProvider::new(None, Some(url));

        // Mock serves `llama3:latest` + `mistral:latest`.
        assert_eq!(
            provider.resolve_installed_model("llama3").await.unwrap(),
            Some("llama3:latest".to_string())
        );
        assert_eq!(
            provider
                .resolve_installed_model("llama3:latest")
                .await
                .unwrap(),
            Some("llama3:latest".to_string())
        );
        assert_eq!(
            provider.resolve_installed_model("qwen3.8").await.unwrap(),
            None
        );
        // Not an Ollama reference at all — short-circuits before any HTTP.
        assert_eq!(
            provider
                .resolve_installed_model("openrouter/auto")
                .await
                .unwrap(),
            None
        );

        let _ = stop.send(());
    }

    #[tokio::test]
    async fn pull_model_streams_progress_to_success() {
        let (url, stop) = run_mock_server().await;
        let provider = OllamaProvider::new(None, Some(url));

        let mut seen: Vec<String> = Vec::new();
        let mut percents: Vec<f64> = Vec::new();
        provider
            .pull_model("qwen3.8", |p| {
                seen.push(p.status.clone());
                if let Some(pct) = p.percent() {
                    percents.push(pct);
                }
            })
            .await
            .unwrap();

        assert_eq!(seen, vec!["pulling manifest", "pulling abc123", "success"]);
        assert_eq!(percents, vec![40.0]);

        let _ = stop.send(());
    }

    #[tokio::test]
    async fn pull_model_surfaces_daemon_error() {
        let (url, stop) = run_mock_server().await;
        let provider = OllamaProvider::new(None, Some(url));

        let err = provider
            .pull_model("boom-model", |_| {})
            .await
            .expect_err("daemon reported an error");
        assert!(err.to_string().contains("model not found"), "{err}");

        let _ = stop.send(());
    }

    #[tokio::test]
    async fn pull_model_rejects_provider_routes() {
        let provider = OllamaProvider::new(None, Some("http://127.0.0.1:1".to_string()));
        let err = provider
            .pull_model("openrouter/auto", |_| {})
            .await
            .expect_err("not an Ollama tag");
        assert!(err.to_string().contains("not an Ollama model tag"), "{err}");
    }

    #[tokio::test]
    async fn complete_works() {
        let (url, stop) = run_mock_server().await;
        let provider = OllamaProvider::new(None, Some(url));

        let req = LlmRequest {
            model: "llama3".to_string(),
            messages: vec![ChatMessage {
                role: ChatRole::User,
                content: MessagePart::Text("Hi".to_string()),
            }],
            system: None,
            max_tokens: None,
            temperature: None,
            tools: vec![],
        };

        let res = provider.complete(&req).await.unwrap();
        match &res.content[0] {
            ContentBlock::Text { text } => assert_eq!(text, "Hello World"),
            _ => panic!("expected text content"),
        }

        let _ = stop.send(());
    }

    #[tokio::test]
    async fn stream_complete_works() {
        let (url, stop) = run_mock_server().await;
        let provider = OllamaProvider::new(None, Some(url));

        let req = LlmRequest {
            model: "llama3".to_string(),
            messages: vec![],
            system: None,
            max_tokens: None,
            temperature: None,
            tools: vec![],
        };

        let mut stream = provider.stream_complete(&req).await.unwrap();
        let mut full_text = String::new();
        while let Some(chunk_res) = stream.next().await {
            let chunk = chunk_res.unwrap();
            if let ContentBlock::Text { text } = &chunk.content[0] {
                full_text.push_str(text);
            }
        }

        assert_eq!(full_text, "Hello World");
        let _ = stop.send(());
    }

    #[test]
    fn request_serialization_includes_tools() {
        let provider = OllamaProvider::new(None, None);
        let tools = vec![crate::providers::ToolDefinition {
            name: "test_tool".to_string(),
            description: "A test tool".to_string(),
            input_schema: json!({"type": "object"}),
        }];
        let req = LlmRequest {
            model: "llama3".to_string(),
            messages: vec![],
            system: None,
            max_tokens: None,
            temperature: None,
            tools,
        };

        let body = provider.build_request_body(&req, false);

        // precise verification of tool structure
        let tools_json = body.get("tools").expect("tools field missing");
        let tool = &tools_json[0];
        assert_eq!(tool["type"], "function");
        assert_eq!(tool["function"]["name"], "test_tool");
        assert_eq!(tool["function"]["description"], "A test tool");
    }
}
