//! `POST /v1/messages` — superfície compatível com a Anthropic Messages API.
//!
//! Existe para que o Claude Code (e qualquer cliente que fale esse wire) possa
//! apontar `ANTHROPIC_BASE_URL` para o gateway e usar o provedor configurado,
//! herdando o failover primário→backup do runtime de graça.
//!
//! # Isto é um proxy, não um agente
//!
//! O caminho **não** passa por `AgentRuntime::process_message_*`. Aquele loop
//! injeta as tools do próprio GarraIA, executa-as ele mesmo e devolve uma
//! `String` achatada. O Claude Code precisa dos blocos `tool_use` crus de volta
//! para rodar o próprio Read/Edit/Bash — passar pelo runtime significaria que
//! ele nunca conseguiria editar um arquivo. Também não há hidratação de
//! sessão: a API da Anthropic é stateless e o cliente reenvia a conversa
//! inteira a cada turno, então reusar o histórico do gateway duplicaria
//! contexto.
//!
//! # Streaming é obrigatório
//!
//! O Claude Code manda `"stream": true` no loop principal. Mas nem todo
//! provedor sabe streamar — o `OllamaProvider`, por exemplo, não implementa
//! `stream_complete` no trait, então o default devolve `Err`. Por isso o
//! envelope SSE é **sintetizado sobre `complete()`** quando o streaming real
//! não está disponível: o cliente sempre recebe uma sequência SSE válida.

use std::convert::Infallible;
use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
    routing::post,
};
use futures::stream::{self, Stream, StreamExt};
use garraia_agents::{
    ChatMessage, ChatRole, ContentBlock, LlmProvider, LlmRequest, MessagePart, StreamEvent,
    ToolDefinition,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{debug, warn};

use crate::state::SharedState;

/// A Anthropic exige `max_tokens`. Clientes reais sempre mandam; o default
/// existe só para não transformar um campo ausente em 400.
const DEFAULT_MAX_TOKENS: u32 = 4096;

/// Estimativa grosseira de tokens quando o provedor não reporta uso.
///
/// Precisa ser diferente de zero: o Claude Code lê `usage.input_tokens` do
/// `message_start` para o medidor de contexto **e para disparar o auto-compact**.
/// Com zero ele nunca compacta, o transcript cresce sem limite e o provedor
/// acaba recusando por tamanho de contexto.
fn estimate_tokens(text: &str) -> u32 {
    (text.chars().count() / 4).max(1) as u32
}

/// Usa o valor reportado só quando ele é diferente de zero.
///
/// Zero reportado é "não sei", não "custou nada" — e um zero repassado deixa o
/// medidor de contexto do cliente parado e o auto-compact nunca dispara.
fn non_zero_or(reported: Option<u32>, estimate: u32) -> u32 {
    match reported {
        Some(v) if v > 0 => v,
        _ => estimate,
    }
}

// =============================================================================
// Wire de entrada
// =============================================================================

#[derive(Debug, Deserialize)]
pub struct MessagesRequest {
    pub model: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    pub messages: Vec<InboundMessage>,
    #[serde(default)]
    pub system: Option<SystemField>,
    #[serde(default)]
    pub tools: Vec<InboundTool>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub stream: bool,
    // Sem `deny_unknown_fields`: clientes reais mandam `metadata`,
    // `cache_control`, `top_k`, `thinking`, `tool_choice`… e recusar por causa
    // de um campo que não usamos quebraria o cliente sem motivo.
}

fn default_max_tokens() -> u32 {
    DEFAULT_MAX_TOKENS
}

#[derive(Debug, Deserialize)]
pub struct InboundMessage {
    pub role: String,
    pub content: InboundContent,
}

/// `content` aceita string simples ou lista de blocos.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum InboundContent {
    Text(String),
    Blocks(Vec<InboundBlock>),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum InboundBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        #[serde(default)]
        input: Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        #[serde(default)]
        content: ToolResultContent,
        #[serde(default)]
        is_error: bool,
    },
    #[serde(rename = "image")]
    Image {
        #[serde(default)]
        source: Value,
    },
    /// Blocos futuros (`thinking`, etc.) não podem derrubar a requisição.
    #[serde(other)]
    Unknown,
}

