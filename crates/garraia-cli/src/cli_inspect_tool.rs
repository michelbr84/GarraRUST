//! #1712 — Read-only, on-demand introspection of the *current CLI process*.
//!
//! Unlike gateway diagnostics, this tool does not access gateway AppState,
//! credentials, files, the network, or other sessions. It reuses live runtime
//! inventory and last-turn statistics. Unknown external state stays unknown.

use std::sync::{Arc, RwLock, Weak};

use async_trait::async_trait;
use garraia_agents::tools::Disponibilidade;
use garraia_agents::{AgentRuntime, Tool, ToolContext, ToolOutput};
use garraia_common::Result;
use serde_json::{Value, json};

const PAGE_SIZE: usize = 16;

pub struct CliInspectTool {
    runtime: Weak<AgentRuntime>,
    provider: String,
    requested_model: Arc<RwLock<String>>,
}

impl CliInspectTool {
    pub fn new(
        runtime: Weak<AgentRuntime>,
        provider: String,
        requested_model: Arc<RwLock<String>>,
    ) -> Self {
        Self {
            runtime,
            provider,
            requested_model,
        }
    }
}

/// These identifiers may originate from provider configuration / MCP metadata.
/// Keep a single line, limit token cost, and run the project's secret redactor.
fn safe_text(value: &str) -> String {
    let single_line: String = value
        .chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect();
    garraia_security::redact_secrets(&single_line)
}

/// A tool identifier should never carry newlines or other text instructions.
fn safe_tool_name(value: &str) -> String {
    let clean: String = value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(*c, '_' | '-' | '.'))
        .take(80)
        .collect();
    if clean.is_empty() {
        "unknown".to_owned()
    } else {
        clean
    }
}

#[async_trait]
impl Tool for CliInspectTool {
    fn name(&self) -> &str {
        "cli_inspect"
    }

