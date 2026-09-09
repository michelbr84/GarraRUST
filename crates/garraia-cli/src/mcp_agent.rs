//! `garra_agent` — full-agent MCP tool (opt-in), companion to the
//! LLM-only `garra_ask` (GAR-583).
//!
//! # Divergencia deliberada dos invariantes do `mcp_server.rs`
//!
//! O `mcp_server.rs` e auditado por dois testes que varrem a propria
//! producao e proibem: registrar ferramentas de runtime, spawnar
//! subprocessos e escrever no stdout. Este modulo NAO e escaneado por
//! aqueles testes — e por desenho: aqui o agente registra ferramentas
//! de verdade (`bash`, `file_read`, `file_write`, `web_fetch`,
//! `git_diff`, `web_search` opcional) e o `bash` spawna processos via
//! o `BashTool` nativo do runtime (mesmo safety_gate do gateway).
//! A auditoria continua valendo para `mcp_server.rs` e `ask.rs`, que
//! permanecem LLM-only; este modulo existe para o caso em que o
//! operador QUER o agente completo via MCP.
//!
//! # Opt-in do operador
//!
//! A tool so e anunciada em `tools/list` e despachada em `tools/call`
//! quando o processo do servidor MCP inicia com
//! `GARRAIA_MCP_ENABLE_TOOLS` em {1, true, yes}. Sem a env, a
//! superficie e identica a de antes: so `garra_ask`, e `garra_agent`
//! responde "unknown tool".
//!
//! # Risco aceito e documentado
//!
//! O bash roda sem canal de confirmacao (impossivel em chamada MCP
//! stateless) — com o hardening #1075, o tier de risco e fail-closed:
//! comandos Sensiveis/risky (DENY_LIST, CONFIRM_LIST, programas
//! exfiltraveis, interpolacao de env) sao BLOQUEADOS, nao auto-executam.
//! E o processo filho herda apenas a allowlist de env do R3
//! (`R3_ENV_ALLOWLIST`), nunca o ambiente cru do servidor MCP. Ver
//! `docs/cli-mcp-server.md` para a superficie restante.

use std::sync::Arc;
use std::time::{Duration, Instant};

use garraia_agents::exec_context::ExecContext;
use garraia_agents::tools::git_diff_tool::GitDiffTool;
use garraia_agents::{
    AgentRuntime, BashTool, ChatMessage, FileReadTool, FileWriteTool, TurnEvent, WebFetchTool,
    WebSearchTool,
};
use garraia_config::AppConfig;
use rmcp::model::Tool;
use serde::Deserialize;
use serde_json::{Map as JsonMap, Value as JsonValue, json};

use crate::ask::{AskError, sanitize_provider_error};
use crate::chat;
use crate::mcp_server::{PROVIDER_ENUM, ServerPolicy, resolve_overrides, validate_agent_policy};

/// 64 KiB cap on `message` — same bound as `garra_ask` (`crate::ask::STDIN_CAP_BYTES`).
const AGENT_MESSAGE_MAX_BYTES: usize = 64 * 1024;
/// 8 KiB cap on the `system_prompt` override — same bound as `garra_ask`.
const AGENT_SYSTEM_PROMPT_MAX_BYTES: usize = 8 * 1024;

/// Timeout bounds for the agent call. The wall clock covers the ENTIRE
/// agent loop — every LLM round-trip and every tool execution — so the
/// range is wider than `garra_ask`'s [1, 600]: multi-step tool work
/// legitimately runs for minutes. The operator cap
/// (`GARRAIA_MCP_MAX_TIMEOUT_SECS`) applies on top, clamped to [1, 1800].
pub(crate) const AGENT_TIMEOUT_SECS_MIN: u64 = 5;
pub(crate) const AGENT_TIMEOUT_SECS_MAX: u64 = 1800;
pub(crate) const AGENT_TIMEOUT_SECS_DEFAULT: u64 = 300;

/// Belt-and-braces cap on the per-tool-call summary that reaches the
/// envelope. The runtime already truncates to 72 chars at the source
/// (`turn_events::SUMMARY_MAX_CHARS`); this only guards against that
/// invariant changing out from under us.
const TOOL_CALL_SUMMARY_MAX_CHARS: usize = 160;

