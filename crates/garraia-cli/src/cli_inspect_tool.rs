//! #1712 — Read-only, on-demand introspection of the *current CLI process*.
//!
//! Unlike gateway diagnostics, this tool does not access gateway AppState,
//! credentials, files, the network, or other sessions. It reuses live runtime
//! inventory and last-turn statistics. Optional integrations (gateway, MCP
//! servers) are reported from the configuration this process already loaded —
//! always as *declared*, with `live_verification: unavailable_from_cli`, never
//! as observed health ([`crate::estado_declarado`]).

use std::sync::{Arc, Mutex, RwLock, Weak};

use async_trait::async_trait;
use garraia_agents::runtime::CLI_INSPECT_TOOL_NAME;
use garraia_agents::tools::Disponibilidade;
use garraia_agents::{AgentRuntime, Tool, ToolContext, ToolOutput};
use garraia_common::Result;
use serde_json::{Value, json};

use crate::efeitos_de_ferramenta::{ProjecaoDeEfeitos, SnapshotDeEfeitos};
use crate::estado_declarado::{EstadoDeclarado, MAX_SERVIDORES};

const PAGE_SIZE: usize = 16;

pub struct CliInspectTool {
    runtime: Weak<AgentRuntime>,
    provider: String,
    requested_model: Arc<RwLock<String>>,
    /// #1712 — configuração efetiva deste processo, já projetada. Separada do
    /// runtime porque não é inventário de ferramentas: é estado DECLARADO de
    /// integrações que a CLI não sobe nem verifica.
    estado: EstadoDeclarado,
    /// #1709 — efeitos de ferramenta observados nesta sessão. Alimentado pelo
    /// fim de cada turno com os registros do executor, nunca pelo texto do
    /// modelo; a projeção é fail-closed quando nenhum turno foi observado.
    efeitos: Arc<Mutex<SnapshotDeEfeitos>>,
}

impl CliInspectTool {
    pub fn new(
        runtime: Weak<AgentRuntime>,
        provider: String,
        requested_model: Arc<RwLock<String>>,
        estado: EstadoDeclarado,
        efeitos: Arc<Mutex<SnapshotDeEfeitos>>,
    ) -> Self {
        Self {
            runtime,
            provider,
            requested_model,
            estado,
            efeitos,
        }
    }
}

/// Explicitly labelled credentials in free-form configuration values are not
/// covered by token-shape redaction. Since these fields may be reflected to an
/// LLM, discard the whole value rather than preserving a partial header or key.
fn contains_explicit_credential(value: &str) -> bool {
    static AUTH_HEADER: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r#"(?ix)
                (?:[\x22']\s*)?\b(?:proxy[-_ ]*)?authorization[\x22']?\s*:\s*[\x22']?[a-z][a-z0-9_-]*\s+\S+
                | \bbearer\s+\S+
                "#,
        )
        .expect("credential header regex should compile")
    });
    AUTH_HEADER.is_match(value) || contains_credential_assignment(value)
}

fn contains_credential_assignment(value: &str) -> bool {
    value.char_indices().any(|(delimiter_index, delimiter)| {
        if !matches!(delimiter, ':' | '=') {
            return false;
        }
        let prefix = value[..delimiter_index]
            .trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '\"' | '\''));
        let label_start = prefix
            .char_indices()
            .rev()
            .find(|(_, ch)| !ch.is_ascii_alphanumeric() && !matches!(*ch, '_' | '-' | '.' | ' '))
            .map_or(0, |(index, ch)| index + ch.len_utf8());
        credential_label_contains_marker(&prefix[label_start..])
    })
}

fn credential_label_contains_marker(label: &str) -> bool {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut previous = None;
    let mut characters = label.chars().peekable();
    while let Some(character) = characters.next() {
        if !character.is_ascii_alphanumeric() {
            if !current.is_empty() {
                parts.push(std::mem::take(&mut current));
            }
            previous = None;
            continue;
        }
        let next = characters.peek().copied();
        let camel_case_boundary = character.is_ascii_uppercase()
            && previous.is_some_and(|previous: char| {
                previous.is_ascii_lowercase()
                    || previous.is_ascii_digit()
                    || (previous.is_ascii_uppercase()
                        && next.is_some_and(|next| next.is_ascii_lowercase()))
            });
        if camel_case_boundary && !current.is_empty() {
            parts.push(std::mem::take(&mut current));
        }
        current.push(character.to_ascii_lowercase());
        previous = Some(character);
    }
    if !current.is_empty() {
        parts.push(current);
    }

    if parts.last().is_some_and(|part| {
        matches!(
            part.as_str(),
            "key"
                | "token"
                | "secret"
                | "password"
                | "passwd"
                | "pwd"
                | "credential"
                | "credentials"
                | "auth"
                | "authorization"
                | "bearer"
        )
    }) {
        return true;
    }

    matches!(
        parts.join("").as_str(),
        "apikey"
            | "accesstoken"
            | "refreshtoken"
            | "authtoken"
            | "clientsecret"
            | "privatekey"
            | "accesskey"
            | "signingkey"
            | "secretkey"
            | "secretaccesskey"
            | "awssecretaccesskey"
            | "idtoken"
            | "sessiontoken"
    )
}

