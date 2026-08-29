//! GAR-583 — MCP server exposing `garra ask` as a stdio tool.
//!
//! Implements the `Model Context Protocol` (MCP) `ServerHandler` trait
//! from `rmcp 1.6` and exposes a single tool — `garra_ask` — that calls
//! [`crate::ask::ask_oneshot`] **in-process** (no subprocess spawn, no
//! shell). Designed for Claude Desktop, Claude Code, and any MCP host
//! that speaks stdio JSON-RPC.
//!
//! Scope (locked-in MVP, user-approved 2026-05-11):
//!   - Stdio transport only. No HTTP / Streamable HTTP in this PR.
//!   - One tool: `garra_ask`.
//!   - Default `openrouter/free`; `openrouter/auto` only opt-in.
//!   - Response = full `garra.ask.v1` envelope as MCP text content.
//!   - LLM-only — `mcp_server` MUST NOT register tools, MUST NOT spawn
//!     subprocesses, MUST NOT write to stdout outside of `rmcp`'s
//!     JSON-RPC channel. Two audit tests enforce these invariants at
//!     compile time by scanning the production code of this very file.

use std::sync::Arc;

use anyhow::Result;
use garraia_config::AppConfig;
use rmcp::ErrorData as McpError;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, Content, ListToolsResult, PaginatedRequestParams,
    ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler, ServiceExt};
use serde::Deserialize;
use serde_json::{Map as JsonMap, Value as JsonValue, json};

use crate::ask::{self, AskOptions};

/// 64 KiB cap mirrors `crate::ask::STDIN_CAP_BYTES`.
const ARG_MESSAGE_MAX_BYTES: usize = 64 * 1024;
/// Soft cap on `system_prompt` override; keeps the MCP payload bounded.
const ARG_SYSTEM_PROMPT_MAX_BYTES: usize = 8 * 1024;
/// Timeout range — matches the `garra ask --timeout-secs` schema.
const ARG_TIMEOUT_SECS_MIN: u64 = 1;
const ARG_TIMEOUT_SECS_MAX: u64 = 600;
const ARG_TIMEOUT_SECS_DEFAULT: u64 = 60;

/// GAR-587 — Default provider applied when the MCP caller omits `provider`.
/// Matches the JSON-Schema `default` advertised by `garra_ask_tool`; the
/// schema's `default` is advisory, so MCP hosts do not synthesize it before
/// dispatching `tools/call`. The handler applies it explicitly to honor the
/// advertised contract.
const PROVIDER_DEFAULT: &str = "openrouter";

/// GAR-587 — Default model applied when the MCP caller omits `model`.
/// Stays at `openrouter/free`; `openrouter/auto` remains opt-in (must be
/// passed explicitly by the caller).
const MODEL_DEFAULT: &str = "openrouter/free";

/// Providers accepted at runtime. Mirrors the `enum` advertised in the
/// `garra_ask` JSON schema — which MCP hosts treat as advisory, so it must
/// be enforced here. Provider aliases defined in the user's `config.yml`
/// `llm:` section are additionally accepted (see `validate_policy`).
const PROVIDER_ENUM: [&str; 4] = ["ollama", "anthropic", "openai", "openrouter"];

/// Runtime limits for `garra_ask`, resolved once at server startup.
///
/// Both knobs are opt-in via env vars; when absent, behavior is unchanged
/// (any model accepted, caller timeout honored up to the schema max).
/// This is the operator-side "policy" hook for pairing GarraIA with other
/// agents (e.g. Hermes) without handing them an unbounded spend button.
#[derive(Debug, Clone, Default)]
pub(crate) struct ServerPolicy {
    /// From `GARRAIA_MCP_MODEL_ALLOWLIST` (comma-separated). Empty = any
    /// model. When set, a caller-supplied (or defaulted) model outside the
    /// list is rejected with `invalid_params` — the operator's way to keep
    /// e.g. `openrouter/auto` unreachable from MCP callers.
    model_allowlist: Vec<String>,
    /// From `GARRAIA_MCP_MAX_TIMEOUT_SECS`, clamped to the schema max.
    /// `None` = schema max (600) applies.
    max_timeout_secs: Option<u64>,
}