/// `tool_result.content` é `String | Vec<Block>` no wire da Anthropic.
#[derive(Debug, Deserialize, Default)]
#[serde(untagged)]
pub enum ToolResultContent {
    Text(String),
    Blocks(Vec<Value>),
    #[default]
    Empty,
}

impl ToolResultContent {
    fn flatten(&self) -> String {
        match self {
            ToolResultContent::Text(t) => t.clone(),
            ToolResultContent::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n"),
            ToolResultContent::Empty => String::new(),
        }
    }
}

/// `system` é campo de topo e chega como string ou lista de blocos de texto
/// (o Claude Code manda lista, com `cache_control` que não repassamos).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum SystemField {
    Text(String),
    Blocks(Vec<Value>),
}

impl SystemField {
    fn flatten(&self) -> String {
        match self {
            SystemField::Text(t) => t.clone(),
            SystemField::Blocks(blocks) => blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n\n"),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct InboundTool {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub input_schema: Value,
}

// =============================================================================
// Tradução
// =============================================================================

/// Converte a requisição do wire da Anthropic para o `LlmRequest` interno.
///
/// Pura e sem I/O, para ser testável sem provedor nenhum.
pub fn to_llm_request(req: &MessagesRequest) -> LlmRequest {
    let messages = req
        .messages
        .iter()
        .map(|m| ChatMessage {
            role: match m.role.as_str() {
                "assistant" => ChatRole::Assistant,
                "system" => ChatRole::System,
                "tool" => ChatRole::Tool,
                _ => ChatRole::User,
            },
            content: match &m.content {
                InboundContent::Text(t) => MessagePart::Text(t.clone()),
                InboundContent::Blocks(blocks) => {
                    MessagePart::Parts(blocks.iter().filter_map(convert_block).collect())
                }
            },
        })
        .collect();

    LlmRequest {
        model: req.model.clone(),
        messages,
        system: req.system.as_ref().map(SystemField::flatten),
        max_tokens: Some(req.max_tokens),
        temperature: req.temperature,
        tools: req
            .tools
            .iter()
            .map(|t| ToolDefinition {
                name: t.name.clone(),
                description: t.description.clone(),
                input_schema: t.input_schema.clone(),
            })
            .collect(),
    }
}

fn convert_block(block: &InboundBlock) -> Option<ContentBlock> {
    match block {
        InboundBlock::Text { text } => Some(ContentBlock::Text { text: text.clone() }),
        InboundBlock::ToolUse { id, name, input } => Some(ContentBlock::ToolUse {
            id: id.clone(),
            name: name.clone(),
            input: input.clone(),
        }),
        InboundBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            let flattened = content.flatten();
            Some(ContentBlock::ToolResult {
                tool_use_id: tool_use_id.clone(),
                // `ContentBlock::ToolResult` não tem `is_error`; prefixar
                // preserva a informação sem uma mudança cross-crate que tocaria
                // os match arms de todos os providers.
                content: if *is_error {
                    format!("[tool error] {flattened}")
                } else {
                    flattened
                },
            })
        }
        InboundBlock::Image { .. } => {
            // `ContentBlock::Image` carrega só uma URL, que não acomoda
            // `source.base64`. Degradar explicitamente é melhor que descartar
            // em silêncio: o modelo vê que havia uma imagem.
            Some(ContentBlock::Text {
                text: "[image omitted: not supported by this gateway]".to_string(),
            })
        }
        InboundBlock::Unknown => None,
    }
}

/// Mapeia o motivo de parada interno para o vocabulário da Anthropic.
///
/// **Crítico:** se a resposta contém blocos `tool_use`, o `stop_reason`
/// precisa ser `tool_use`, senão o Claude Code apenas imprime as tools em vez
/// de executá-las. Por isso a presença de bloco manda, não só o campo — os
/// modelos da OpenRouter são inconsistentes no `finish_reason`.
pub fn map_stop_reason(raw: Option<&str>, has_tool_use: bool) -> &'static str {
    if has_tool_use {
        return "tool_use";
    }
    match raw {
        Some("length") | Some("max_tokens") => "max_tokens",
        Some("tool_calls") | Some("function_call") | Some("tool_use") => "tool_use",
        Some("stop_sequence") => "stop_sequence",
        _ => "end_turn",
    }
}