/// How long to keep draining the event channel after the turn future
/// resolves (success or timeout). The producer side is closed in both
/// paths, so this normally returns immediately; the bound only matters
/// if some background task still holds a sender clone — then we take
/// whatever arrived and move on with partial observability.
const EVENT_DRAIN_CAP: Duration = Duration::from_millis(500);

/// `garra_agent` — argument shape. `deny_unknown_fields` mirrors the
/// `additionalProperties: false` constraint in the tool descriptor's
/// JSON schema (same contract as `GarraAskArgs`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GarraAgentArgs {
    pub message: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    /// Directory against which file-tool relative paths resolve, and in
    /// which the bash child runs (current_dir — #1075 R3). Validated for
    /// existence only, NOT against GARRAIA_MCP_ALLOWED_DIRS; bash is an
    /// unsandboxed shell on the server host, so absolute paths reach
    /// outside this directory.
    #[serde(default)]
    pub working_dir: Option<String>,
}

/// Fully-validated agent options — defaults applied, policy checked.
#[derive(Debug, Clone)]
pub(crate) struct AgentOptions {
    pub message: String,
    pub provider: String,
    pub model: String,
    pub timeout_secs: u64,
    pub system_prompt: Option<String>,
    pub working_dir: Option<String>,
}

/// One executed tool call, as reported to the MCP host in the envelope.
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct ToolCallSummary {
    pub name: String,
    pub duration_ms: u64,
    pub success: bool,
    pub summary: String,
}

/// Pure outcome of the agent one-shot. `tool_calls` travels on BOTH
/// branches so the host keeps partial observability when the run fails
/// or times out mid-loop.
#[derive(Debug, Clone)]
pub(crate) enum AgentOutcome {
    Success {
        answer: String,
        provider: String,
        model: String,
        latency_ms: u128,
        session_id: String,
        tool_calls: Vec<ToolCallSummary>,
    },
    Failure {
        error: AskError,
        provider: String,
        model: String,
        tool_calls: Vec<ToolCallSummary>,
    },
}