impl ServerPolicy {
    /// Pure constructor so tests never mutate process env (module
    /// invariant: zero env-mutation in these tests).
    pub(crate) fn from_values(allowlist: Option<&str>, max_timeout: Option<&str>) -> Self {
        let model_allowlist = allowlist
            .map(|raw| {
                raw.split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();
        let max_timeout_secs = max_timeout
            .and_then(|raw| raw.trim().parse::<u64>().ok())
            .map(|v| v.clamp(ARG_TIMEOUT_SECS_MIN, ARG_TIMEOUT_SECS_MAX));
        Self {
            model_allowlist,
            max_timeout_secs,
        }
    }

    pub(crate) fn from_env() -> Self {
        Self::from_values(
            std::env::var("GARRAIA_MCP_MODEL_ALLOWLIST").ok().as_deref(),
            std::env::var("GARRAIA_MCP_MAX_TIMEOUT_SECS")
                .ok()
                .as_deref(),
        )
    }

    /// Effective timeout when the caller omits `timeout_secs`: the schema
    /// default, never above the operator cap.
    pub(crate) fn default_timeout_secs(&self) -> u64 {
        self.max_timeout_secs
            .map_or(ARG_TIMEOUT_SECS_DEFAULT, |cap| {
                cap.min(ARG_TIMEOUT_SECS_DEFAULT)
            })
    }
}

/// Enforce the runtime policy over the already-bounds-checked args.
/// `config` supplies the accepted provider set: the schema enum plus any
/// alias the operator defined under `llm:` in config.yml.
pub(crate) fn validate_policy(
    config: &AppConfig,
    policy: &ServerPolicy,
    provider: &str,
    model: &str,
    timeout_secs: u64,
) -> Result<(), String> {
    // Dev/CI only: mirrors the feature-gated "echo" entry the schema enum
    // gains under `dev-echo-provider` (PR #859) — never in default builds.
    #[cfg(feature = "dev-echo-provider")]
    let feature_gated_ok = provider == "echo";
    #[cfg(not(feature = "dev-echo-provider"))]
    let feature_gated_ok = false;
    if !PROVIDER_ENUM.contains(&provider) && !feature_gated_ok && !config.llm.contains_key(provider)
    {
        return Err(format!(
            "provider '{provider}' not accepted (schema enum {PROVIDER_ENUM:?} or an alias \
             configured under llm: in config.yml)"
        ));
    }
    if !policy.model_allowlist.is_empty() && !policy.model_allowlist.iter().any(|m| m == model) {
        return Err(format!(
            "model '{model}' blocked by GARRAIA_MCP_MODEL_ALLOWLIST"
        ));
    }
    if let Some(cap) = policy.max_timeout_secs
        && timeout_secs > cap
    {
        return Err(format!(
            "timeout_secs {timeout_secs} exceeds operator cap GARRAIA_MCP_MAX_TIMEOUT_SECS={cap}"
        ));
    }
    Ok(())
}

/// GAR-583 — Argument shape for the `garra_ask` MCP tool.
///
/// Deserialized from `CallToolRequestParam.arguments`. `deny_unknown_fields`
/// matches the `additionalProperties: false` constraint in the tool
/// descriptor's JSON schema.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GarraAskArgs {
    pub message: String,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub system_prompt: Option<String>,
}

/// GAR-587 — Apply MCP schema defaults to `(provider, model)` when the
/// caller omitted them. JSON-Schema `default` is advisory; MCP hosts do
/// **not** synthesize missing values, so the handler applies them
/// explicitly to honor the contract advertised by [`garra_ask_tool`].
///
/// Pure function — no I/O, no allocation when both fields are `Some`.
/// `openrouter/auto` is **only** reachable when the caller passes it
/// explicitly; an absent `model` never resolves to `auto`.
pub(crate) fn resolve_overrides(
    provider: Option<String>,
    model: Option<String>,
) -> (String, String) {
    (
        provider.unwrap_or_else(|| PROVIDER_DEFAULT.to_string()),
        model.unwrap_or_else(|| MODEL_DEFAULT.to_string()),
    )
}

/// GAR-583 — Validate `GarraAskArgs` bounds. Returns the same error
/// shape that the MCP host will see in `CallToolResult` on rejection.
pub(crate) fn validate_args(args: &GarraAskArgs) -> Result<(), String> {
    if args.message.trim().is_empty() {
        return Err("message must be non-empty".to_string());
    }
    if args.message.len() > ARG_MESSAGE_MAX_BYTES {
        return Err(format!(
            "message exceeds 64 KiB cap ({ARG_MESSAGE_MAX_BYTES} bytes)"
        ));
    }
    if let Some(ts) = args.timeout_secs
        && !(ARG_TIMEOUT_SECS_MIN..=ARG_TIMEOUT_SECS_MAX).contains(&ts)
    {
        return Err(format!(
            "timeout_secs out of range [{ARG_TIMEOUT_SECS_MIN}, {ARG_TIMEOUT_SECS_MAX}]"
        ));
    }
    if let Some(ref sp) = args.system_prompt
        && sp.len() > ARG_SYSTEM_PROMPT_MAX_BYTES
    {
        return Err(format!(
            "system_prompt exceeds {ARG_SYSTEM_PROMPT_MAX_BYTES}-byte cap"
        ));
    }
    Ok(())
}

/// GAR-583 — Build the `garra_ask` tool descriptor (advertised in
/// response to `tools/list`).
///
/// Schema mirrors the documented contract: `message` required, defaults
/// for `provider`/`model`/`timeout_secs`, `additionalProperties: false`,
/// bounds for `timeout_secs` and string lengths.
pub(crate) fn garra_ask_tool() -> Tool {
    let schema_value = json!({
        "type": "object",
        "properties": {
            "message": {
                "type": "string",
                "description": "The question or instruction to send to GarraIA. Max 64 KiB.",
                "minLength": 1,
                "maxLength": ARG_MESSAGE_MAX_BYTES
            },
            "provider": {
                "type": "string",
                "enum": ["ollama", "anthropic", "openai", "openrouter"],
                "default": "openrouter",
                "description": "LLM provider. Default 'openrouter'."
            },
            "model": {
                "type": "string",
                "default": "openrouter/free",
                "description": "Model name. Default 'openrouter/free' (cheap, suitable for most tasks). Pass 'openrouter/auto' explicitly for complex tasks — never automatic."
            },
            "timeout_secs": {
                "type": "integer",
                "default": ARG_TIMEOUT_SECS_DEFAULT,
                "minimum": ARG_TIMEOUT_SECS_MIN,
                "maximum": ARG_TIMEOUT_SECS_MAX,
                "description": "LLM call timeout in seconds. Range [1, 600]. Default 60."
            },
            "system_prompt": {
                "type": "string",
                "maxLength": ARG_SYSTEM_PROMPT_MAX_BYTES,
                "description": "Optional system prompt override. If omitted, a minimal default is used."
            }
        },
        "required": ["message"],
        "additionalProperties": false
    });
    // Dev/CI only: com a feature `dev-echo-provider`, o enum ganha "echo" —
    // provider keyless que permite testar o garra_ask fim-a-fim sem chave.
    // Nunca aparece em builds de produção (feature default OFF).
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
        "garra_ask",
        "Ask the GarraIA assistant a single question. Non-interactive, LLM-only — no shell, file, or git access. Returns a `garra.ask.v1` JSON envelope as text content.",
        Arc::new(schema_map),
    )
}