/// Serializa um `ContentBlock` interno no formato de saída da Anthropic.
fn block_to_json(block: &ContentBlock) -> Option<Value> {
    match block {
        ContentBlock::Text { text } => Some(json!({ "type": "text", "text": text })),
        ContentBlock::ToolUse { id, name, input } => {
            Some(json!({ "type": "tool_use", "id": id, "name": name, "input": input }))
        }
        // Um `tool_result` nunca aparece numa resposta do assistente.
        _ => None,
    }
}

// =============================================================================
// Erros
// =============================================================================

/// Erro no formato que o cliente da Anthropic entende.
///
/// O status importa: o Claude Code só aciona o retry dele em 429 e 529, então
/// devolver 500 genérico transforma uma sobrecarga temporária em falha opaca.
fn anthropic_error(status: StatusCode, kind: &str, message: &str) -> Response {
    (
        status,
        Json(json!({
            "type": "error",
            "error": { "type": kind, "message": message }
        })),
    )
        .into_response()
}

// =============================================================================
// Handlers
// =============================================================================

/// Resolve o provedor que atenderá a requisição.
fn resolve_provider(state: &SharedState) -> Option<Arc<dyn LlmProvider>> {
    state.agents.default_provider()
}

pub async fn messages_handler(
    State(state): State<SharedState>,
    headers: HeaderMap,
    Json(req): Json<MessagesRequest>,
) -> Response {
    // `anthropic-version` e `anthropic-beta` são aceitos e ignorados: recusar
    // por causa deles quebraria clientes por nada.
    debug!(
        version = ?headers.get("anthropic-version"),
        model = %req.model,
        stream = req.stream,
        tools = req.tools.len(),
        "POST /v1/messages"
    );

    let Some(provider) = resolve_provider(&state) else {
        return anthropic_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "api_error",
            "No LLM provider is configured on this gateway. Run `garra config set-routing`.",
        );
    };

    let llm_request = to_llm_request(&req);
    let input_estimate = estimate_request_tokens(&llm_request);

    if req.stream {
        stream_messages(state, provider, req, llm_request, input_estimate).await
    } else {
        complete_messages(state, provider, req, llm_request, input_estimate).await
    }
}

async fn complete_messages(
    state: SharedState,
    provider: Arc<dyn LlmProvider>,
    req: MessagesRequest,
    llm_request: LlmRequest,
    input_estimate: u32,
) -> Response {
    match state
        .agents
        .complete_with_fallback(&provider, &llm_request)
        .await
    {
        Ok(resp) => {
            let has_tool_use = resp
                .content
                .iter()
                .any(|b| matches!(b, ContentBlock::ToolUse { .. }));
            let content: Vec<Value> = resp.content.iter().filter_map(block_to_json).collect();
            let output_estimate =
                estimate_tokens(&serde_json::to_string(&content).unwrap_or_default());
            // A reported zero means "I don't know", not "this was free": some
            // providers omit accounting entirely. Passing that zero through
            // would leave Claude Code's context meter pinned at empty and
            // auto-compact would never fire.
            let (input_tokens, output_tokens) = (
                non_zero_or(resp.usage.as_ref().map(|u| u.input_tokens), input_estimate),
                non_zero_or(
                    resp.usage.as_ref().map(|u| u.output_tokens),
                    output_estimate,
                ),
            );

            Json(json!({
                "id": format!("msg_{}", uuid::Uuid::new_v4().simple()),
                "type": "message",
                "role": "assistant",
                "model": req.model,
                "content": content,
                "stop_reason": map_stop_reason(resp.stop_reason.as_deref(), has_tool_use),
                "stop_sequence": Value::Null,
                "usage": { "input_tokens": input_tokens, "output_tokens": output_tokens },
            }))
            .into_response()
        }
        Err(e) => {
            warn!("/v1/messages upstream failure: {e}");
            let msg = e.to_string();
            // Sobrecarga vira 529 para o retry do cliente engatar.
            let (status, kind) = if msg.contains("429") || msg.to_lowercase().contains("rate limit")
            {
                (StatusCode::TOO_MANY_REQUESTS, "rate_limit_error")
            } else if msg.contains("503") || msg.contains("overload") {
                (
                    StatusCode::from_u16(529).unwrap_or(StatusCode::SERVICE_UNAVAILABLE),
                    "overloaded_error",
                )
            } else {
                (StatusCode::BAD_GATEWAY, "api_error")
            };
            anthropic_error(status, kind, &msg)
        }
    }
}