impl AgentOutcome {
    pub(crate) fn is_ok(&self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// Build the `garra.agent.v1` JSON envelope for this outcome.
    pub(crate) fn to_envelope(&self) -> serde_json::Value {
        match self {
            Self::Success {
                answer,
                provider,
                model,
                latency_ms,
                session_id,
                tool_calls,
            } => {
                agent_success_envelope(answer, provider, model, *latency_ms, session_id, tool_calls)
            }
            Self::Failure {
                error,
                provider,
                model,
                tool_calls,
            } => agent_error_envelope(
                error.kind_str(),
                &error.message(),
                provider,
                model,
                tool_calls,
            ),
        }
    }
}

/// `garra.agent.v1` — success envelope. Same shape as `garra.ask.v1`
/// plus `session_id` and the `tool_calls` summary.
pub(crate) fn agent_success_envelope(
    answer: &str,
    provider: &str,
    model: &str,
    latency_ms: u128,
    session_id: &str,
    tool_calls: &[ToolCallSummary],
) -> serde_json::Value {
    json!({
        "schema": "garra.agent.v1",
        "ok": true,
        "answer": answer,
        "provider": provider,
        "model": model,
        "latency_ms": latency_ms,
        "session_id": session_id,
        "tool_calls": tool_calls,
    })
}

/// `garra.agent.v1` — error envelope. `kind` reuses the stable
/// `AskError` labels (usage|no_provider|provider_error|timeout|io).
pub(crate) fn agent_error_envelope(
    kind: &str,
    message: &str,
    provider: &str,
    model: &str,
    tool_calls: &[ToolCallSummary],
) -> serde_json::Value {
    json!({
        "schema": "garra.agent.v1",
        "ok": false,
        "provider": provider,
        "model": model,
        "tool_calls": tool_calls,
        "error": {
            "kind": kind,
            "message": message,
        }
    })
}

/// GAR-583 sibling — build the `garra_agent` tool descriptor (advertised
/// in `tools/list` only when the opt-in env is set).
pub(crate) fn garra_agent_tool() -> Tool {
    let schema_value = json!({
        "type": "object",
        "properties": {
            "message": {
                "type": "string",
                "description": "The task or instruction for the full agent. Max 64 KiB.",
                "minLength": 1,
                "maxLength": AGENT_MESSAGE_MAX_BYTES
            },
            "provider": {
                "type": "string",
                "enum": PROVIDER_ENUM,
                "default": "openrouter",
                "description": "LLM provider. Default 'openrouter'."
            },
            "model": {
                "type": "string",
                "default": "openrouter/free",
                "description": "Model name. Default 'openrouter/free'. Pass 'openrouter/auto' explicitly for complex tasks — never automatic."
            },
            "timeout_secs": {
                "type": "integer",
                "default": AGENT_TIMEOUT_SECS_DEFAULT,
                "minimum": AGENT_TIMEOUT_SECS_MIN,
                "maximum": AGENT_TIMEOUT_SECS_MAX,
                "description": "Wall-clock cap for the ENTIRE agent loop including tool execution. Range [5, 1800]. Default 300."
            },
            "system_prompt": {
                "type": "string",
                "maxLength": AGENT_SYSTEM_PROMPT_MAX_BYTES,
                "description": "Optional system prompt override. If omitted, a default generated from the registered tools is used."
            },
            "working_dir": {
                "type": "string",
                "description": "Directory against which file tools resolve relative paths and in which the bash child runs. Validated for existence only — not against allowed_dirs; bash is an unsandboxed shell."
            }
        },
        "required": ["message"],
        "additionalProperties": false
    });
    // Dev/CI only: mirrors the feature-gated "echo" entry of `garra_ask`
    // (PR #859) so the full agent can be smoke-tested without an API key.
    #[cfg(feature = "dev-echo-provider")]
    let schema_value = {
        let mut v = schema_value;
        if let Some(e) = v
            .pointer_mut("/properties/provider/enum")
            .and_then(|e| e.as_array_mut())
        {
            e.push(json!("echo"));
        }
        v
    };
    // serde_json::Value::Object guaranteed by the literal above.
    let schema_map: JsonMap<String, JsonValue> = match schema_value {
        JsonValue::Object(map) => map,
        _ => unreachable!("schema literal is an object"),
    };
    Tool::new(
        "garra_agent",
        "Run the GarraIA assistant as a FULL AGENT with tool access (shell, files, git, web) — one-shot, fresh session per call. Unlike garra_ask (LLM-only), this tool can execute real commands and read/write files on the operator's machine. Opt-in: only advertised when GARRAIA_MCP_ENABLE_TOOLS is set on the server process. Returns a `garra.agent.v1` JSON envelope as text content, including a tool_calls summary.",
        Arc::new(schema_map),
    )
}

/// Validate `GarraAgentArgs` bounds. Same error shape as
/// `mcp_server::validate_args` — the MCP host sees it in
/// `CallToolResult` as `invalid_params`.
pub(crate) fn validate_agent_args(args: &GarraAgentArgs) -> Result<(), String> {
    if args.message.trim().is_empty() {
        return Err("message must be non-empty".to_string());
    }
    if args.message.len() > AGENT_MESSAGE_MAX_BYTES {
        return Err(format!(
            "message exceeds 64 KiB cap ({AGENT_MESSAGE_MAX_BYTES} bytes)"
        ));
    }
    if let Some(ts) = args.timeout_secs
        && !(AGENT_TIMEOUT_SECS_MIN..=AGENT_TIMEOUT_SECS_MAX).contains(&ts)
    {
        return Err(format!(
            "timeout_secs out of range [{AGENT_TIMEOUT_SECS_MIN}, {AGENT_TIMEOUT_SECS_MAX}]"
        ));
    }
    if let Some(ref sp) = args.system_prompt
        && sp.len() > AGENT_SYSTEM_PROMPT_MAX_BYTES
    {
        return Err(format!(
            "system_prompt exceeds {AGENT_SYSTEM_PROMPT_MAX_BYTES}-byte cap"
        ));
    }
    Ok(())
}

/// Directories the file tools may touch, from the operator's
/// `GARRAIA_MCP_ALLOWED_DIRS` (comma-separated); falls back to the
/// server process CWD. Fail-open to `None` (whole filesystem) when the
/// CWD cannot be read — unrestricted `bash` is the real boundary
/// anyway (documented: `allowed_dirs` is a UX belt, not a boundary).
fn allowed_dirs() -> Option<Vec<std::path::PathBuf>> {
    if let Ok(raw) = std::env::var("GARRAIA_MCP_ALLOWED_DIRS") {
        let dirs: Vec<std::path::PathBuf> = raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(std::path::PathBuf::from)
            .collect();
        if !dirs.is_empty() {
            return Some(dirs);
        }
    }
    match std::env::current_dir() {
        Ok(cwd) => Some(vec![cwd]),
        Err(e) => {
            tracing::warn!("cannot resolve CWD for file tools allowed_dirs: {e}");
            None
        }
    }
}

/// Register the same tool set the gateway bootstrap wires
/// (`garraia-gateway/src/bootstrap/mod.rs`): bash (DENY_LIST + risky tier
/// fail-closed — no confirmation channel in a stateless MCP call, #1075
/// R1), file read/write,
/// web fetch, git diff, and web search when a Brave key is available.
/// `ListDirTool` is skipped on purpose: `bash ls` + the file tools
/// cover it, and a tighter tool list helps weaker models route.
fn build_tools(config: &AppConfig) -> Vec<Box<dyn garraia_agents::Tool>> {
    let dirs = allowed_dirs();
    let mut tools: Vec<Box<dyn garraia_agents::Tool>> = vec![
        Box::new(BashTool::new(None)),
        Box::new(FileReadTool::new(dirs.clone())),
        Box::new(FileWriteTool::new(dirs)),
        Box::new(WebFetchTool::new(None)),
        Box::new(GitDiffTool::new(None, None)),
    ];
    let brave_config_key = config.llm.get("brave").and_then(|c| c.api_key.clone());
    let brave = brave_config_key
        .or_else(|| std::env::var("BRAVE_API_KEY").ok())
        .filter(|k| !k.trim().is_empty());
    if let Some(key) = brave {
        tools.push(Box::new(WebSearchTool::new(key)));
    }
    tools
}

/// Default system prompt, generated from the tools actually registered
/// (name + description of each), the effective working dir, and the
/// bash-CWD caveat. Structure mirrors the CLI chat recipe
/// (`chat.rs`) — weak models only call tools when the prompt names them.
pub(crate) fn agent_system_prompt(tools: &[(String, String)], working_dir: Option<&str>) -> String {
    let mut prompt = String::from(
        "Voce e o GarraIA, um assistente pessoal de IA criado em Rust. \
         Voce esta rodando como agente COMPLETO via MCP e pode executar \
         ferramentas de verdade (shell, arquivos, git, web) para cumprir a tarefa. \
         Seja prestativo, conciso e amigavel. Responda no idioma do usuario.\n\n\
         ## Ferramentas disponiveis\n\
         Voce tem acesso a estas ferramentas que pode usar quando necessario:\n",
    );
    for (name, description) in tools {
        prompt.push_str(&format!("- **{name}**: {}\n", description.trim()));
    }
    prompt.push_str(
        "\nIMPORTANTE: Quando a tarefa envolver arquivos, comandos ou dados reais, \
         USE as ferramentas para investigar em vez de apenas descrever. \
         Use 'bash' com 'ls' para listar arquivos e 'file_read' para ler conteudo. \
         Nao invente resultados — execute e reporte o que aconteceu de verdade. \
         Se uma ferramenta falhar ou for bloqueada, relate isso na resposta \
         final: nunca reporte sucesso sem saida real e nunca contorne \
         silenciosamente um bloqueio de seguranca.\n",
    );
    if let Some(dir) = working_dir {
        prompt.push_str(&format!(
            "\n## Contexto do diretorio\n\
             O chamador indicou o diretorio de trabalho: {dir}\n\
             As ferramentas de arquivo (file_read/file_write) resolvem caminhos \
             relativos contra este diretorio e o 'bash' EXECUTA nele (current_dir, \
             hardening #1075). O campo e validado apenas quanto a existencia — \
             caminhos absolutos no bash alcancam fora dele (sem sandbox).\n"
        ));
    }
    prompt
}

/// Pure reducer over the turn events: one envelope summary per finished
/// tool call, in emission order. `ToolStarted`/`TextDelta` are skipped;
/// summaries are re-truncated defensively (the runtime already redacts
/// and truncates at the source).
pub(crate) fn events_to_summaries(events: &[TurnEvent]) -> Vec<ToolCallSummary> {
    events
        .iter()
        .filter_map(|e| match e {
            TurnEvent::ToolFinished {
                name,
                duration,
                success,
                summary,
                ..
            } => Some(ToolCallSummary {
                name: name.clone(),
                duration_ms: u64::try_from(duration.as_millis()).unwrap_or(u64::MAX),
                success: *success,
                summary: truncate_chars(summary, TOOL_CALL_SUMMARY_MAX_CHARS),
            }),
            _ => None,
        })
        .collect()
}

/// Truncate by chars (never bytes — paths and summaries carry accents),
/// mirroring `turn_events::truncar`.
fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Full-agent one-shot: resolve the provider, build a fresh
/// `AgentRuntime` WITH tools, run a single turn on an empty history,
/// and return the outcome with the tool-call summary.
///
/// Shape mirrors [`crate::ask::ask_oneshot`]. Differences by design:
/// tools are registered (the caller opted in via env), the events
/// channel carries the full turn flow, and the wall-clock timeout
/// covers the whole loop.
async fn agent_oneshot(config: &AppConfig, opts: &AgentOptions) -> AgentOutcome {
    let start = Instant::now();

    // 1. Resolve provider (same pipeline as `garra_ask`).
    let (provider_name, model_name, provider) =
        match chat::select_explicit_provider(config, &opts.provider, Some(&opts.model), None) {
            Ok(triple) => triple,
            Err(e) => {
                return AgentOutcome::Failure {
                    error: AskError::NoProvider(sanitize_provider_error(&format!("{e:#}"))),
                    provider: opts.provider.clone(),
                    model: opts.model.clone(),
                    tool_calls: Vec::new(),
                };
            }
        };

    // 2. Build the runtime: provider + full tool set. `register_tool`
    //    takes `&self` (Arc-safe); the `&mut` setters must run before.
    let tools = build_tools(config);
    let tool_pairs: Vec<(String, String)> = tools
        .iter()
        .map(|t| (t.name().to_string(), t.description().to_string()))
        .collect();
    let mut runtime = AgentRuntime::new();
    runtime.register_provider(provider);
    for tool in tools {
        runtime.register_tool(tool);
    }
    runtime.set_system_prompt(agent_system_prompt(
        &tool_pairs,
        opts.working_dir.as_deref(),
    ));
    runtime.set_max_tokens(4096);

    // 3. One-shot call: fresh session, empty history, wall-clock timeout
    //    over the whole loop. The events channel gives us the tool
    //    lifecycle for the envelope's `tool_calls` summary.
    let session_id = format!("mcp-{}", uuid::Uuid::new_v4());
    let history: Vec<ChatMessage> = Vec::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnEvent>(512);
    let exec = ExecContext::with_working_dir(opts.working_dir.clone());

    let call = runtime.process_message_streaming_with_events(
        &session_id,
        &opts.message,
        &history,
        tx,
        None,
        None,
        Some(&provider_name),
        Some(&model_name),
        opts.system_prompt.as_deref(),
        None,
        &exec,
    );
    let result = tokio::time::timeout(Duration::from_secs(opts.timeout_secs), call).await;
    let latency_ms = start.elapsed().as_millis();

    // Drain remaining events. The producer side is closed in both paths
    // (turn finished and dropped its sender clone; or the future was
    // dropped by the timeout) — `recv` returns `None` immediately. The
    // cap only bites if a background task still holds a clone.
    let mut events: Vec<TurnEvent> = Vec::new();
    let _ = tokio::time::timeout(EVENT_DRAIN_CAP, async {
        while let Some(e) = rx.recv().await {
            events.push(e);
        }
    })
    .await;
    let tool_calls = events_to_summaries(&events);

    match result {
        Ok(Ok(answer)) => AgentOutcome::Success {
            answer,
            provider: provider_name,
            model: model_name,
            latency_ms,
            session_id,
            tool_calls,
        },
        Ok(Err(e)) => AgentOutcome::Failure {
            error: AskError::ProviderError(sanitize_provider_error(&format!("{e:#}"))),
            provider: provider_name,
            model: model_name,
            tool_calls,
        },
        Err(_elapsed) => AgentOutcome::Failure {
            error: AskError::Timeout(opts.timeout_secs),
            provider: provider_name,
            model: model_name,
            tool_calls,
        },
    }
}

/// MCP handler for `tools/call garra_agent`. Validation errors that are
/// the CALLER's fault (bad `working_dir`, policy violations) come back
/// as `Err` → the dispatcher maps them to `invalid_params`; runtime
/// outcomes (including provider errors and timeouts) come back as the
/// `garra.agent.v1` envelope + `is_ok` flag, packed like the
/// `garra_ask` arm.
pub(crate) async fn handle_agent_call(
    config: &AppConfig,
    policy: &ServerPolicy,
    args: GarraAgentArgs,
) -> Result<(serde_json::Value, bool), String> {
    // GAR-587 parity: apply schema defaults before the policy check.
    let (provider, model) = resolve_overrides(args.provider, args.model);
    let timeout_secs = args
        .timeout_secs
        .unwrap_or_else(|| policy.agent_default_timeout_secs());
    validate_agent_policy(config, policy, &provider, &model, timeout_secs)?;
    if let Some(ref dir) = args.working_dir {
        let is_dir = std::fs::metadata(dir).map(|m| m.is_dir()).unwrap_or(false);
        if !is_dir {
            return Err(format!(
                "working_dir does not exist or is not a directory: {dir}"
            ));
        }
    }
    let opts = AgentOptions {
        message: args.message,
        provider,
        model,
        timeout_secs,
        system_prompt: args.system_prompt,
        working_dir: args.working_dir,
    };
    let outcome = agent_oneshot(config, &opts).await;
    let envelope = outcome.to_envelope();
    Ok((envelope, outcome.is_ok()))
}

#[cfg(test)]
mod tests {
    //! Pure tests. Zero network, zero env-mutation, zero filesystem.