/// GAR-583 — handler held by the running MCP server. Wraps the shared
/// `AppConfig` so each `garra_ask` invocation can resolve the provider
/// + model through the same pipeline used by the CLI.
#[derive(Clone)]
pub(crate) struct GarraToolHandler {
    config: Arc<AppConfig>,
    policy: ServerPolicy,
}

impl GarraToolHandler {
    pub(crate) fn with_policy(config: Arc<AppConfig>, policy: ServerPolicy) -> Self {
        Self { config, policy }
    }
}

impl ServerHandler for GarraToolHandler {
    /// GAR-585 — advertise the `tools` capability in the `initialize`
    /// handshake. Without this override, `rmcp 1.6`'s default `get_info`
    /// returns `ServerInfo::default()` whose `capabilities` field is
    /// empty (`{}`); MCP hosts (Claude Code's `/mcp` panel, Claude
    /// Desktop) read that as "no tools" and never call `tools/list`,
    /// leaving `garra_ask` invisible to the model even though the
    /// `list_tools` handler below is wired correctly.
    ///
    /// Mirrors the canonical example in `rmcp-1.6.0/tests/common/
    /// calculator.rs` (`enable_tools()` flips `tools` to
    /// `Some(ToolsCapability::default())`). No other capabilities are
    /// enabled here on purpose — see `get_info_advertises_only_tools_capability`.
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            tools: vec![garra_ask_tool()],
            next_cursor: None,
            meta: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        if request.name != "garra_ask" {
            return Err(McpError::invalid_params(
                format!("unknown tool: '{}'", request.name),
                None,
            ));
        }