/// These identifiers may originate from provider configuration / MCP metadata.
/// Keep a single line, limit token cost, and run the project's secret redactor.
fn safe_text(value: &str) -> String {
    let single_line: String = value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(120)
        .collect();
    if contains_explicit_credential(&single_line) {
        return "[REDACTED]".to_owned();
    }
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

fn disponibilidade_json(disponibilidade: &Disponibilidade) -> Value {
    match disponibilidade {
        Disponibilidade::Disponivel => json!({ "state": "available" }),
        Disponibilidade::Indisponivel { codigo, .. } => json!({
            "state": "unavailable",
            "reason_code": codigo
        }),
    }
}

/// #1712 — o gateway é um processo SEPARADO: a CLI nunca fala com ele nem tem
/// canal de saúde. O que dá para afirmar com fonte autorizada é o listen
/// declarado na configuração efetiva deste processo; a saúde fica fora.
fn gateway_json(estado: &EstadoDeclarado) -> Value {
    if estado.gateway_listen.is_empty() {
        return json!({
            "state": "not_configured",
            "source": "cli_process_config_snapshot",
            "live_verification": "unavailable_from_cli"
        });
    }
    json!({
        "state": "declared",
        "source": "cli_process_config_snapshot",
        "live_verification": "unavailable_from_cli",
        "listen": safe_text(&estado.gateway_listen),
        "note": "declared listener only; the CLI does not connect to the gateway, so a running gateway is NOT verified here"
    })
}

/// #1712 — servidores MCP declarados (config.yml + mcp.json), sem nenhum
/// campo sensível: só nome, transporte e habilitado. `not_configured` quando o
/// conjunto é vazio é o vocabulário que o resto do produto usa.
fn mcp_servers_json(estado: &EstadoDeclarado) -> Value {
    let total = estado.mcp_servers.len();
    let entradas: Vec<Value> = estado
        .mcp_servers
        .iter()
        .take(MAX_SERVIDORES)
        .map(|servidor| {
            json!({
                "name": safe_tool_name(&servidor.nome),
                "transport": servidor.transporte.como_str(),
                "enabled": servidor.habilitado
            })
        })
        .collect();
    let listados = entradas.len();
    json!({
        "state": if total == 0 { "not_configured" } else { "declared" },
        "source": "merged_config_snapshot",
        "mcp_json_included": estado.mcp_json_incluido,
        "live_verification": "unavailable_from_cli",
        "count": total,
        "enabled_count": estado.habilitados(),
        "listed": listados,
        "has_more": listados < total,
        "servers": entradas,
        "note": "the CLI does not start MCP servers; declared is not running"
    })
}

/// #1709 — efeitos de ferramenta observados nesta sessão da CLI.
///
/// Fonte: a trilha alimentada pelos eventos do **executor**. Cada entrada leva
/// a classe de evidência (`observado`/`falhou`/`nao_verificado`) e o id da
/// execução quando existe; nunca o `input`, o `output` ou o caminho envolvido.
/// Sem turno com ferramenta, o estado é `unverified` — lista vazia que pareça
/// "nada rodou" seria a leitura errada.
fn efeitos_json(projecao: &ProjecaoDeEfeitos) -> Value {
    if !projecao.disponivel {
        return json!({
            "state": "unverified",
            "source": "cli_session_tool_trail",
            "reason": projecao
                .motivo
                .unwrap_or("no_tool_turn_observed_in_this_session"),
            "total_observed": 0,
            "entries": []
        });
    }
    let entries: Vec<Value> = projecao
        .linhas
        .iter()
        .map(|linha| {
            json!({
                "tool_call_id": linha.execucao.as_deref().map(safe_tool_name),
                "tool": linha.ferramenta.as_deref().map(safe_tool_name),
                "class": linha.classe,
                "turn": linha.turno,
                "order": linha.ordem,
                "claim": safe_text(&linha.texto)
            })
        })
        .collect();
    json!({
        "state": "observed",
        "source": "cli_session_tool_trail",
        "total_observed": projecao.total,
        "shown": projecao.exibidas,
        "last_turn": projecao.ultimo_turno,
        "bounded": true,
        "note": "executor records only; an interrupted call is NOT evidence that it did not run",
        "entries": entries
    })
}

#[async_trait]
impl Tool for CliInspectTool {
    fn name(&self) -> &str {
        CLI_INSPECT_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Read-only inspection of THIS Garra CLI session: binary version, requested provider/model, last observed turn, tools allowed by the current turn gate, tool executions the executor actually recorded (with evidence class and call id, so claims like 'I wrote the file' can be checked against the runtime ledger), and the DECLARED (never verified) gateway/MCP configuration. Blocked tools are omitted. Does not access gateway runtime state or secrets; gateway and MCP health is not observable from the CLI and is reported as declared, not as running. Use when asked about this running CLI's verified state or whether a previous tool call really ran."
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
        let efeitos = self
            .efeitos
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .projecao();
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
            "gateway": gateway_json(&self.estado),
            "mcp_servers": mcp_servers_json(&self.estado),
            "tool_effects": efeitos_json(&efeitos),
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
            // #1712: MESMA fonte dos slash commands (`/status`, `/tools`). Esta
            // projeção filtra fail-closed (só o que o portão liberou); a de
            // interface mostra o registro e diz que a liberação é por turno.
            let visao = runtime.visao_das_ferramentas(turn_tools.as_deref());
            let entries: Vec<Value> = visao
                .ferramentas
                .iter()
                .filter(|ferramenta| ferramenta.liberada == Some(true))
                .map(|ferramenta| {
                    json!({
                        "name": safe_tool_name(&ferramenta.nome),
                        "origin": ferramenta.origem,
                        "registration": if ferramenta.sintetica { "synthetic" } else { "registered" },
                        "runtime_availability": disponibilidade_json(&ferramenta.disponibilidade),
                        "turn_permission": "allowed_this_turn"
                    })
                })
                .collect();
            let total = entries.len();
            let entries: Vec<Value> = entries.into_iter().skip(offset).take(PAGE_SIZE).collect();
            report["tools"] = json!({
                "source": "runtime_dispatch_turn_tools_snapshot",
                "permission_state": if turn_tools.is_some() { "verified" } else { "unknown" },
                "registered_total": visao.registradas(),
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
    use garraia_agents::runtime::TOOL_PROGRAM_NAME;

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
    async fn hides_complete_values_with_explicit_credential_markers() {
        let bearer_marker = ["Author", "ization", ": ", "Be", "arer\n"].concat();
        let bearer_value = format!("{bearer_marker}{}", "test-marker-".repeat(4));
        let assignment_marker = ["\"", "api", "Key", "\": \""].concat();
        let assignment_value = format!("{assignment_marker}{}\"", "not-a-real-value-".repeat(3));
        let runtime = Arc::new(AgentRuntime::new());
        let tool = CliInspectTool::new(
            Arc::downgrade(&runtime),
            bearer_value,
            Arc::new(RwLock::new(assignment_value)),
            EstadoDeclarado::default(),
            Arc::new(Mutex::new(SnapshotDeEfeitos::default())),
        );

        let output = tool.execute(&context(), json!({})).await.unwrap();
        let report: Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(
            report["identity"]["provider_selected"]["value"],
            "[REDACTED]"
        );
        assert_eq!(report["identity"]["model_requested"]["value"], "[REDACTED]");
        assert!(!output.content.contains("test-marker"));
        assert!(!output.content.contains("not-a-real-value"));

        let credential_markers = [
            ["aws", "_", "secret", "_", "access", "_", "key="].concat(),
            ["api", "Key="].concat(),
            ["api", "key="].concat(),
            ["api", "_", "key="].concat(),
            ["access", "Token="].concat(),
            ["auth", "Token="].concat(),
            ["auth", "token="].concat(),
            ["client", "Secret="].concat(),
            "pwd=".to_owned(),
            "passwd=".to_owned(),
        ];
        for marker in credential_markers {
            let value = format!("{marker}{}", "runtime-sentinel-".repeat(3));
            assert_eq!(safe_text(&value), "[REDACTED]", "marker: {marker}");
        }
        for ordinary_value in [
            "model: ordinary-local-model",
            "max_tokens: 4096",
            "tokenizer: simple",
            "monkey:latest",
            "oauth:v1",
        ] {
            assert_eq!(safe_text(ordinary_value), ordinary_value);
        }
    }

    #[tokio::test]
    async fn identity_reads_binary_and_live_model_without_gateway() {
        let runtime = Arc::new(AgentRuntime::new());
        let model = Arc::new(RwLock::new("original-model".into()));
        let tool = CliInspectTool::new(
            Arc::downgrade(&runtime),
            "anthropic".into(),
            model.clone(),
            EstadoDeclarado::default(),
            Arc::new(Mutex::new(SnapshotDeEfeitos::default())),
        );
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
        // #1712: sem configuracao no snapshot, o estado e `not_configured`
        // — nao mais o `unknown` generico que nao distinguia os dois casos.
        assert_eq!(first["gateway"]["state"], "not_configured");
        assert_eq!(first["mcp_servers"]["state"], "not_configured");
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
            EstadoDeclarado::default(),
            Arc::new(Mutex::new(SnapshotDeEfeitos::default())),
        )));
        runtime.register_tool(Box::new(FileWriteFake));
        let tool = CliInspectTool::new(
            Arc::downgrade(&runtime),
            "test".into(),
            model,
            EstadoDeclarado::default(),
            Arc::new(Mutex::new(SnapshotDeEfeitos::default())),
        );
        let output = garraia_agents::tools::turn_tools::com_ferramentas_do_turno(
            vec!["cli_inspect".into(), TOOL_PROGRAM_NAME.into()],
            true,
            tool.execute(&context(), json!({"scope":"tools"})),
        )
        .await
        .unwrap();
        let report: Value = serde_json::from_str(&output.content).unwrap();
        assert_eq!(report["tools"]["count"], 2);
        assert_eq!(report["tools"]["registered_total"], 2);
        assert_eq!(report["tools"]["entries"][0]["name"], "cli_inspect");
        assert_eq!(report["tools"]["entries"][1]["name"], TOOL_PROGRAM_NAME);
        assert_eq!(report["tools"]["entries"][1]["registration"], "synthetic");
        assert_eq!(report["permissions"]["state"], "verified");
        assert_eq!(
            report["tools"]["entries"][0]["turn_permission"],
            "allowed_this_turn"
        );
        assert_eq!(report["gateway"]["state"], "not_configured");
        assert_eq!(report["mcp_servers"]["state"], "not_configured");
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
            EstadoDeclarado::default(),
            Arc::new(Mutex::new(SnapshotDeEfeitos::default())),
        )));
        runtime.register_tool(Box::new(FileWriteFake));
        let tool = CliInspectTool::new(
            Arc::downgrade(&runtime),
            "test".into(),
            model,
            EstadoDeclarado::default(),
            Arc::new(Mutex::new(SnapshotDeEfeitos::default())),
        );

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
        let normalized_tool_source = include_str!("cli_inspect_tool.rs").replace("\r\n", "\n");
        let tool_source = normalized_tool_source
            .split("#[cfg(test)]")
            .next()
            .expect("CLI inspection tool has a production section");
        let runtime_source = include_str!("../../garraia-agents/src/runtime.rs");
        assert!(tool_source.contains("turn_tools::ferramentas_do_turno()"));
        assert!(
            tool_source.contains("fn name(&self) -> &str {\n        CLI_INSPECT_TOOL_NAME\n    }")
        );
        assert!(runtime_source.contains("pode_receber_snapshot_de_ferramentas(name, &source)"));
        assert!(runtime_source.contains("name == CLI_INSPECT_TOOL_NAME"));
        assert!(runtime_source.contains("name == GARRA_DIAGNOSTICS_TOOL_NAME"));
        assert!(runtime_source.contains("ToolSource::Native"));
        assert!(
            runtime_source.contains("pub const GARRA_STATUS_TOOL_NAME: &str = \"garra_status\";")
        );
        assert!(
            runtime_source
                .contains("pub const GARRA_DIAGNOSTICS_TOOL_NAME: &str = \"garra_diagnostics\";")
        );
        assert!(
            runtime_source.contains("pub const CLI_INSPECT_TOOL_NAME: &str = \"cli_inspect\";")
        );
        assert!(runtime_source.contains("pub const TOOL_PROGRAM_NAME: &str = \"tool_program\";"));
        assert!(runtime_source.contains("liberadas.push(TOOL_PROGRAM_NAME.to_string())"));
        // #1712: as duas projeções leem o MESMO coletor. Antes delas o
        // `/status` contava `list_tool_info()` (tudo registrado) e esta tool
        // contava só o liberado — dois números com a mesma etiqueta. O guarda
        // falha se qualquer lado voltar a ler o inventário por conta própria.
        let visualizacao_source = include_str!("../../garraia-agents/src/ferramentas_visiveis.rs");
        assert!(visualizacao_source.contains("pub fn visao_das_ferramentas("));
        assert!(tool_source.contains("runtime.visao_das_ferramentas(turn_tools.as_deref())"));
        assert!(
            tool_source.contains("ferramenta.liberada == Some(true)"),
            "a projeção do modelo precisa filtrar fail-closed pelo coletor"
        );
        assert!(!tool_source.contains("runtime.tool_inventory()"));
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
            EstadoDeclarado::default(),
            Arc::new(Mutex::new(SnapshotDeEfeitos::default())),
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

    /// #1712 — estado declarado: a config do processo e a fonte, e um
    /// `mcp.json` de teste injeta o diretório em vez do ambiente global.
    fn estado_de_teste(mcp_json: Option<&str>) -> EstadoDeclarado {
        let dir = tempfile::tempdir().unwrap();
        if let Some(conteudo) = mcp_json {
            std::fs::write(dir.path().join("mcp.json"), conteudo).unwrap();
        }
        crate::estado_declarado::coletar(
            &garraia_config::AppConfig::default(),
            &garraia_config::ConfigLoader::with_dir(dir.path()),
        )
    }

    #[tokio::test]
    async fn declared_gateway_and_mcp_are_labelled_declared_and_leak_no_values() {
        let sentinela = ["sk", "-live-", "SENTINELA"].concat();
        let estado = estado_de_teste(Some(&format!(
            r#"{{"mcpServers": {{
                "filesystem": {{"command": "npx", "args": ["-y", "server-fs"], "env": {{"API_KEY": "{sentinela}"}}}},
                "remoto": {{"url": "https://mcp.example/sse?token={sentinela}"}}
            }}}}"#
        )));
        let runtime = Arc::new(AgentRuntime::new());
        let tool = CliInspectTool::new(
            Arc::downgrade(&runtime),
            "test".into(),
            Arc::new(RwLock::new("model".into())),
            estado,
            Arc::new(Mutex::new(SnapshotDeEfeitos::default())),
        );

        let output = tool
            .execute(&context(), json!({"scope":"all"}))
            .await
            .unwrap();
        let report: Value = serde_json::from_str(&output.content).unwrap();

        // Declarado, nunca verificado: o listen vem da config e a saúde e
        // explicitamente fora do alcance da CLI.
        assert_eq!(report["gateway"]["state"], "declared");
        assert_eq!(report["gateway"]["listen"], "127.0.0.1:3888");
        assert_eq!(
            report["gateway"]["live_verification"],
            "unavailable_from_cli"
        );
        assert_eq!(report["mcp_servers"]["state"], "declared");
        assert_eq!(
            report["mcp_servers"]["live_verification"],
            "unavailable_from_cli"
        );
        assert_eq!(report["mcp_servers"]["count"], 2);
        assert_eq!(report["mcp_servers"]["enabled_count"], 2);
        assert_eq!(report["mcp_servers"]["servers"][0]["name"], "filesystem");
        assert_eq!(report["mcp_servers"]["servers"][0]["transport"], "stdio");
        assert_eq!(report["mcp_servers"]["servers"][1]["transport"], "http");
        // Nem valor de env nem URL (que pode carregar credencial) saem no relatorio.
        assert!(!output.content.contains(&sentinela), "{}", output.content);
        assert!(
            !output.content.contains("mcp.example"),
            "{}",
            output.content
        );
    }

    #[tokio::test]
    async fn declared_mcp_list_is_capped_and_says_so() {
        let mut servidores = String::new();
        for indice in 0..(MAX_SERVIDORES + 4) {
            if indice > 0 {
                servidores.push(',');
            }
            servidores.push_str(&format!(r#""srv{indice:02}": {{"command": "npx"}}"#));
        }
        let estado = estado_de_teste(Some(&format!(r#"{{"mcpServers": {{{servidores}}}}}"#)));
        let runtime = Arc::new(AgentRuntime::new());
        let tool = CliInspectTool::new(
            Arc::downgrade(&runtime),
            "test".into(),
            Arc::new(RwLock::new("model".into())),
            estado,
            Arc::new(Mutex::new(SnapshotDeEfeitos::default())),
        );

        let report: Value = serde_json::from_str(
            &tool
                .execute(&context(), json!({"scope":"identity"}))
                .await
                .unwrap()
                .content,
        )
        .unwrap();

        // O escopo `identity` tambem carrega o estado declarado (custo fixo
        // pequeno), e a lista e limitada como a pagina de ferramentas.
        assert_eq!(report["mcp_servers"]["count"], MAX_SERVIDORES + 4);
        assert_eq!(report["mcp_servers"]["listed"], MAX_SERVIDORES);
        assert_eq!(
            report["mcp_servers"]["servers"].as_array().unwrap().len(),
            MAX_SERVIDORES
        );
        assert_eq!(report["mcp_servers"]["has_more"], true);
    }

    fn registro(
        id: &str,
        nome: &str,
        status: crate::tool_history::Status,
        turno: u32,
        ordem: u32,
    ) -> crate::tool_history::ToolRecord {
        crate::tool_history::ToolRecord {
            call_id: id.to_string(),
            name: nome.to_string(),
            input: json!({ "path": "/home/alguem/pagina.html" }),
            detail: "pagina.html".to_string(),
            status,
            summary: "713 bytes".to_string(),
            output: "713 bytes escritos em /home/alguem/pagina.html".to_string(),
            turno,
            ordem,
        }
    }

    /// #1709 — o relatório de efeitos sai da trilha do **executor**, com classe
    /// de evidência e id da chamada, e não carrega input/output/caminho.
    #[tokio::test]
    async fn tool_effects_report_executor_records_without_payload() {
        let mut snapshot = SnapshotDeEfeitos::default();
        snapshot.atualizar(&[
            registro(
                "chamada-1",
                "file_write",
                crate::tool_history::Status::Ok,
                3,
                1,
            ),
            registro(
                "chamada-2",
                "bash",
                crate::tool_history::Status::Interrupted,
                3,
                2,
            ),
        ]);
        let runtime = Arc::new(AgentRuntime::new());
        let tool = CliInspectTool::new(
            Arc::downgrade(&runtime),
            "test".into(),
            Arc::new(RwLock::new("model".into())),
            EstadoDeclarado::default(),
            Arc::new(Mutex::new(snapshot)),
        );

        let output = tool
            .execute(&context(), json!({"scope":"all"}))
            .await
            .unwrap();
        let report: Value = serde_json::from_str(&output.content).unwrap();

        assert_eq!(report["tool_effects"]["state"], "observed");
        assert_eq!(report["tool_effects"]["total_observed"], 2);
        assert_eq!(report["tool_effects"]["last_turn"], 3);
        assert_eq!(report["tool_effects"]["entries"][0]["class"], "observado");
        assert_eq!(
            report["tool_effects"]["entries"][0]["tool_call_id"],
            "chamada-1"
        );
        // Chamada interrompida nunca aparece como observada nem falhou: sem
        // resultado registrado, a classe e `nao_verificado` e nao ha id de
        // execucao comprovando resultado.
        assert_eq!(
            report["tool_effects"]["entries"][1]["class"],
            "nao_verificado"
        );
        assert_eq!(
            report["tool_effects"]["entries"][1]["tool_call_id"],
            Value::Null
        );
        // Nada de payload da ferramenta no relatorio.
        assert!(
            !output.content.contains("pagina.html"),
            "{}",
            output.content
        );
        assert!(!output.content.contains("713 bytes"), "{}", output.content);
    }

    /// Fail-closed: nenhum turno com ferramenta observado nesta sessao nao
    /// vira lista vazia silenciosa — o relatorio diz que nao foi observado.
    #[tokio::test]
    async fn tool_effects_fail_closed_when_no_turn_was_observed() {
        let runtime = Arc::new(AgentRuntime::new());
        let tool = CliInspectTool::new(
            Arc::downgrade(&runtime),
            "test".into(),
            Arc::new(RwLock::new("model".into())),
            EstadoDeclarado::default(),
            Arc::new(Mutex::new(SnapshotDeEfeitos::default())),
        );

        let output = tool
            .execute(&context(), json!({"scope":"identity"}))
            .await
            .unwrap();
        let report: Value = serde_json::from_str(&output.content).unwrap();

        assert_eq!(report["tool_effects"]["state"], "unverified");
        assert_eq!(
            report["tool_effects"]["reason"],
            "no_tool_turn_observed_in_this_session"
        );
        assert_eq!(
            report["tool_effects"]["entries"].as_array().unwrap().len(),
            0
        );
    }
}