    use super::*;

    // ─── Tool descriptor ──────────────────────────────────────────────

    #[test]
    fn tool_descriptor_name_is_garra_agent() {
        let t = garra_agent_tool();
        assert_eq!(t.name.as_ref(), "garra_agent");
    }

    #[test]
    fn tool_descriptor_describes_full_agent_and_envelope() {
        let t = garra_agent_tool();
        let desc = t.description.expect("description present");
        assert!(desc.contains("FULL AGENT"));
        assert!(desc.contains("garra.agent.v1"));
        assert!(desc.contains("garra_ask"));
        assert!(desc.contains("GARRAIA_MCP_ENABLE_TOOLS"));
    }

    #[test]
    fn tool_descriptor_message_is_required() {
        let t = garra_agent_tool();
        let schema = (*t.input_schema).clone();
        let required = schema
            .get("required")
            .and_then(|v| v.as_array())
            .cloned()
            .expect("required array present");
        assert!(required.iter().any(|v| v.as_str() == Some("message")));
    }

    #[test]
    fn tool_descriptor_timeout_range_is_5_to_1800_default_300() {
        let t = garra_agent_tool();
        let schema = (*t.input_schema).clone();
        let ts = schema
            .get("properties")
            .and_then(|p| p.get("timeout_secs"))
            .cloned()
            .expect("timeout_secs property present");
        assert_eq!(ts.get("minimum").and_then(|v| v.as_u64()), Some(5));
        assert_eq!(ts.get("maximum").and_then(|v| v.as_u64()), Some(1800));
        assert_eq!(ts.get("default").and_then(|v| v.as_u64()), Some(300));
    }