        // Deserialize arguments (additionalProperties = false enforced via serde).
        let raw = request.arguments.unwrap_or_default();
        let args: GarraAskArgs = serde_json::from_value(JsonValue::Object(raw))
            .map_err(|e| McpError::invalid_params(format!("invalid arguments: {e}"), None))?;
        if let Err(e) = validate_args(&args) {
            return Err(McpError::invalid_params(e, None));
        }

        // Build opts + call the pure core.
        //
        // GAR-587: apply MCP schema defaults to `provider`/`model` before
        // they reach `ask::ask_oneshot`. The schema descriptor advertises
        // `default` values but MCP hosts do not synthesize them, so the
        // handler honors the contract explicitly.
        let (provider, model) = resolve_overrides(args.provider, args.model);
        let timeout_secs = args
            .timeout_secs
            .unwrap_or_else(|| self.policy.default_timeout_secs());
        // Runtime policy: provider enum (advisory in the schema, binding
        // here) + operator model allowlist + operator timeout cap.
        if let Err(e) = validate_policy(&self.config, &self.policy, &provider, &model, timeout_secs)
        {
            return Err(McpError::invalid_params(e, None));
        }
        let opts = AskOptions {
            message: args.message,
            provider_override: Some(provider),
            model_override: Some(model),
            url_override: None,
            timeout_secs,
            system_prompt_override: args.system_prompt,
        };
        let outcome = ask::ask_oneshot(&self.config, opts).await;

        // Pack the full `garra.ask.v1` envelope as MCP text content
        // (user-locked design: gives Claude visibility into provider/
        // model/latency/error.kind without parsing free-form text).
        let envelope = outcome.to_envelope();
        let text = serde_json::to_string(&envelope).unwrap_or_else(|_| {
            String::from(
                "{\"schema\":\"garra.ask.v1\",\"ok\":false,\"error\":{\"kind\":\"io\",\"message\":\"json serialization failed\"}}",
            )
        });
        let content = vec![Content::text(text)];
        if outcome.is_ok() {
            Ok(CallToolResult::success(content))
        } else {
            Ok(CallToolResult::error(content))
        }
    }
}

/// GAR-583 — entry point invoked by `Commands::McpServer` in `main.rs`.
///
/// Runs the MCP stdio server until EOF on stdin (graceful shutdown by
/// the host). Tracing logs go to stderr via the `RedactingWriter`
/// configured in `main.rs`; stdout is reserved exclusively for the
/// JSON-RPC channel.
pub async fn run_mcp_server(config: AppConfig) -> Result<()> {
    tracing::info!("MCP server starting on stdio (GAR-583)");
    let policy = ServerPolicy::from_env();
    if policy.model_allowlist.is_empty() && policy.max_timeout_secs.is_none() {
        tracing::info!(
            "garra_ask policy: no operator limits (set GARRAIA_MCP_MODEL_ALLOWLIST / \
             GARRAIA_MCP_MAX_TIMEOUT_SECS to restrict MCP callers)"
        );
    } else {
        tracing::info!(
            model_allowlist = ?policy.model_allowlist,
            max_timeout_secs = ?policy.max_timeout_secs,
            "garra_ask policy: operator limits active"
        );
    }
    let handler = GarraToolHandler::with_policy(Arc::new(config), policy);
    let (stdin, stdout) = rmcp::transport::io::stdio();
    let service = handler.serve((stdin, stdout)).await?;
    tracing::info!("MCP server ready; waiting for client requests");
    service.waiting().await?;
    tracing::info!("MCP server shutdown");
    Ok(())
}

#[cfg(test)]
mod tests {
    //! GAR-583 — Pure tests. Zero network, zero env-mutation, zero
    //! filesystem. The two `audit_…` tests below are the load-bearing
    //! safety net for the locked-in invariants of this PR.

    use super::*;
    use serde_json::json;

    // ─── Tool descriptor ──────────────────────────────────────────────