    fn description(&self) -> &str {
        "Read-only inspection of THIS Garra CLI session: installed binary version,          requested provider/model, last observed provider/model and confidence,          registered tools and runtime availability. Does not access the gateway          or secrets. For gateway/MCP health it explicitly reports unknown.          Use when asked what version/model/tools this running CLI really has."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "scope": { "type": "string", "enum": ["identity", "tools", "all"] },
                "offset": { "type": "integer", "minimum": 0, "maximum": 1000 }
            },
            "additionalProperties": false
        })
    }

    async fn execute(&self, ctx: &ToolContext, input: Value) -> Result<ToolOutput> {
        let scope = input
            .get("scope")
            .and_then(Value::as_str)
            .unwrap_or("identity");
        if !matches!(scope, "identity" | "tools" | "all") {
            return Ok(ToolOutput::error("scope must be identity, tools, or all"));
        }
        let offset = match input.get("offset") {
            Some(v) => match v.as_u64() {
                Some(n) if n <= 1000 => n as usize,
                _ => {
                    return Ok(ToolOutput::error(
                        "offset must be an integer from 0 to 1000",
                    ));
                }
            },
            None => 0,
        };
        let Some(runtime) = self.runtime.upgrade() else {
            return Ok(ToolOutput::error("CLI runtime is no longer available"));
        };
        let mut report = json!({
            "origin": "cli_runtime",
            "read_only": true,
            "channel": "cli",
            "scope": scope,
            "gateway": { "state": "unknown", "reason": "not inspected by the CLI tool" },
            "mcp_servers": { "state": "unknown", "reason": "no authorized live MCP health source in this CLI tool" },
            "permissions": { "state": "unknown", "reason": "tool registration does not prove per-turn authorization" }
        });

        if matches!(scope, "identity" | "all") {
            let requested_model = self
                .requested_model
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let last_turn = runtime.last_turn_stats(&ctx.session_id).map(|stat| {
                json!({
                    "state": "observed",
                    "provider": safe_text(&stat.provider),
                    "model": safe_text(&stat.model),
                    "model_confirmed_by_provider": stat.model_confirmado,
                    "fallback_used": stat.fallback,
                    "tool_calls": stat.tool_calls,
                    "source": if stat.model_confirmado { "provider_response" } else { "runtime_requested_model" }
                })
            }).unwrap_or_else(|| json!({
                "state": "unknown",
                "reason": "no completed turn statistics for this CLI session"
            }));
            report["identity"] = json!({
                "version": { "value": env!("CARGO_PKG_VERSION"), "source": "running_binary" },
                "provider_selected": { "value": safe_text(&self.provider), "source": "cli_selection" },
                "model_requested": { "value": safe_text(&requested_model), "source": "current_cli_selection" },
                "last_turn": last_turn
            });
        }

        if matches!(scope, "tools" | "all") {
            let mut inventory = runtime.tool_inventory();
            inventory.sort_by(|a, b| a.name.cmp(&b.name));
            let total = inventory.len();
            let entries: Vec<Value> = inventory
                .into_iter()
                .skip(offset)
                .take(PAGE_SIZE)
                .map(|entry| {
                    let available = match runtime.disponibilidade_de(&entry.name) {
                        Disponibilidade::Disponivel => {
                            json!({ "state": "available_before_policy_gate" })
                        }
                        Disponibilidade::Indisponivel { codigo, .. } => json!({
                            "state": "unavailable",
                            "reason_code": codigo
                        }),
                    };
                    json!({
                        "name": safe_tool_name(&entry.name),
                        "origin": entry.source,
                        "registration": "registered",
                        "runtime_availability": available,
                        "turn_permission": "not_verified"
                    })
                })
                .collect();
            report["tools"] = json!({
                "source": "AgentRuntime.tool_inventory",
                "count": total,
                "offset": offset,
                "page_size": PAGE_SIZE,
                "has_more": offset.saturating_add(entries.len()) < total,
                "entries": entries
            });
        }
        Ok(ToolOutput::success(report.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> ToolContext {
        ToolContext {
            session_id: "cli-test".into(),
            user_id: None,
            is_heartbeat: false,
            approval: Default::default(),
            working_dir: None,
            project_id: None,
        }
    }

    #[tokio::test]
    async fn identity_reads_binary_and_live_model_without_gateway() {
        let runtime = Arc::new(AgentRuntime::new());
        let model = Arc::new(RwLock::new("original-model".into()));
        let tool = CliInspectTool::new(Arc::downgrade(&runtime), "anthropic".into(), model.clone());
        let first = tool.execute(&context(), json!({})).await.unwrap();
        let first: Value = serde_json::from_str(&first.content).unwrap();
        assert_eq!(
            first["identity"]["version"]["value"],
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(
            first["identity"]["model_requested"]["value"],
            "original-model"
        );
        assert_eq!(first["identity"]["last_turn"]["state"], "unknown");
        assert_eq!(first["gateway"]["state"], "unknown");
        *model.write().unwrap() = "switched-model".into();
        let second = tool.execute(&context(), json!({})).await.unwrap();
        let second: Value = serde_json::from_str(&second.content).unwrap();
        assert_eq!(
            second["identity"]["model_requested"]["value"],
            "switched-model"
        );
    }

    #[tokio::test]
    async fn tool_list_is_current_bounded_and_does_not_claim_permissions() {
        let runtime = Arc::new(AgentRuntime::new());
        let model = Arc::new(RwLock::new("model".into()));
        runtime.register_tool(Box::new(CliInspectTool::new(
            Arc::downgrade(&runtime),
            "test".into(),
            model.clone(),
        )));
        let tool = CliInspectTool::new(Arc::downgrade(&runtime), "test".into(), model);
        let output = tool
            .execute(&context(), json!({"scope":"tools"}))
            .await
            .unwrap();
        let report: Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(report["tools"]["count"], 1);
        assert_eq!(report["tools"]["entries"][0]["name"], "cli_inspect");
        assert_eq!(
            report["tools"]["entries"][0]["turn_permission"],
            "not_verified"
        );
        assert_eq!(report["gateway"]["state"], "unknown");
    }

    #[tokio::test]
    async fn rejects_invalid_scope_and_never_executes_another_tool() {
        let runtime = Arc::new(AgentRuntime::new());
        let tool = CliInspectTool::new(
            Arc::downgrade(&runtime),
            "test".into(),
            Arc::new(RwLock::new("model".into())),
        );
        assert!(
            tool.execute(&context(), json!({"scope":"admin"}))
                .await
                .unwrap()
                .is_error
        );
        assert!(
            tool.execute(&context(), json!({"scope":"tools","offset":-1}))
                .await
                .unwrap()
                .is_error
        );
        assert_eq!(runtime.tool_names().len(), 0);
    }
}