    #[test]
    fn tool_descriptor_default_model_is_openrouter_free() {
        let t = garra_agent_tool();
        let schema = (*t.input_schema).clone();
        let model_default = schema
            .get("properties")
            .and_then(|p| p.get("model"))
            .and_then(|m| m.get("default"))
            .and_then(|d| d.as_str());
        assert_eq!(model_default, Some("openrouter/free"));
    }

    #[test]
    fn tool_descriptor_has_working_dir() {
        let t = garra_agent_tool();
        let schema = (*t.input_schema).clone();
        let wd = schema
            .get("properties")
            .and_then(|p| p.get("working_dir"))
            .cloned()
            .expect("working_dir property present");
        assert_eq!(wd.get("type").and_then(|v| v.as_str()), Some("string"));
    }

    #[test]
    fn tool_descriptor_additional_properties_is_false() {
        let t = garra_agent_tool();
        let schema = (*t.input_schema).clone();
        let ap = schema.get("additionalProperties").and_then(|v| v.as_bool());
        assert_eq!(ap, Some(false));
    }

    // ─── Argument deserialization ─────────────────────────────────────

    fn parse_args(v: serde_json::Value) -> Result<GarraAgentArgs, String> {
        serde_json::from_value(v).map_err(|e| e.to_string())
    }