    #[test]
    fn tool_descriptor_name_is_garra_ask() {
        let t = garra_ask_tool();
        assert_eq!(t.name.as_ref(), "garra_ask");
    }

    #[test]
    fn tool_descriptor_has_description() {
        let t = garra_ask_tool();
        let desc = t.description.expect("description present");
        assert!(desc.contains("GarraIA"));
        assert!(desc.contains("garra.ask.v1"));
    }

    /// O provider keyless `echo` só entra no enum com a feature
    /// `dev-echo-provider` — jamais em builds default/produção.
    #[test]
    fn tool_descriptor_provider_enum_gates_echo_by_feature() {
        let t = garra_ask_tool();
        let schema = (*t.input_schema).clone();
        let has_echo = schema
            .get("properties")
            .and_then(|p| p.get("provider"))
            .and_then(|p| p.get("enum"))
            .and_then(|e| e.as_array())
            .map(|e| e.iter().any(|v| v.as_str() == Some("echo")))
            .expect("provider enum present");
        assert_eq!(has_echo, cfg!(feature = "dev-echo-provider"));
    }

    #[test]
    fn tool_descriptor_message_is_required() {
        let t = garra_ask_tool();
        let schema = (*t.input_schema).clone();
        let required = schema
            .get("required")
            .and_then(|v| v.as_array())
            .cloned()
            .expect("required array present");
        assert!(required.iter().any(|v| v.as_str() == Some("message")));
    }

    #[test]
    fn tool_descriptor_default_model_is_openrouter_free() {
        let t = garra_ask_tool();
        let schema = (*t.input_schema).clone();
        let model_default = schema
            .get("properties")
            .and_then(|p| p.get("model"))
            .and_then(|m| m.get("default"))
            .and_then(|d| d.as_str());
        assert_eq!(model_default, Some("openrouter/free"));
    }

    /// GAR-587 — schema-side parity with `tool_descriptor_default_model_…`.
    /// Pins the advertised default so the schema descriptor and the
    /// runtime constant in [`resolve_overrides`] never drift.
    #[test]
    fn tool_descriptor_default_provider_is_openrouter() {
        let t = garra_ask_tool();
        let schema = (*t.input_schema).clone();
        let provider_default = schema
            .get("properties")
            .and_then(|p| p.get("provider"))
            .and_then(|m| m.get("default"))
            .and_then(|d| d.as_str());
        assert_eq!(provider_default, Some("openrouter"));
    }

    #[test]
    fn tool_descriptor_timeout_range_is_1_to_600() {
        let t = garra_ask_tool();
        let schema = (*t.input_schema).clone();
        let ts = schema
            .get("properties")
            .and_then(|p| p.get("timeout_secs"))
            .cloned()
            .expect("timeout_secs property present");
        assert_eq!(ts.get("minimum").and_then(|v| v.as_u64()), Some(1));
        assert_eq!(ts.get("maximum").and_then(|v| v.as_u64()), Some(600));
        assert_eq!(ts.get("default").and_then(|v| v.as_u64()), Some(60));
    }

    #[test]
    fn tool_descriptor_additional_properties_is_false() {
        let t = garra_ask_tool();
        let schema = (*t.input_schema).clone();
        let ap = schema.get("additionalProperties").and_then(|v| v.as_bool());
        assert_eq!(ap, Some(false));
    }

    // ─── Argument validation ──────────────────────────────────────────

    fn parse_args(v: serde_json::Value) -> Result<GarraAskArgs, String> {
        serde_json::from_value(v).map_err(|e| e.to_string())
    }

    #[test]
    fn args_deserialize_minimum_required() {
        let a = parse_args(json!({"message": "hi"})).unwrap();
        assert_eq!(a.message, "hi");
        assert!(a.provider.is_none());
        assert!(a.model.is_none());
        assert!(a.timeout_secs.is_none());
        assert!(a.system_prompt.is_none());
    }

    #[test]
    fn args_reject_missing_message() {
        let err = parse_args(json!({})).unwrap_err();
        assert!(err.contains("message"), "got: {err}");
    }

    #[test]
    fn args_reject_additional_properties() {
        let err = parse_args(json!({"message": "hi", "rogue": "x"})).unwrap_err();
        assert!(err.contains("rogue") || err.contains("unknown field"));
    }