/// Monta a sequência SSE.
///
/// Os eventos precisam ser **nomeados** (`event: content_block_delta`): o
/// stream OpenAI-compatible do gateway usa `Event::default().data(...)` sem
/// nome, o que serve para clientes OpenAI mas não para clientes Anthropic, que
/// despacham pelo nome do evento.
async fn stream_messages(
    state: SharedState,
    provider: Arc<dyn LlmProvider>,
    req: MessagesRequest,
    llm_request: LlmRequest,
    input_estimate: u32,
) -> Response {
    let message_id = format!("msg_{}", uuid::Uuid::new_v4().simple());
    let model = req.model.clone();

    let upstream = state
        .agents
        .stream_complete_with_fallback(&provider, &llm_request)
        .await;

    match upstream {
        Ok(events) => sse_from_stream(message_id, model, input_estimate, events).into_response(),
        Err(e) => {
            // Provedor sem streaming (o Ollama é o caso comum): sintetiza o
            // envelope SSE sobre uma resposta completa em vez de dar 500.
            debug!("provider cannot stream ({e}); synthesising SSE from a complete() call");
            match state
                .agents
                .complete_with_fallback(&provider, &llm_request)
                .await
            {
                Ok(resp) => {
                    let has_tool_use = resp
                        .content
                        .iter()
                        .any(|b| matches!(b, ContentBlock::ToolUse { .. }));
                    let stop = map_stop_reason(resp.stop_reason.as_deref(), has_tool_use);
                    let output_estimate = estimate_tokens(
                        &resp
                            .content
                            .iter()
                            .filter_map(|b| match b {
                                ContentBlock::Text { text } => Some(text.clone()),
                                _ => None,
                            })
                            .collect::<String>(),
                    );
                    let output_tokens = non_zero_or(
                        resp.usage.as_ref().map(|u| u.output_tokens),
                        output_estimate,
                    );
                    let input_tokens =
                        non_zero_or(resp.usage.as_ref().map(|u| u.input_tokens), input_estimate);

                    let synthetic = synth_events(&resp.content);
                    sse_from_synthetic(
                        message_id,
                        model,
                        input_tokens,
                        output_tokens,
                        stop,
                        synthetic,
                    )
                    .into_response()
                }
                Err(e) => {
                    warn!("/v1/messages streaming fallback failed: {e}");
                    anthropic_error(StatusCode::BAD_GATEWAY, "api_error", &e.to_string())
                }
            }
        }
    }
}