    #[test]
    fn args_deserialize_minimum_required() {
        let a = parse_args(json!({"message": "list files"})).unwrap();
        assert_eq!(a.message, "list files");
        assert!(a.provider.is_none());
        assert!(a.model.is_none());
        assert!(a.timeout_secs.is_none());
        assert!(a.system_prompt.is_none());
        assert!(a.working_dir.is_none());
    }

    #[test]
    fn args_reject_additional_properties() {
        let err = parse_args(json!({"message": "hi", "rogue": "x"})).unwrap_err();
        assert!(
            err.contains("rogue") || err.contains("unknown field"),
            "{err}"
        );
    }

    #[test]
    fn args_explicit_model_accepted_at_deser_layer() {
        let a = parse_args(json!({"message": "x", "model": "openrouter/auto"})).unwrap();
        assert_eq!(a.model.as_deref(), Some("openrouter/auto"));
    }

    #[test]
    fn args_working_dir_deserializes() {
        let a = parse_args(json!({"message": "x", "working_dir": "/tmp"})).unwrap();
        assert_eq!(a.working_dir.as_deref(), Some("/tmp"));
    }

    // ─── Argument validation ──────────────────────────────────────────

    fn make_args(message: &str, timeout_secs: Option<u64>) -> GarraAgentArgs {
        GarraAgentArgs {
            message: message.to_string(),
            provider: None,
            model: None,
            timeout_secs,
            system_prompt: None,
            working_dir: None,
        }
    }