    #[test]
    fn args_explicit_openrouter_auto_accepted_at_deser_layer() {
        // The deser layer accepts any string for `model` — the policy
        // (default = free, auto only opt-in) is enforced by passing the
        // value through to `ask_oneshot`, NOT by rejecting it here.
        let a = parse_args(json!({"message": "x", "model": "openrouter/auto"})).unwrap();
        assert_eq!(a.model.as_deref(), Some("openrouter/auto"));
    }

    #[test]
    fn validate_args_rejects_empty_message() {
        let a = GarraAskArgs {
            message: "   ".to_string(),
            provider: None,
            model: None,
            timeout_secs: None,
            system_prompt: None,
        };
        let err = validate_args(&a).unwrap_err();
        assert!(err.contains("non-empty"));
    }

    #[test]
    fn validate_args_rejects_message_over_64kib() {
        let a = GarraAskArgs {
            message: "a".repeat(ARG_MESSAGE_MAX_BYTES + 1),
            provider: None,
            model: None,
            timeout_secs: None,
            system_prompt: None,
        };
        let err = validate_args(&a).unwrap_err();
        assert!(err.contains("64"));
    }

    #[test]
    fn validate_args_rejects_timeout_below_min() {
        let a = GarraAskArgs {
            message: "x".to_string(),
            provider: None,
            model: None,
            timeout_secs: Some(0),
            system_prompt: None,
        };
        assert!(validate_args(&a).is_err());
    }

    #[test]
    fn validate_args_rejects_timeout_above_max() {
        let a = GarraAskArgs {
            message: "x".to_string(),
            provider: None,
            model: None,
            timeout_secs: Some(601),
            system_prompt: None,
        };
        assert!(validate_args(&a).is_err());
    }

    #[test]
    fn validate_args_rejects_system_prompt_over_8kib() {
        let a = GarraAskArgs {
            message: "x".to_string(),
            provider: None,
            model: None,
            timeout_secs: None,
            system_prompt: Some("a".repeat(ARG_SYSTEM_PROMPT_MAX_BYTES + 1)),
        };
        let err = validate_args(&a).unwrap_err();
        assert!(err.contains("system_prompt"));
    }

    #[test]
    fn validate_args_accepts_typical_payload() {
        let a = GarraAskArgs {
            message: "hi".to_string(),
            provider: Some("openrouter".to_string()),
            model: Some("openrouter/free".to_string()),
            timeout_secs: Some(30),
            system_prompt: Some("be brief".to_string()),
        };
        assert!(validate_args(&a).is_ok());
    }

    // ─── Default resolution (GAR-587) ─────────────────────────────────

    /// GAR-587 §5 case #1 — the load-bearing test. Minimum-payload calls
    /// (`{ "message": "hi" }`) MUST resolve to the schema-advertised
    /// defaults BEFORE reaching `ask::ask_oneshot`, otherwise the
    /// `AppConfig` fallback path can route to an unrelated provider
    /// (operator-side OpenAI 401 was the original repro on 2026-05-11).
    #[test]
    fn resolve_overrides_applies_defaults_when_args_are_none() {
        let (provider, model) = resolve_overrides(None, None);
        assert_eq!(provider, "openrouter");
        assert_eq!(model, "openrouter/free");
    }

    /// GAR-587 §5 case #2 — caller-explicit `provider`/`model` MUST
    /// survive the helper unchanged. Otherwise the fix would clobber
    /// legitimate per-call routing.
    #[test]
    fn resolve_overrides_keeps_explicit_provider_and_model() {
        let (provider, model) = resolve_overrides(
            Some("anthropic".to_string()),
            Some("claude-opus-4-7".to_string()),
        );
        assert_eq!(provider, "anthropic");
        assert_eq!(model, "claude-opus-4-7");
    }

    /// GAR-587 §5 case #3 — `openrouter/auto` MUST remain opt-in. Two
    /// branches: (a) explicit `auto` survives; (b) `None` model never
    /// promotes itself to `auto` — the default stays `openrouter/free`.
    #[test]
    fn resolve_overrides_passes_openrouter_auto_only_when_explicit() {
        // (a) Explicit `auto` is preserved.
        let (provider, model) = resolve_overrides(None, Some("openrouter/auto".to_string()));
        assert_eq!(provider, "openrouter");
        assert_eq!(model, "openrouter/auto");

        // (b) Absent model never becomes `auto`.
        let (_, model_default) = resolve_overrides(None, None);
        assert_ne!(model_default, "openrouter/auto");
        assert_eq!(model_default, "openrouter/free");
    }

