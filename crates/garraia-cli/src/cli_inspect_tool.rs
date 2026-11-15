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
const TOOL_PROGRAM_NAME: &str = "tool_program";

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
        "Read-only inspection of THIS Garra CLI session: binary version, requested provider/model, last observed turn, and tools allowed by the current turn gate. Blocked tools are omitted. Does not access gateway state or secrets; unavailable external state is reported as unknown. Use when asked about this running CLI's verified state."
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
        let turn_tools = garraia_agents::tools::turn_tools::ferramentas_do_turno();
        let permissions = if turn_tools.is_some() {
            json!({
                "state": "verified",
                "source": "runtime_dispatch_turn_tools_snapshot",
                "scope": "current_turn",
                "blocked_tools_omitted": true
            })
        } else {
            json!({
                "state": "unknown",
                "reason": "no current-turn tool snapshot; tool entries are omitted"
            })
        };
        let mut report = json!({
            "origin": "cli_runtime",
            "read_only": true,
            "channel": "cli",
            "scope": scope,
            "gateway": { "state": "unknown", "reason": "not inspected by the CLI tool" },
            "mcp_servers": { "state": "unknown", "reason": "no authorized live MCP health source in this CLI tool" },
            "permissions": permissions
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
            let allowed_tools = turn_tools.as_deref().unwrap_or_default();
            let mut entries: Vec<Value> = runtime
                .tool_inventory()
                .into_iter()
                .filter(|entry| {
                    entry.name != TOOL_PROGRAM_NAME && allowed_tools.contains(&entry.name)
                })
                .map(|entry| {
                    let available = match runtime.disponibilidade_de(&entry.name) {
                        Disponibilidade::Disponivel => json!({ "state": "available" }),
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
                        "turn_permission": "allowed_this_turn"
                    })
                })
                .collect();
            if allowed_tools.iter().any(|name| name == TOOL_PROGRAM_NAME) {
                let available = match runtime.disponibilidade_de(TOOL_PROGRAM_NAME) {
                    Disponibilidade::Disponivel => json!({ "state": "available" }),
                    Disponibilidade::Indisponivel { codigo, .. } => json!({
                        "state": "unavailable",
                        "reason_code": codigo
                    }),
                };
                entries.push(json!({
                    "name": TOOL_PROGRAM_NAME,
                    "origin": "native_runtime",
                    "registration": "synthetic",
                    "runtime_availability": available,
                    "turn_permission": "allowed_this_turn"
                }));
            }
            entries.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
            let total = entries.len();
            let entries: Vec<Value> = entries.into_iter().skip(offset).take(PAGE_SIZE).collect();
            report["tools"] = json!({
                "source": "runtime_dispatch_turn_tools_snapshot",
                "permission_state": if turn_tools.is_some() { "verified" } else { "unknown" },
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

    struct FileWriteFake;

    #[async_trait]
    impl Tool for FileWriteFake {
        fn name(&self) -> &str {
            "file_write"
        }

        fn description(&self) -> &str {
            "test fixture"
        }

        fn input_schema(&self) -> Value {
            json!({"type":"object"})
        }

        async fn execute(&self, _ctx: &ToolContext, _input: Value) -> Result<ToolOutput> {
            Ok(ToolOutput::success("ok"))
        }
    }

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
    async fn tool_list_is_current_bounded_and_reports_effective_permission() {
        let runtime = Arc::new(AgentRuntime::new());
        let model = Arc::new(RwLock::new("model".into()));
        runtime.register_tool(Box::new(CliInspectTool::new(
            Arc::downgrade(&runtime),
            "test".into(),
            model.clone(),
        )));
        runtime.register_tool(Box::new(FileWriteFake));
        let tool = CliInspectTool::new(Arc::downgrade(&runtime), "test".into(), model);
        let output = garraia_agents::tools::turn_tools::com_ferramentas_do_turno(
            vec!["cli_inspect".into(), TOOL_PROGRAM_NAME.into()],
            true,
            tool.execute(&context(), json!({"scope":"tools"})),
        )
        .await
        .unwrap();
        let report: Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(report["tools"]["count"], 2);
        assert_eq!(report["tools"]["entries"][0]["name"], "cli_inspect");
        assert_eq!(report["tools"]["entries"][1]["name"], TOOL_PROGRAM_NAME);
        assert_eq!(report["tools"]["entries"][1]["registration"], "synthetic");
        assert_eq!(report["permissions"]["state"], "verified");
        assert_eq!(
            report["tools"]["entries"][0]["turn_permission"],
            "allowed_this_turn"
        );
        assert_eq!(report["gateway"]["state"], "unknown");
        assert_eq!(report["tools"]["permission_state"], "verified");
    }

    #[tokio::test]
    async fn blocked_tools_are_not_disclosed_and_missing_gate_fails_closed() {
        let runtime = Arc::new(AgentRuntime::new());
        let model = Arc::new(RwLock::new("model".into()));
        runtime.register_tool(Box::new(CliInspectTool::new(
            Arc::downgrade(&runtime),
            "test".into(),
            model.clone(),
        )));
        runtime.register_tool(Box::new(FileWriteFake));
        let tool = CliInspectTool::new(Arc::downgrade(&runtime), "test".into(), model);

        let unknown: Value = serde_json::from_str(
            &tool
                .execute(&context(), json!({"scope":"tools"}))
                .await
                .unwrap()
                .content,
        )
        .unwrap();
        assert_eq!(unknown["permissions"]["state"], "unknown");
        assert_eq!(unknown["tools"]["count"], 0);

        let verified: Value = serde_json::from_str(
            &garraia_agents::tools::turn_tools::com_ferramentas_do_turno(
                vec!["cli_inspect".into()],
                true,
                tool.execute(&context(), json!({"scope":"tools"})),
            )
            .await
            .unwrap()
            .content,
        )
        .unwrap();
        assert_eq!(verified["permissions"]["state"], "verified");
        let names: Vec<_> = verified["tools"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|entry| entry["name"].as_str())
            .collect();
        assert!(names.contains(&"cli_inspect"));
        assert!(!names.contains(&"file_write"));
    }

    #[test]
    fn cli_inspect_is_wired_to_the_runtime_dispatch_snapshot() {
        let tool_source = include_str!("cli_inspect_tool.rs");
        let runtime_source = include_str!("../../garraia-agents/src/runtime.rs");
        assert!(tool_source.contains("turn_tools::ferramentas_do_turno()"));
        assert!(runtime_source.contains("pode_receber_snapshot_de_ferramentas(name, &source)"));
        assert!(runtime_source.contains("name == CLI_INSPECT_TOOL"));
        assert!(runtime_source.contains("ToolSource::Native"));
        assert!(runtime_source.contains("const TOOL_PROGRAM_NAME: &str = \"tool_program\";"));
        assert!(runtime_source.contains("liberadas.push(TOOL_PROGRAM_NAME.to_string())"));
        assert!(tool_source.contains("name == TOOL_PROGRAM_NAME"));
        let dispatch = runtime_source
            .find("match self.find_tool_com_origem(name)")
            .expect("production tool dispatch retains registration source");
        let dispatch_source = &runtime_source[dispatch..];
        let trust_check = dispatch_source
            .find("pode_receber_snapshot_de_ferramentas(name, &source)")
            .expect("the runtime checks the trusted native-tool allowlist");
        let scope_open = dispatch_source
            .find("crate::tools::turn_tools::com_ferramentas_do_turno(")
            .expect("the checked dispatch opens the task-local snapshot");
        assert!(trust_check < scope_open);
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