/// Blocos completos → a mesma sequência de eventos que um stream real produz.
fn synth_events(content: &[ContentBlock]) -> Vec<Value> {
    let mut out = Vec::new();
    for (index, block) in content.iter().enumerate() {
        match block {
            ContentBlock::Text { text } => {
                out.push(json!({
                    "event": "content_block_start",
                    "data": { "type": "content_block_start", "index": index,
                              "content_block": { "type": "text", "text": "" } }
                }));
                out.push(json!({
                    "event": "content_block_delta",
                    "data": { "type": "content_block_delta", "index": index,
                              "delta": { "type": "text_delta", "text": text } }
                }));
                out.push(json!({
                    "event": "content_block_stop",
                    "data": { "type": "content_block_stop", "index": index }
                }));
            }
            ContentBlock::ToolUse { id, name, input } => {
                out.push(json!({
                    "event": "content_block_start",
                    "data": { "type": "content_block_start", "index": index,
                              "content_block": { "type": "tool_use", "id": id, "name": name, "input": {} } }
                }));
                out.push(json!({
                    "event": "content_block_delta",
                    "data": { "type": "content_block_delta", "index": index,
                              "delta": { "type": "input_json_delta",
                                         "partial_json": serde_json::to_string(input).unwrap_or_default() } }
                }));
                out.push(json!({
                    "event": "content_block_stop",
                    "data": { "type": "content_block_stop", "index": index }
                }));
            }
            _ => {}
        }
    }
    out
}

fn message_start(message_id: &str, model: &str, input_tokens: u32) -> Value {
    json!({
        "type": "message_start",
        "message": {
            "id": message_id,
            "type": "message",
            "role": "assistant",
            "model": model,
            "content": [],
            "stop_reason": Value::Null,
            "stop_sequence": Value::Null,
            // Não-zero de propósito: com zero o cliente nunca dispara auto-compact.
            "usage": { "input_tokens": input_tokens, "output_tokens": 0 }
        }
    })
}

fn named(event: &str, data: Value) -> Event {
    Event::default()
        .event(event)
        .data(serde_json::to_string(&data).unwrap_or_default())
}

fn sse_from_synthetic(
    message_id: String,
    model: String,
    input_tokens: u32,
    output_tokens: u32,
    stop_reason: &'static str,
    events: Vec<Value>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut frames: Vec<Event> = vec![named(
        "message_start",
        message_start(&message_id, &model, input_tokens),
    )];
    for e in events {
        let name = e["event"]
            .as_str()
            .unwrap_or("content_block_delta")
            .to_string();
        frames.push(named(&name, e["data"].clone()));
    }
    frames.push(named(
        "message_delta",
        json!({
            "type": "message_delta",
            "delta": { "stop_reason": stop_reason, "stop_sequence": Value::Null },
            "usage": { "output_tokens": output_tokens }
        }),
    ));
    frames.push(named("message_stop", json!({ "type": "message_stop" })));

    Sse::new(stream::iter(frames.into_iter().map(Ok)))
}