    /// GAR-587 — mixed-fill case (provider explicit, model omitted) and
    /// its mirror (model explicit, provider omitted). Catches a class of
    /// regressions where a future refactor swaps the two branches.
    #[test]
    fn resolve_overrides_handles_partial_overrides() {
        let (provider, model) = resolve_overrides(Some("ollama".to_string()), None);
        assert_eq!(provider, "ollama");
        assert_eq!(model, "openrouter/free");

        let (provider, model) = resolve_overrides(None, Some("gpt-4o-mini".to_string()));
        assert_eq!(provider, "openrouter");
        assert_eq!(model, "gpt-4o-mini");
    }

    // ─── Audit invariants (load-bearing) ──────────────────────────────

    /// GAR-583 §4 invariant #1 — production code MUST NOT write to
    /// stdout. Stdio MCP transport reserves stdout for JSON-RPC; any
    /// `println!`/`print!`/`stdout()` call corrupts the channel.
    ///
    /// Scans only the production half of `mcp_server.rs` (everything
    /// before `#[cfg(test)]`).
    #[test]
    fn audit_mcp_server_never_writes_to_stdout() {
        let source = include_str!("mcp_server.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        let forbidden = [
            "println!",
            "print!",
            "io::stdout",
            "std::io::stdout",
            "stdout().write",
        ];
        for needle in forbidden {
            assert!(
                !production.contains(needle),
                "mcp_server.rs production code must not contain `{needle}` (GAR-583 stdio invariant)"
            );
        }
    }

    // ─── ServerHandler::get_info (GAR-585) ────────────────────────────

    /// GAR-585 — the MCP `initialize` handshake serves whatever
    /// `ServerHandler::get_info` returns; the default impl in `rmcp 1.6`
    /// hands back `ServerInfo::default()` with `ServerCapabilities::default()`
    /// (every field `None`), which serializes as `"capabilities":{}`.
    /// Hosts read that as "no tools" and skip `tools/list`, leaving
    /// `garra_ask` invisible to Claude Code / Desktop even though
    /// `list_tools` is wired correctly. This test pins the override.
    #[test]
    fn get_info_advertises_tools_capability() {
        let cfg = std::sync::Arc::new(garraia_config::AppConfig::default());
        let handler = GarraToolHandler::with_policy(cfg, ServerPolicy::default());
        let info = ServerHandler::get_info(&handler);
        assert!(
            info.capabilities.tools.is_some(),
            "GAR-585: `initialize` must advertise the `tools` capability so MCP hosts call `tools/list`"
        );
    }

    /// GAR-585 §3 (out of scope) — the override touches the `tools`
    /// capability only; we must not silently start advertising
    /// resources/prompts/logging/sampling/etc., which would change the
    /// contract surface MCP hosts negotiate against us.
    #[test]
    fn get_info_advertises_only_tools_capability() {
        let cfg = std::sync::Arc::new(garraia_config::AppConfig::default());
        let handler = GarraToolHandler::with_policy(cfg, ServerPolicy::default());
        let info = ServerHandler::get_info(&handler);
        let caps = &info.capabilities;
        assert!(caps.experimental.is_none(), "experimental must stay off");
        assert!(caps.extensions.is_none(), "extensions must stay off");
        assert!(caps.logging.is_none(), "logging must stay off");
        assert!(caps.completions.is_none(), "completions must stay off");
        assert!(caps.prompts.is_none(), "prompts must stay off");
        assert!(caps.resources.is_none(), "resources must stay off");
        assert!(caps.tasks.is_none(), "tasks must stay off");
    }

    /// GAR-583 §4 invariant #2 — production code MUST NOT register
    /// dangerous tools and MUST NOT spawn subprocesses. The handler
    /// calls `ask::ask_oneshot` in-process; any escape hatch defeats
    /// the LLM-only guarantee documented in the tool description.
    #[test]
    fn audit_mcp_server_never_registers_dangerous_tools() {
        let source = include_str!("mcp_server.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        let forbidden = [
            "register_tool",
            "BashTool",
            "FileReadTool",
            "FileWriteTool",
            "GitDiffTool",
            "std::process::Command",
            "tokio::process::Command",
        ];
        for needle in forbidden {
            assert!(
                !production.contains(needle),
                "mcp_server.rs production code must not contain `{needle}` (GAR-583 safety invariant)"
            );
        }
    }