    #[test]
    fn validate_agent_args_rejects_empty_message() {
        let err = validate_agent_args(&make_args("   ", None)).unwrap_err();
        assert!(err.contains("non-empty"));
    }

    #[test]
    fn validate_agent_args_rejects_message_over_64kib() {
        let err = validate_agent_args(&make_args(&"a".repeat(AGENT_MESSAGE_MAX_BYTES + 1), None))
            .unwrap_err();
        assert!(err.contains("64"));
    }

    #[test]
    fn validate_agent_args_timeout_bounds_are_5_to_1800() {
        assert!(validate_agent_args(&make_args("x", Some(4))).is_err());
        assert!(validate_agent_args(&make_args("x", Some(1801))).is_err());
        assert!(validate_agent_args(&make_args("x", Some(5))).is_ok());
        assert!(validate_agent_args(&make_args("x", Some(1800))).is_ok());
    }

    #[test]
    fn validate_agent_args_rejects_system_prompt_over_8kib() {
        let mut a = make_args("x", None);
        a.system_prompt = Some("a".repeat(AGENT_SYSTEM_PROMPT_MAX_BYTES + 1));
        let err = validate_agent_args(&a).unwrap_err();
        assert!(err.contains("system_prompt"));
    }

    // ─── events_to_summaries (pure reducer) ───────────────────────────

    fn finished(name: &str, ms: u64, success: bool, summary: &str) -> TurnEvent {
        TurnEvent::ToolFinished {
            name: name.to_string(),
            duration: Duration::from_millis(ms),
            success,
            summary: summary.to_string(),
            output: String::new(),
        }
    }