fn sse_from_stream(
    message_id: String,
    model: String,
    input_tokens: u32,
    upstream: std::pin::Pin<Box<dyn Stream<Item = garraia_common::Result<StreamEvent>> + Send>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let head = stream::iter(vec![Ok(named(
        "message_start",
        message_start(&message_id, &model, input_tokens),
    ))]);

    // O wire da Anthropic exige `content_block_start` antes do primeiro delta,
    // mas o `StreamEvent` interno não tem esse evento e o `TextDelta` não
    // carrega índice. O índice do bloco corrente é mantido aqui, e o
    // `content_block_start` é emitido junto do primeiro delta de cada bloco.
    let mut block_index: usize = 0;
    let mut block_open = false;

    let body = upstream.flat_map(move |item| {
        let frames: Vec<Result<Event, Infallible>> = match item {
            Ok(StreamEvent::TextDelta(text)) => {
                let mut out = Vec::new();
                if !block_open {
                    block_open = true;
                    out.push(Ok(named(
                        "content_block_start",
                        json!({ "type": "content_block_start", "index": block_index,
                                "content_block": { "type": "text", "text": "" } }),
                    )));
                }
                out.push(Ok(named(
                    "content_block_delta",
                    json!({ "type": "content_block_delta", "index": block_index,
                            "delta": { "type": "text_delta", "text": text } }),
                )));
                out
            }
            Ok(StreamEvent::ToolUseStart { index, id, name }) => {
                block_index = index;
                block_open = true;
                vec![Ok(named(
                    "content_block_start",
                    json!({ "type": "content_block_start", "index": index,
                            "content_block": { "type": "tool_use", "id": id, "name": name, "input": {} } }),
                ))]
            }
            Ok(StreamEvent::InputJsonDelta(partial_json)) => vec![Ok(named(
                "content_block_delta",
                json!({ "type": "content_block_delta", "index": block_index,
                        "delta": { "type": "input_json_delta", "partial_json": partial_json } }),
            ))],
            Ok(StreamEvent::ContentBlockStop { index }) => {
                block_open = false;
                block_index = index + 1;
                vec![Ok(named(
                    "content_block_stop",
                    json!({ "type": "content_block_stop", "index": index }),
                ))]
            }
            Ok(StreamEvent::MessageDelta { stop_reason, usage }) => vec![Ok(named(
                "message_delta",
                json!({
                    "type": "message_delta",
                    "delta": {
                        "stop_reason": map_stop_reason(stop_reason.as_deref(), false),
                        "stop_sequence": Value::Null
                    },
                    "usage": { "output_tokens": non_zero_or(usage.map(|u| u.output_tokens), 1) }
                }),
            ))],
            Ok(StreamEvent::MessageStop) => {
                vec![Ok(named("message_stop", json!({ "type": "message_stop" })))]
            }
            Err(e) => {
                warn!("/v1/messages stream error: {e}");
                vec![Ok(named(
                    "error",
                    json!({ "type": "error",
                            "error": { "type": "api_error", "message": e.to_string() } }),
                ))]
            }
        };
        stream::iter(frames)
    });

    Sse::new(head.chain(body))
}

/// `POST /v1/messages/count_tokens`.
///
/// Versões recentes do Claude Code chamam este endpoint; devolver 404 faria a
/// contagem de contexto dele falhar em silêncio. Sem colisão de rota, então é
/// barato de oferecer — a estimativa é `chars/4`, a mesma usada no resto.
pub async fn count_tokens_handler(Json(req): Json<MessagesRequest>) -> Response {
    let total = estimate_request_tokens(&to_llm_request(&req));
    Json(json!({ "input_tokens": total })).into_response()
}

/// Estimativa de tokens de entrada de uma requisição inteira.
///
/// Compartilhada entre `/v1/messages` (que precisa dela para o `message_start`)
/// e `/v1/messages/count_tokens`, para os dois nunca discordarem.
pub fn estimate_request_tokens(req: &LlmRequest) -> u32 {
    let body: u32 = req
        .messages
        .iter()
        .map(|m| match &m.content {
            MessagePart::Text(t) => estimate_tokens(t),
            MessagePart::Parts(blocks) => blocks
                .iter()
                .map(|b| match b {
                    ContentBlock::Text { text } => estimate_tokens(text),
                    ContentBlock::ToolResult { content, .. } => estimate_tokens(content),
                    _ => 8,
                })
                .sum(),
        })
        .sum();
    let system = req.system.as_deref().map(estimate_tokens).unwrap_or(0);
    (body + system).max(1)
}