    // ─── ServerPolicy (operator limits; pure — no env mutation) ────────

    fn cfg_with_llm_alias(alias: &str) -> AppConfig {
        let mut cfg = AppConfig::default();
        cfg.llm.insert(
            alias.to_string(),
            garraia_config::LlmProviderConfig {
                provider: "openai".to_string(),
                model: None,
                api_key: Some("k".to_string()),
                base_url: Some("http://localhost:1234/v1".to_string()),
                extra: std::collections::HashMap::new(),
            },
        );
        cfg
    }

    #[test]
    fn policy_default_is_unrestricted_and_preserves_historic_behavior() {
        let policy = ServerPolicy::from_values(None, None);
        let cfg = AppConfig::default();
        // Any model — including openrouter/auto — passes without an allowlist.
        assert!(validate_policy(&cfg, &policy, "openrouter", "openrouter/auto", 600).is_ok());
        assert_eq!(policy.default_timeout_secs(), 60);
    }

    #[test]
    fn policy_parses_allowlist_trimming_and_skipping_empties() {
        let policy = ServerPolicy::from_values(Some(" openrouter/free, ,gpt-5-mini "), None);
        let cfg = AppConfig::default();
        assert!(validate_policy(&cfg, &policy, "openrouter", "openrouter/free", 60).is_ok());
        assert!(validate_policy(&cfg, &policy, "openai", "gpt-5-mini", 60).is_ok());
        let err = validate_policy(&cfg, &policy, "openrouter", "openrouter/auto", 60)
            .expect_err("auto must be blocked when allowlist is set");
        assert!(err.contains("GARRAIA_MCP_MODEL_ALLOWLIST"), "{err}");
    }

    #[test]
    fn policy_timeout_cap_rejects_above_and_lowers_default() {
        let policy = ServerPolicy::from_values(None, Some("30"));
        let cfg = AppConfig::default();
        assert!(validate_policy(&cfg, &policy, "openrouter", "openrouter/free", 30).is_ok());
        let err = validate_policy(&cfg, &policy, "openrouter", "openrouter/free", 31)
            .expect_err("above cap must be rejected");
        assert!(err.contains("GARRAIA_MCP_MAX_TIMEOUT_SECS=30"), "{err}");
        // Caller omitting timeout_secs gets a default that respects the cap.
        assert_eq!(policy.default_timeout_secs(), 30);
    }

    #[test]
    fn policy_timeout_cap_is_clamped_to_schema_range() {
        let policy = ServerPolicy::from_values(None, Some("9999"));
        assert_eq!(policy.max_timeout_secs, Some(600));
        let unparsable = ServerPolicy::from_values(None, Some("banana"));
        assert_eq!(unparsable.max_timeout_secs, None);
    }

    #[test]
    fn policy_gates_echo_provider_by_feature() {
        // Runtime enforcement must mirror the schema enum: "echo" passes
        // only when dev-echo-provider is compiled in (PR #859).
        let policy = ServerPolicy::default();
        let cfg = AppConfig::default();
        let ok = validate_policy(&cfg, &policy, "echo", "echo", 60).is_ok();
        assert_eq!(ok, cfg!(feature = "dev-echo-provider"));
    }

    #[test]
    fn policy_provider_enum_is_enforced_but_config_aliases_pass() {
        let policy = ServerPolicy::default();
        let cfg = cfg_with_llm_alias("sansa");
        // Schema enum members always pass.
        for p in ["ollama", "anthropic", "openai", "openrouter"] {
            assert!(validate_policy(&cfg, &policy, p, "any-model", 60).is_ok());
        }
        // Alias configured under llm: passes; an unknown provider does not.
        assert!(validate_policy(&cfg, &policy, "sansa", "any-model", 60).is_ok());
        let err = validate_policy(&cfg, &policy, "evil-provider", "any-model", 60)
            .expect_err("unknown provider must be rejected");
        assert!(err.contains("not accepted"), "{err}");
    }
}