    #[test]
    fn events_to_summaries_keeps_only_tool_finished_in_order() {
        let events = vec![
            TurnEvent::TextDelta("oi ".into()),
            TurnEvent::ToolStarted {
                name: "bash".into(),
                detail: "ls".into(),
            },
            finished("bash", 120, true, "3 files"),
            finished("file_read", 5, true, "conteudo"),
        ];
        let got = events_to_summaries(&events);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].name, "bash");
        assert_eq!(got[0].duration_ms, 120);
        assert!(got[0].success);
        assert_eq!(got[0].summary, "3 files");
        assert_eq!(got[1].name, "file_read");
    }

    #[test]
    fn events_to_summaries_truncates_long_summary() {
        let events = vec![finished("bash", 1, true, &"ç".repeat(200))];
        let got = events_to_summaries(&events);
        assert!(got[0].summary.chars().count() <= TOOL_CALL_SUMMARY_MAX_CHARS);
        assert!(got[0].summary.ends_with('…'));
    }

    #[test]
    fn events_to_summaries_passthrough_failure_flag() {
        let events = vec![finished("bash", 30, false, "exit 1")];
        let got = events_to_summaries(&events);
        assert!(!got[0].success);
    }

    #[test]
    fn events_to_summaries_empty_on_no_events() {
        assert!(events_to_summaries(&[]).is_empty());
    }

    // ─── agent_system_prompt ──────────────────────────────────────────

    #[test]
    fn system_prompt_names_every_registered_tool() {
        let pairs = vec![
            ("bash".to_string(), "Executa comandos".to_string()),
            ("file_read".to_string(), "Le arquivos".to_string()),
        ];
        let prompt = agent_system_prompt(&pairs, None);
        assert!(prompt.contains("bash"));
        assert!(prompt.contains("file_read"));
        assert!(prompt.contains("## Ferramentas disponiveis"));
    }

    #[test]
    fn system_prompt_includes_working_dir_and_bash_caveat() {
        let pairs = vec![("bash".to_string(), "Executa comandos".to_string())];
        let prompt = agent_system_prompt(&pairs, Some("/tmp/projeto"));
        assert!(prompt.contains("/tmp/projeto"));
        // #1075 R3: bash EXECUTA no working_dir (nao ignora mais), e o
        // prompt avisa que caminhos absolutos alcancam fora dele.
        assert!(
            prompt.contains("bash' EXECUTA nele"),
            "deve dizer que bash roda no working_dir"
        );
        assert!(prompt.contains("sem sandbox"));
    }

    #[test]
    fn system_prompt_obriga_relatar_falhas_de_ferramenta() {
        let pairs = vec![("bash".to_string(), "Executa comandos".to_string())];
        let prompt = agent_system_prompt(&pairs, None);
        assert!(
            prompt.contains("nunca reporte sucesso sem saida real"),
            "prompt deve obrigar a relatar falhas de ferramenta"
        );
        assert!(
            prompt.contains("bloqueada"),
            "prompt deve cobrir ferramenta bloqueada (ex.: file_write fora de allowed_dirs)"
        );
    }

    // ─── Envelopes (garra.agent.v1) ───────────────────────────────────

    #[test]
    fn agent_success_envelope_shape() {
        let calls = vec![ToolCallSummary {
            name: "bash".into(),
            duration_ms: 120,
            success: true,
            summary: "3 files".into(),
        }];
        let env = agent_success_envelope(
            "feito",
            "openrouter",
            "openrouter/free",
            1500,
            "mcp-abc",
            &calls,
        );
        assert_eq!(env["schema"], "garra.agent.v1");
        assert_eq!(env["ok"], true);
        assert_eq!(env["answer"], "feito");
        assert_eq!(env["provider"], "openrouter");
        assert_eq!(env["model"], "openrouter/free");
        assert_eq!(env["session_id"], "mcp-abc");
        assert_eq!(env["tool_calls"][0]["name"], "bash");
        assert_eq!(env["tool_calls"][0]["duration_ms"], 120);
        assert!(env.get("error").is_none());
    }

    #[test]
    fn agent_error_envelope_carries_tool_calls_and_kind() {
        let calls = vec![ToolCallSummary {
            name: "bash".into(),
            duration_ms: 4000,
            success: true,
            summary: "trabalhando".into(),
        }];
        let env = agent_error_envelope(
            "timeout",
            "exceeded 5s timeout",
            "openrouter",
            "openrouter/free",
            &calls,
        );
        assert_eq!(env["schema"], "garra.agent.v1");
        assert_eq!(env["ok"], false);
        assert_eq!(env["error"]["kind"], "timeout");
        assert_eq!(env["error"]["message"], "exceeded 5s timeout");
        assert_eq!(env["tool_calls"][0]["name"], "bash");
        assert!(env.get("answer").is_none());
        assert!(env.get("session_id").is_none());
    }

    #[test]
    fn agent_error_envelope_redacts_key_fingerprints() {
        let sanitized = sanitize_provider_error("boom sk-abcdefgh12345678");
        let env = agent_error_envelope(
            AskError::ProviderError(sanitized.clone()).kind_str(),
            &AskError::ProviderError(sanitized).message(),
            "openrouter",
            "openrouter/free",
            &[],
        );
        let msg = env["error"]["message"].as_str().unwrap_or_default();
        assert!(msg.contains("sk-[REDACTED]"), "{msg}");
        assert!(!msg.contains("sk-abcdefgh12345678"), "{msg}");
    }
}