/// Rotas Anthropic-compatible.
///
/// **Não** registra `GET /v1/models`: `build_openai_router` já registra essa
/// rota, e o Axum entra em pânico no boot com método+path duplicados. O Claude
/// Code não precisa dela quando `ANTHROPIC_MODEL` está definido.
pub fn build_anthropic_router(state: SharedState) -> Router {
    Router::new()
        .route("/v1/messages", post(messages_handler))
        .route("/v1/messages/count_tokens", post(count_tokens_handler))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(body: &str) -> MessagesRequest {
        serde_json::from_str(body).expect("request must parse")
    }

    #[test]
    fn accepts_the_shape_claude_code_actually_sends() {
        // system as a block array with cache_control, tools, and unknown
        // top-level fields — all of which must be tolerated, not rejected.
        let req = parse(
            r#"{
              "model": "z-ai/glm-5.3-flash",
              "max_tokens": 8192,
              "system": [{"type":"text","text":"You are helpful.","cache_control":{"type":"ephemeral"}}],
              "messages": [{"role":"user","content":[{"type":"text","text":"hi"}]}],
              "tools": [{"name":"Read","description":"read a file","input_schema":{"type":"object"}}],
              "metadata": {"user_id":"abc"},
              "top_k": 5,
              "stream": true
            }"#,
        );
        let llm = to_llm_request(&req);
        assert_eq!(llm.system.as_deref(), Some("You are helpful."));
        assert_eq!(llm.max_tokens, Some(8192));
        assert_eq!(llm.tools.len(), 1);
        assert!(req.stream);
    }

    #[test]
    fn a_missing_max_tokens_defaults_rather_than_failing() {
        let req = parse(r#"{"model":"m","messages":[{"role":"user","content":"hi"}]}"#);
        assert_eq!(req.max_tokens, DEFAULT_MAX_TOKENS);
    }

    #[test]
    fn system_accepts_both_a_string_and_a_block_list() {
        let as_string = parse(r#"{"model":"m","system":"plain","messages":[]}"#);
        assert_eq!(to_llm_request(&as_string).system.as_deref(), Some("plain"));

        let as_blocks = parse(
            r#"{"model":"m","system":[{"type":"text","text":"a"},{"type":"text","text":"b"}],"messages":[]}"#,
        );
        assert_eq!(to_llm_request(&as_blocks).system.as_deref(), Some("a\n\nb"));
    }

    #[test]
    fn tool_result_flattens_arrays_and_marks_errors() {
        let req = parse(
            r#"{"model":"m","messages":[{"role":"user","content":[
                {"type":"tool_result","tool_use_id":"t1","content":[{"type":"text","text":"ok"}]},
                {"type":"tool_result","tool_use_id":"t2","content":"boom","is_error":true}
            ]}]}"#,
        );
        let llm = to_llm_request(&req);
        let MessagePart::Parts(blocks) = &llm.messages[0].content else {
            panic!("expected block content");
        };
        match &blocks[0] {
            ContentBlock::ToolResult { content, .. } => assert_eq!(content, "ok"),
            other => panic!("unexpected block: {other:?}"),
        }
        match &blocks[1] {
            // ContentBlock::ToolResult carries no `is_error`, so the flag is
            // preserved in the text rather than dropped.
            ContentBlock::ToolResult { content, .. } => {
                assert!(content.starts_with("[tool error] "), "{content}");
            }
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn an_image_block_degrades_visibly_instead_of_vanishing() {
        let req = parse(
            r#"{"model":"m","messages":[{"role":"user","content":[
                {"type":"image","source":{"type":"base64","media_type":"image/png","data":"AAAA"}}
            ]}]}"#,
        );
        let llm = to_llm_request(&req);
        let MessagePart::Parts(blocks) = &llm.messages[0].content else {
            panic!("expected block content");
        };
        match &blocks[0] {
            ContentBlock::Text { text } => assert!(text.contains("image omitted")),
            other => panic!("unexpected block: {other:?}"),
        }
    }

    #[test]
    fn an_unknown_block_type_does_not_break_the_request() {
        let req = parse(
            r#"{"model":"m","messages":[{"role":"user","content":[
                {"type":"thinking","thinking":"..."},
                {"type":"text","text":"hi"}
            ]}]}"#,
        );
        let llm = to_llm_request(&req);
        let MessagePart::Parts(blocks) = &llm.messages[0].content else {
            panic!("expected block content");
        };
        assert_eq!(blocks.len(), 1, "the unknown block is skipped, not fatal");
    }

    #[test]
    fn tool_use_blocks_force_the_tool_use_stop_reason() {
        // This is the one that decides whether Claude Code *runs* a tool or
        // just prints it. Provider finish_reason is inconsistent across
        // OpenRouter models, so block presence has to win.
        assert_eq!(map_stop_reason(Some("stop"), true), "tool_use");
        assert_eq!(map_stop_reason(None, true), "tool_use");
        assert_eq!(map_stop_reason(Some("tool_calls"), false), "tool_use");
        assert_eq!(map_stop_reason(Some("function_call"), false), "tool_use");
    }

    #[test]
    fn other_stop_reasons_map_to_the_anthropic_vocabulary() {
        assert_eq!(map_stop_reason(Some("stop"), false), "end_turn");
        assert_eq!(map_stop_reason(Some("length"), false), "max_tokens");
        assert_eq!(map_stop_reason(Some("max_tokens"), false), "max_tokens");
        assert_eq!(
            map_stop_reason(Some("stop_sequence"), false),
            "stop_sequence"
        );
        assert_eq!(map_stop_reason(None, false), "end_turn");
        assert_eq!(map_stop_reason(Some("content_filter"), false), "end_turn");
    }

    #[test]
    fn token_estimates_are_never_zero() {
        // A zero input_tokens in message_start means Claude Code never fires
        // auto-compact, the transcript grows unbounded, and the provider
        // eventually rejects the request on context length.
        let req = parse(r#"{"model":"m","messages":[]}"#);
        assert!(estimate_request_tokens(&to_llm_request(&req)) >= 1);

        let with_text =
            parse(r#"{"model":"m","messages":[{"role":"user","content":"hello there friend"}]}"#);
        assert!(estimate_request_tokens(&to_llm_request(&with_text)) >= 4);
    }

    #[test]
    fn message_start_carries_a_full_envelope() {
        let v = message_start("msg_1", "some-model", 42);
        let msg = &v["message"];
        assert_eq!(msg["type"], "message");
        assert_eq!(msg["role"], "assistant");
        assert_eq!(msg["model"], "some-model");
        assert_eq!(msg["usage"]["input_tokens"], 42);
        assert!(msg["content"].is_array());
        assert!(msg["stop_reason"].is_null());
    }

    #[test]
    fn synthetic_events_follow_the_anthropic_order() {
        let content = vec![
            ContentBlock::Text {
                text: "hello".into(),
            },
            ContentBlock::ToolUse {
                id: "tu_1".into(),
                name: "Read".into(),
                input: json!({"path": "/tmp/x"}),
            },
        ];
        let events = synth_events(&content);
        let names: Vec<&str> = events
            .iter()
            .map(|e| e["event"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(
            names,
            vec![
                "content_block_start",
                "content_block_delta",
                "content_block_stop",
                "content_block_start",
                "content_block_delta",
                "content_block_stop",
            ]
        );
        // The tool block must announce itself as tool_use, with its id/name.
        assert_eq!(events[3]["data"]["content_block"]["type"], "tool_use");
        assert_eq!(events[3]["data"]["content_block"]["id"], "tu_1");
        assert_eq!(events[4]["data"]["delta"]["type"], "input_json_delta");
    }

    #[test]
    fn roles_map_onto_the_internal_enum() {
        let req = parse(
            r#"{"model":"m","messages":[
                {"role":"user","content":"a"},
                {"role":"assistant","content":"b"},
                {"role":"weird","content":"c"}
            ]}"#,
        );
        let llm = to_llm_request(&req);
        assert!(matches!(llm.messages[0].role, ChatRole::User));
        assert!(matches!(llm.messages[1].role, ChatRole::Assistant));
        // An unknown role degrades to user rather than dropping the message.
        assert!(matches!(llm.messages[2].role, ChatRole::User));
    }
}

#[cfg(test)]
mod usage_tests {
    use super::*;

    #[test]
    fn a_reported_zero_falls_back_to_the_estimate() {
        // Providers that omit accounting report zero rather than None. Passing
        // that through pins the client's context meter at empty forever.
        assert_eq!(non_zero_or(Some(0), 42), 42);
        assert_eq!(non_zero_or(None, 42), 42);
        assert_eq!(non_zero_or(Some(7), 42), 7);
    }
}
