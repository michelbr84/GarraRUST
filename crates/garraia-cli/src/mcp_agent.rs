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
    AgentRuntime, BashTool, ChatMessage, FileJail, FileReadTool, FileWriteTool, TurnEvent,
    WebFetchTool, WebSearchTool,
};
use garraia_config::AppConfig;
use garraia_gateway::bootstrap::{ExposicaoDoBash, exposicao_do_bash, sandbox_policy_from};
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
    /// which the bash child runs (current_dir — #1075 R3).
    ///
    /// Escolhido pelo MODELO, e por isso **confinado** em
    /// [`handle_agent_call`]: tem de existir e cair dentro das raizes do
    /// jail deste servidor (`GARRAIA_MCP_ALLOWED_DIRS`, ou
    /// `agent.file_roots` mais o CWD do processo). Ele pode estreitar o
    /// jail das file tools, nunca alargar (#1244).
    ///
    /// O jail **nao** prende o `bash`, que segue sendo um shell irrestrito
    /// no host do servidor e alcanca caminho absoluto fora daqui.
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
    // Issue #1180 — same project-wide defaults `garra_ask` advertises; both
    // tools resolve through `mcp_server::resolve_overrides`.
    let provider_default = crate::defaults::DEFAULT_CLOUD_PROVIDER;
    let model_default = crate::defaults::DEFAULT_CLOUD_MODEL;
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
                "default": provider_default,
                "description": format!("LLM provider. Default '{provider_default}'.")
            },
            "model": {
                "type": "string",
                "default": model_default,
                "description": format!("Model name. Default '{model_default}' (cheap flash-tier model — the default is a spend guardrail). Pass a pricier model such as 'openrouter/auto' explicitly for complex tasks — never automatic.")
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
                "description": "Directory against which file tools resolve relative paths and in which the bash child runs. Must exist AND be inside this server's file-tool roots (GARRAIA_MCP_ALLOWED_DIRS, or agent.file_roots plus the server's CWD): it may narrow the file-tool jail, never widen it (issue #1244). The jail does not bind bash, which stays an unsandboxed shell."
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

/// O jail das file tools deste servidor MCP, a partir do
/// `GARRAIA_MCP_ALLOWED_DIRS` do operador (separado por virgula); sem ele, o
/// CWD do processo servidor mais `agent.file_roots`.
///
/// #1244 mudou o fail-open que estava documentado aqui: quando nada resolve, o
/// jail fica **vazio e nega tudo**, em vez de virar `None` = sistema de
/// arquivos inteiro. A justificativa antiga ("o `bash` irrestrito e a fronteira
/// de verdade") explicava por que o cinto era frouxo, nao por que ele podia
/// sumir — e a #1244 e sobre um modelo que le `~/.ssh/id_rsa` sem passar pelo
/// `bash`.
fn file_jail(config: &AppConfig) -> FileJail {
    if let Ok(raw) = std::env::var("GARRAIA_MCP_ALLOWED_DIRS") {
        let dirs: Vec<&str> = raw
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if !dirs.is_empty() {
            return FileJail::from_roots(dirs);
        }
    }
    FileJail::from_config_roots_plus_cwd(&config.agent.file_roots)
}

/// Register the same tool set the gateway bootstrap wires
/// (`garraia-gateway/src/bootstrap/mod.rs`): bash only where #1272 allows it
/// (a valid docker/podman sandbox, or the host of an explicit `isolated-pod`;
/// DENY_LIST + risky tier fail-closed either way — no confirmation channel in
/// a stateless MCP call, #1075 R1), file read/write,
/// web fetch, git diff, and web search when a Brave key is available.
/// `ListDirTool` is skipped on purpose: `bash ls` + the file tools
/// cover it, and a tighter tool list helps weaker models route.
///
/// O `jail` chega **pronto**, por parametro: quem o constroi e
/// [`handle_agent_call`], com a mesma instancia que validou o
/// `working_dir`. Construir um segundo aqui criaria duas reguas — a
/// validacao aprovando contra uma e as tools operando com outra (#1244,
/// rodada 4 I2).
/// Devolve tambem a decisao do `bash`, porque o system prompt e gerado a
/// partir dela (#1272).
fn build_tools(
    config: &AppConfig,
    jail: &FileJail,
) -> (Vec<Box<dyn garraia_agents::Tool>>, ExposicaoDoBash) {
    let policy = sandbox_policy_from(&config.agent.sandbox);
    let exposicao = exposicao_do_bash(config.execution.perfil(), &policy);
    let tools = build_tools_com(config, jail, policy, &exposicao);
    (tools, exposicao)
}

/// [`build_tools`] com a decisao do `bash` ja tomada — e por aqui que os
/// testes injetam a disponibilidade do backend sem depender do host.
///
/// #1272: o `bash` so entra quando `exposicao.registra_bash()`, ou seja,
/// dentro de um sandbox docker/podman valido ou no host de um `isolated-pod`
/// explicito. Em `standard` sem sandbox ele nao existe: aqui NAO ha canal de
/// confirmacao humana (#1075 R1) e o tier arriscado nao pega `cat` nem `>`.
fn build_tools_com(
    config: &AppConfig,
    jail: &FileJail,
    policy: garraia_agents::SandboxPolicy,
    exposicao: &ExposicaoDoBash,
) -> Vec<Box<dyn garraia_agents::Tool>> {
    let mut tools: Vec<Box<dyn garraia_agents::Tool>> = Vec::new();
    if exposicao.registra_bash() {
        // #1225: a policy do config chega ao BashTool; ortogonal ao jail — a
        // policy diz ONDE o comando roda, o jail diz ONDE o arquivo pode
        // estar.
        let mut bash = BashTool::new(None).with_allowlist(config.agent.bash_allowlist.clone());
        bash.set_sandbox_policy(policy.clone());
        tools.push(Box::new(bash));
    }
    tools.push(Box::new(FileReadTool::new(jail.clone())));
    tools.push(Box::new(FileWriteTool::new(jail.clone())));
    tools.push(Box::new(WebFetchTool::new(None)));
    // #1225 S2: o git_diff tambem consulta `agent.sandbox`.
    tools.push(Box::new(GitDiffTool::new(None, None).com_sandbox(policy)));
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
pub(crate) fn agent_system_prompt(
    tools: &[(String, String)],
    working_dir: Option<&str>,
    bash: &ExposicaoDoBash,
) -> String {
    let mut prompt = String::from(
        "Voce e o GarraIA, um assistente pessoal de IA criado em Rust. \
         Voce esta rodando como agente COMPLETO via MCP e pode executar \
         as ferramentas listadas abaixo para cumprir a tarefa. \
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
         Use 'file_read' para ler conteudo. \
         Nao invente resultados — execute e reporte o que aconteceu de verdade. \
         Se uma ferramenta falhar ou for bloqueada, relate isso na resposta \
         final: nunca reporte sucesso sem saida real e nunca contorne \
         silenciosamente um bloqueio de seguranca.\n",
    );
    // #1272: o que o modelo le sobre o shell tem de ser o que o codigo faz.
    prompt.push_str(&match bash {
        ExposicaoDoBash::Desligado { .. } => "\n## Shell\n\
             A ferramenta 'bash' NAO esta disponivel neste servidor (nenhum \
             sandbox docker/podman utilizavel). Nao existe outro jeito de executar comandos \
             de shell aqui: se a tarefa precisar de um, diga isso na resposta.\n"
            .to_string(),
        ExposicaoDoBash::Sandbox { .. } => "\n## Shell\n\
             O 'bash' roda dentro de um container descartavel: so o diretorio \
             de trabalho e visivel e gravavel, e o resto do host nao existe \
             para ele.\n"
            .to_string(),
        ExposicaoDoBash::HostDoPod => "\n## Shell\n\
             O 'bash' roda no host deste pod isolado (execution.profile = \
             isolated-pod); a denylist de comandos perigosos continua valendo.\n"
            .to_string(),
    });
    if let Some(dir) = working_dir {
        prompt.push_str(&format!(
            "\n## Contexto do diretorio\n\
             O chamador indicou o diretorio de trabalho: {dir}\n\
             As ferramentas de arquivo (file_read/file_write) resolvem caminhos \
             relativos contra este diretorio{}. O diretorio ja foi confinado as raizes de \
             arquivo deste servidor: as file tools nao alcancam nada fora \
             delas, e uma recusa de caminho e um bloqueio de seguranca, nao \
             um erro de digitacao — nao tente outra rota para o mesmo arquivo.\n",
            if bash.registra_bash() {
                " e o 'bash' EXECUTA nele (current_dir, hardening #1075)"
            } else {
                ""
            }
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

/// O runtime do `garra_agent`: provider + tools de [`build_tools`] + system
/// prompt gerado delas. Separado de [`agent_oneshot`] para os testes
/// adversariais da #1272 dirigirem ESTA montagem com um provider de stub.
/// `register_tool` recebe `&self` (Arc-safe); os setters `&mut` rodam antes.
fn montar_runtime(
    config: &AppConfig,
    jail: &FileJail,
    provider: Arc<dyn garraia_agents::LlmProvider>,
    working_dir: Option<&str>,
) -> AgentRuntime {
    let (tools, exposicao) = build_tools(config, jail);
    let tool_pairs: Vec<(String, String)> = tools
        .iter()
        .map(|t| (t.name().to_string(), t.description().to_string()))
        .collect();
    let mut runtime = AgentRuntime::new();
    runtime.register_provider(provider);
    for tool in tools {
        runtime.register_tool(tool);
    }
    runtime.set_system_prompt(agent_system_prompt(&tool_pairs, working_dir, &exposicao));
    runtime.set_max_tokens(4096);
    runtime
}

/// Full-agent one-shot: resolve the provider, build a fresh
/// `AgentRuntime` WITH tools, run a single turn on an empty history,
/// and return the outcome with the tool-call summary.
///
/// Shape mirrors [`crate::ask::ask_oneshot`]. Differences by design:
/// tools are registered (the caller opted in via env), the events
/// channel carries the full turn flow, and the wall-clock timeout
/// covers the whole loop.
async fn agent_oneshot(config: &AppConfig, opts: &AgentOptions, jail: &FileJail) -> AgentOutcome {
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

    // 2. Build the runtime: provider + full tool set.
    let runtime = montar_runtime(config, jail, provider, opts.working_dir.as_deref());

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

/// Confina o `working_dir` — escolhido pelo MODELO, pelo argumento da
/// tool — as raizes do `jail` deste servidor. `Ok(())` significa "pode
/// seguir"; o `Err` e texto de `invalid_params` para o chamador.
///
/// #1244 (auditoria R4, A1): o `working_dir` **vira raiz** das file tools
/// dentro de `FileJail::confine`. Sem esta checagem, `{"working_dir": "/"}`
/// devolve o disco inteiro as file tools — o oposto do fail-closed que o
/// resto da #1244 afirma.
///
/// A regra e a mesma do #1255: o `working_dir` pode **estreitar** o jail ou
/// ficar dentro dele, nunca alargar. `session_dir = None` de proposito: e
/// exatamente o valor que se esta validando, e ele nao pode se autorizar.
/// Jail sem raiz nenhuma recusa aqui tambem (`NoRoots`), que e o mesmo
/// fail-closed das tools.
fn working_dir_dentro_do_jail(jail: &FileJail, dir: &str) -> Result<(), String> {
    let is_dir = std::fs::metadata(dir).map(|m| m.is_dir()).unwrap_or(false);
    if !is_dir {
        return Err(format!(
            "working_dir does not exist or is not a directory: {dir}"
        ));
    }
    if jail.confine(std::path::Path::new(dir), None).is_err() {
        return Err(format!(
            "working_dir is outside this server's file-tool roots: {dir} — it may narrow \
             the jail, never widen it (issue #1244). Allow it by adding the directory to \
             GARRAIA_MCP_ALLOWED_DIRS or to agent.file_roots in config.yml, or by starting \
             the server with its CWD inside it — the CWD is always a root."
        ));
    }
    Ok(())
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
    // Uma construcao so do jail por chamada: a MESMA instancia valida o
    // `working_dir` e vai para as file tools em `build_tools`. Enquanto
    // eram duas chamadas a `file_jail`, a validacao podia aprovar contra
    // uma regua e as tools operarem com outra sem nenhum teste notar
    // (#1244, rodada 4 I2).
    let jail = file_jail(config);
    if let Some(ref dir) = args.working_dir {
        working_dir_dentro_do_jail(&jail, dir)?;
    }
    let opts = AgentOptions {
        message: args.message,
        provider,
        model,
        timeout_secs,
        system_prompt: args.system_prompt,
        working_dir: args.working_dir,
    };
    let outcome = agent_oneshot(config, &opts, &jail).await;
    let envelope = outcome.to_envelope();
    Ok((envelope, outcome.is_ok()))
}

#[cfg(test)]
mod tests {
    //! Zero network, zero env-mutation, zero turno de agente. Quase todos
    //! puros; os dois testes do jail do `working_dir` (#1244 A1) **leem** o
    //! filesystem — precisam, porque o que esta sob teste e a resolucao de um
    //! diretorio real — mas nao escrevem nada e nao mexem em env.

    use super::*;

    // ─── #1225: agent.sandbox chega ao BashTool ────────────────────────

    fn config_isolated_pod() -> AppConfig {
        AppConfig {
            execution: garraia_config::ExecutionConfig::new(
                Some(garraia_config::ExecutionProfile::IsolatedPod),
                None,
            ),
            ..AppConfig::default()
        }
    }

    fn ctx_sem_dir() -> garraia_agents::tools::ToolContext {
        garraia_agents::tools::ToolContext {
            session_id: "teste-1272".into(),
            user_id: None,
            is_heartbeat: false,
            approval: Default::default(),
            working_dir: None,
            project_id: None,
        }
    }

    /// `isolated-pod` + `mode = all` sem `backend`: o sandbox e EXIGIDO e
    /// impossivel, entao o `bash` NAO e registrado — antes ele entrava como
    /// `HostDoPod` e recusava todo comando, com o system prompt anunciando um
    /// shell no host do pod (review da #1272, SANDBOX-11/13).
    #[test]
    fn isolated_pod_com_sandbox_exigido_sem_backend_nao_registra_bash() {
        let mut config = config_isolated_pod();
        config.agent.sandbox.mode = garraia_config::SandboxMode::All;
        let (tools, exposicao) = build_tools(&config, &file_jail(&config));
        assert!(!tools.iter().any(|t| t.name() == "bash"));
        assert_eq!(
            exposicao,
            ExposicaoDoBash::Desligado {
                motivo: garraia_gateway::bootstrap::MotivoDoBashDesligado::SemBackend
            }
        );
    }

    /// Prova de fiacao ponta a ponta pela funcao de PRODUCAO
    /// (`build_tools_com`): a policy do config chega ao `BashTool`. Com
    /// sandbox docker exigido e sessao sem `working_dir`, todo comando e
    /// recusado fail-closed — deterministico com ou sem docker no host (sem
    /// docker o motivo e o binario; com docker, o mount vazio: o sandbox
    /// nunca monta o cwd do processo).
    #[cfg(unix)]
    #[tokio::test]
    async fn sandbox_do_config_chega_ao_bash_tool_pelo_build_tools() {
        let mut config = AppConfig::default();
        config.agent.sandbox.mode = garraia_config::SandboxMode::All;
        config.agent.sandbox.backend = Some(garraia_config::SandboxBackendKind::Docker);
        let policy = sandbox_policy_from(&config.agent.sandbox);
        let exposicao = ExposicaoDoBash::Sandbox {
            backend: garraia_agents::SandboxBackend::Docker,
        };
        let tools = build_tools_com(&config, &file_jail(&config), policy, &exposicao);
        let bash = tools
            .iter()
            .find(|t| t.name() == "bash")
            .expect("exposicao Sandbox registra a tool bash");
        let out = bash
            .execute(&ctx_sem_dir(), serde_json::json!({"command": "echo nunca"}))
            .await
            .expect("a tool devolve ToolOutput, nao Err");
        assert!(out.is_error, "sandbox obrigatorio tem de bloquear: {out:?}");
        assert!(out.content.contains("fail-closed"), "{}", out.content);
        assert!(
            !out.content.contains("nunca"),
            "o comando nao pode ter rodado no host: {}",
            out.content
        );
    }

    /// #1225 S2b: o `git_diff` do `garra_agent` recebe a policy do config.
    #[tokio::test]
    async fn sandbox_do_config_chega_ao_git_diff_pelo_build_tools() {
        let mut config = AppConfig::default();
        config.agent.sandbox.mode = garraia_config::SandboxMode::All;
        let tools = build_tools(&config, &file_jail(&config)).0;
        let git = tools
            .iter()
            .find(|t| t.name() == "git_diff")
            .expect("git_diff registrada");
        let texto = match git
            .execute(&ctx_sem_dir(), serde_json::json!({"operation": "status"}))
            .await
        {
            Ok(o) => o.content,
            Err(e) => e.to_string(),
        };
        assert!(texto.contains("nenhum backend"), "{texto}");
    }

    // ─── #1272: bash fail-closed em standard ───────────────────────────

    /// O default de toda instalacao (`standard`, sem `agent.sandbox`): o
    /// `bash` NAO e registrado; as file tools e o git seguem.
    #[test]
    fn standard_sem_sandbox_nao_registra_bash() {
        let config = AppConfig::default();
        let (tools, exposicao) = build_tools(&config, &file_jail(&config));
        let nomes: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(!nomes.contains(&"bash"), "bash em standard: {nomes:?}");
        assert!(!exposicao.registra_bash());
        for esperada in ["file_read", "file_write", "web_fetch", "git_diff"] {
            assert!(nomes.contains(&esperada), "{esperada} sumiu: {nomes:?}");
        }
    }

    /// Cada forma de "sandbox que nao isola" em `standard` deixa o bash de
    /// fora: ssh (mesmo com as flags reconhecidas), bash elevado, allowlist
    /// sem bash, e sandbox sem backend.
    #[test]
    fn standard_com_sandbox_que_nao_isola_nao_registra_bash() {
        use garraia_config::{SandboxBackendKind, SandboxMode};
        let mut ssh = AppConfig::default();
        ssh.agent.sandbox.mode = SandboxMode::All;
        ssh.agent.sandbox.backend = Some(SandboxBackendKind::Ssh);
        ssh.agent.sandbox.ssh_host = Some("box".into());
        ssh.agent.sandbox.network_disabled = false;
        ssh.agent.sandbox.mount_workdir = false;

        let mut elevado = AppConfig::default();
        elevado.agent.sandbox.mode = SandboxMode::All;
        elevado.agent.sandbox.backend = Some(SandboxBackendKind::Docker);
        elevado.agent.sandbox.elevated = vec!["bash".into()];

        let mut allowlist = AppConfig::default();
        allowlist.agent.sandbox.mode = SandboxMode::Allowlist;
        allowlist.agent.sandbox.backend = Some(SandboxBackendKind::Docker);
        allowlist.agent.sandbox.sandboxed_tools = vec!["web_fetch".into()];

        let mut sem_backend = AppConfig::default();
        sem_backend.agent.sandbox.mode = SandboxMode::All;

        for (caso, config) in [
            ("ssh", ssh),
            ("elevado", elevado),
            ("allowlist", allowlist),
            ("sem_backend", sem_backend),
        ] {
            let (tools, _) = build_tools(&config, &file_jail(&config));
            assert!(
                !tools.iter().any(|t| t.name() == "bash"),
                "{caso}: bash registrado em standard"
            );
        }
    }

    /// Gemeo positivo: com sandbox docker e o binario "presente" (decisao
    /// injetada), o bash entra.
    #[test]
    fn standard_com_sandbox_valido_registra_bash() {
        let mut config = AppConfig::default();
        config.agent.sandbox.mode = garraia_config::SandboxMode::All;
        config.agent.sandbox.backend = Some(garraia_config::SandboxBackendKind::Docker);
        let policy = sandbox_policy_from(&config.agent.sandbox);
        let exposicao = garraia_gateway::bootstrap::decidir_exposicao_do_bash(
            config.execution.perfil(),
            &policy,
            true,
            |_| true,
        );
        let tools = build_tools_com(&config, &file_jail(&config), policy, &exposicao);
        assert!(tools.iter().any(|t| t.name() == "bash"));
    }

    #[test]
    fn isolated_pod_registra_bash() {
        let config = config_isolated_pod();
        let (tools, exposicao) = build_tools(&config, &file_jail(&config));
        assert!(tools.iter().any(|t| t.name() == "bash"));
        assert_eq!(exposicao, ExposicaoDoBash::HostDoPod);
    }

    /// Provider de stub: pede, uma por rodada, as tool calls do roteiro e
    /// guarda o que voltou de cada uma e quais tools o modelo viu. Sem
    /// `async_trait` na CLI, o impl e escrito na forma que a macro gera.
    struct Roteiro {
        chamadas: Vec<(String, serde_json::Value)>,
        volta: std::sync::atomic::AtomicUsize,
        resultados: std::sync::Mutex<Vec<String>>,
        vistas: std::sync::Mutex<Vec<String>>,
    }

    impl Roteiro {
        fn novo(chamadas: Vec<(&str, serde_json::Value)>) -> Arc<Self> {
            Arc::new(Self {
                chamadas: chamadas
                    .into_iter()
                    .map(|(n, v)| (n.to_string(), v))
                    .collect(),
                volta: std::sync::atomic::AtomicUsize::new(0),
                resultados: std::sync::Mutex::new(Vec::new()),
                vistas: std::sync::Mutex::new(Vec::new()),
            })
        }

        fn responder(&self, request: &garraia_agents::LlmRequest) -> garraia_agents::LlmResponse {
            use garraia_agents::{ContentBlock, MessagePart};
            let mut resultados = self.resultados.lock().expect("lock");
            resultados.clear();
            for m in &request.messages {
                if let MessagePart::Parts(blocos) = &m.content {
                    for b in blocos {
                        if let ContentBlock::ToolResult { content, .. } = b {
                            resultados.push(content.clone());
                        }
                    }
                }
            }
            *self.vistas.lock().expect("lock") =
                request.tools.iter().map(|t| t.name.clone()).collect();
            let volta = self.volta.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let content = match self.chamadas.get(volta) {
                Some((nome, input)) => vec![ContentBlock::ToolUse {
                    id: format!("roteiro-{volta}"),
                    name: nome.clone(),
                    input: input.clone(),
                }],
                None => vec![ContentBlock::Text {
                    text: "fim do roteiro".to_string(),
                }],
            };
            garraia_agents::LlmResponse {
                content,
                model: "stub".to_string(),
                stop_reason: None,
                usage: None,
            }
        }
    }

    type Fut<'a, T> =
        std::pin::Pin<Box<dyn std::future::Future<Output = garraia_common::Result<T>> + Send + 'a>>;

    impl garraia_agents::LlmProvider for Roteiro {
        fn provider_id(&self) -> &str {
            "roteiro"
        }
        fn complete<'a, 'b, 'c>(
            &'a self,
            request: &'b garraia_agents::LlmRequest,
        ) -> Fut<'c, garraia_agents::LlmResponse>
        where
            'a: 'c,
            'b: 'c,
            Self: 'c,
        {
            let resposta = self.responder(request);
            Box::pin(async move { Ok(resposta) })
        }
        fn health_check<'a, 'c>(&'a self) -> Fut<'c, bool>
        where
            'a: 'c,
            Self: 'c,
        {
            Box::pin(async { Ok(true) })
        }
    }

    /// Roda um turno do `garra_agent` pela montagem de producao
    /// ([`montar_runtime`]) com o provider de stub. Devolve os resultados das
    /// tools (na ultima rodada) e as tools que o modelo viu.
    async fn turno(
        config: &AppConfig,
        working_dir: Option<&str>,
        chamadas: Vec<(&str, serde_json::Value)>,
    ) -> (Vec<String>, Vec<String>) {
        let n = chamadas.len();
        let provider = Roteiro::novo(chamadas);
        let jail = file_jail(config);
        let runtime = montar_runtime(config, &jail, provider.clone(), working_dir);
        let (tx, mut rx) = tokio::sync::mpsc::channel::<TurnEvent>(512);
        let drena = tokio::spawn(async move { while rx.recv().await.is_some() {} });
        let exec = ExecContext::with_working_dir(working_dir.map(str::to_string));
        let _ = runtime
            .process_message_streaming_with_events(
                "mcp-teste-1272",
                "faz o que o roteiro manda",
                &[],
                tx,
                None,
                None,
                Some("roteiro"),
                Some("stub"),
                None,
                None,
                &exec,
            )
            .await;
        drop(runtime);
        let _ = drena.await;
        let resultados = provider.resultados.lock().expect("lock").clone();
        let vistas = provider.vistas.lock().expect("lock").clone();
        assert!(
            provider.volta.load(std::sync::atomic::Ordering::SeqCst) > n,
            "o roteiro nao foi ate o fim"
        );
        (resultados, vistas)
    }

    /// Criterio de aceite da #1272, adversarial, pela montagem REAL: em
    /// `standard` sem sandbox o modelo pede `bash cat /etc/shadow`, uma
    /// escrita fora das raizes por redirecao, e um `sh -c tee` — e nada roda.
    /// Depois pede o mesmo pelas file tools, que o jail recusa.
    #[cfg(unix)]
    #[tokio::test]
    async fn standard_nega_leitura_e_escrita_fora_por_bash_e_por_file_tools() {
        let fora = tempfile::tempdir().expect("tempdir fora");
        let dentro = tempfile::tempdir().expect("tempdir dentro");
        let alvo = fora.path().join("pwned");
        let alvo_tee = fora.path().join("tee");
        let alvo_fw = fora.path().join("fw");
        let mut config = AppConfig::default();
        config.agent.file_roots = vec![dentro.path().to_string_lossy().into_owned()];
        let dir = dentro.path().to_string_lossy().into_owned();

        let (resultados, vistas) = turno(
            &config,
            Some(&dir),
            vec![
                ("bash", serde_json::json!({"command": "cat /etc/shadow"})),
                (
                    "bash",
                    serde_json::json!({"command": format!("echo pwned > {}", alvo.display())}),
                ),
                (
                    "bash",
                    serde_json::json!({
                        "command": format!("sh -c 'echo x | tee {}'", alvo_tee.display())
                    }),
                ),
                ("file_read", serde_json::json!({"path": "/etc/shadow"})),
                (
                    "file_write",
                    serde_json::json!({"path": alvo_fw.to_string_lossy(), "content": "x"}),
                ),
            ],
        )
        .await;

        assert!(
            !vistas.iter().any(|t| t == "bash"),
            "o modelo viu bash: {vistas:?}"
        );
        assert_eq!(
            resultados.len(),
            5,
            "cada chamada tem um resultado: {resultados:?}"
        );
        let tudo = resultados.join("\n");
        assert!(
            !tudo.contains("root:"),
            "conteudo de /etc/shadow vazou: {tudo}"
        );
        assert!(!alvo.exists(), "a redirecao escreveu fora das raizes");
        assert!(!alvo_tee.exists(), "o tee escreveu fora das raizes");
        assert!(!alvo_fw.exists(), "file_write escreveu fora das raizes");
    }

    /// Gemeo positivo: em `isolated-pod` explicito o mesmo turno VE o bash,
    /// roda um comando no working_dir, e a denylist continua barrando o
    /// `rm -rf` da raiz.
    #[cfg(unix)]
    #[tokio::test]
    async fn isolated_pod_roda_bash_no_working_dir_e_mantem_a_denylist() {
        let dentro = tempfile::tempdir().expect("tempdir");
        let dir = dentro.path().canonicalize().expect("canon");
        let dir_str = dir.to_string_lossy().into_owned();
        let mut config = config_isolated_pod();
        config.agent.file_roots = vec![dir_str.clone()];

        let (resultados, vistas) = turno(
            &config,
            Some(&dir_str),
            vec![
                (
                    "bash",
                    serde_json::json!({"command": "echo ok > marca; pwd"}),
                ),
                (
                    "bash",
                    serde_json::json!({"command": concat!("rm -rf", " /")}),
                ),
            ],
        )
        .await;
        assert!(vistas.iter().any(|t| t == "bash"), "{vistas:?}");
        assert!(
            dir.join("marca").exists(),
            "o bash nao rodou no working_dir: {resultados:?}"
        );
        assert!(resultados[0].contains(&dir_str), "{resultados:?}");
        assert!(
            resultados[1].contains("bloqueado"),
            "a denylist tem de continuar valendo: {resultados:?}"
        );
    }

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

    /// Issue #1180 — `garra_agent` advertises the same project-wide default
    /// as `garra_ask`; both dispatch through `resolve_overrides`, so a
    /// divergence here would be a schema that lies about the runtime.
    #[test]
    fn tool_descriptor_default_model_is_the_project_default() {
        let t = garra_agent_tool();
        let schema = (*t.input_schema).clone();
        let model_default = schema
            .get("properties")
            .and_then(|p| p.get("model"))
            .and_then(|m| m.get("default"))
            .and_then(|d| d.as_str());
        assert_eq!(model_default, Some("z-ai/glm-5.3-flash"));
        assert_eq!(model_default, Some(crate::defaults::DEFAULT_CLOUD_MODEL));
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
        let prompt = agent_system_prompt(&pairs, None, &ExposicaoDoBash::HostDoPod);
        assert!(prompt.contains("bash"));
        assert!(prompt.contains("file_read"));
        assert!(prompt.contains("## Ferramentas disponiveis"));
    }

    #[test]
    fn system_prompt_includes_working_dir_and_bash_caveat() {
        let pairs = vec![("bash".to_string(), "Executa comandos".to_string())];
        let prompt = agent_system_prompt(&pairs, Some("/tmp/projeto"), &ExposicaoDoBash::HostDoPod);
        assert!(prompt.contains("/tmp/projeto"));
        // #1075 R3: bash EXECUTA no working_dir (nao ignora mais).
        assert!(
            prompt.contains("bash' EXECUTA nele"),
            "deve dizer que bash roda no working_dir"
        );
        assert!(prompt.contains("host deste pod"), "{prompt}");
    }

    /// #1272: sem bash registrado o prompt diz que ele nao existe, nao manda
    /// usar `bash ls` e nunca promete alcance do host.
    #[test]
    fn system_prompt_sem_bash_diz_que_nao_ha_shell() {
        let pairs = vec![("file_read".to_string(), "Le".to_string())];
        let desligado = ExposicaoDoBash::Desligado {
            motivo: garraia_gateway::bootstrap::MotivoDoBashDesligado::SandboxDesligado,
        };
        let prompt = agent_system_prompt(&pairs, Some("/x"), &desligado);
        assert!(prompt.contains("'bash' NAO esta disponivel"), "{prompt}");
        assert!(!prompt.contains("EXECUTA nele"), "{prompt}");
        assert!(!prompt.contains("sem sandbox"), "{prompt}");
        assert!(!prompt.contains("Use 'bash'"), "{prompt}");
    }

    #[test]
    fn system_prompt_obriga_relatar_falhas_de_ferramenta() {
        let pairs = vec![("bash".to_string(), "Executa comandos".to_string())];
        let prompt = agent_system_prompt(&pairs, None, &ExposicaoDoBash::HostDoPod);
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

    // ─── working_dir confinado ao jail (#1244, auditoria R4 A1) ───────
    //
    // Regressao que a propria #1244 introduziu: `FileJail::confine` soma o
    // `working_dir` da sessao as raizes efetivas, e neste caminho quem
    // escreve o `working_dir` e o MODELO, pelo argumento da tool. Antes da
    // #1244 o `working_dir` do MCP so ancorava caminho relativo — nao era
    // raiz — e `working_dir: "/etc"` batia no jail.
    //
    // Os dois lados da regra ficam presos por dois testes diferentes, de
    // proposito:
    //
    // - o da recusa chama `handle_agent_call`, o handler REAL do
    //   `tools/call garra_agent`, e nao uma replica montada aqui: extrair a
    //   checagem e esquecer de chama-la derruba esse teste (medido por
    //   mutacao);
    // - o do "pode estreitar" chama `working_dir_dentro_do_jail` direto.
    //   Pelo handler ele seguiria para um TURNO DE AGENTE de verdade, com
    //   `BashTool` irrestrito (#1272) e `FileWriteTool` com raiz no CWD do
    //   processo: numa maquina com Ollama de pe, `cargo test` passaria a
    //   rodar shell escolhido por um LLM e poderia escrever dentro do
    //   proprio checkout. Suite unitaria nao pode ter esse efeito colateral
    //   (#1244, rodada 4 I3).

    fn politica_permissiva() -> ServerPolicy {
        ServerPolicy::from_values(None, None, Some("1"))
    }

    fn args_com_working_dir(dir: &str) -> GarraAgentArgs {
        GarraAgentArgs {
            message: "ping".into(),
            provider: Some("ollama".into()),
            model: None,
            timeout_secs: Some(AGENT_TIMEOUT_SECS_MIN),
            system_prompt: None,
            working_dir: Some(dir.to_string()),
        }
    }

    /// O ataque literal da auditoria: `{"working_dir": "/"}` devolvia o disco
    /// inteiro as file tools. Tem de morrer em `invalid_params`, ANTES de
    /// qualquer turno — por isso a assercao e sobre `Err`, que so existe no
    /// ramo de validacao.
    #[tokio::test]
    async fn working_dir_raiz_do_sistema_e_recusado() {
        let config = AppConfig::default();
        // Pre-condicao: se o operador desligou o jail apontando uma raiz
        // para `/`, nao ha o que confinar — e o teste estaria medindo outra
        // coisa. Ver `FileJail::raizes_perigosas`.
        let jail = file_jail(&config);
        assert!(
            jail.raizes_perigosas().is_empty(),
            "ambiente de teste com jail desligado (raiz / ou $HOME): {:?}",
            jail.roots()
        );

        let err = handle_agent_call(&config, &politica_permissiva(), args_com_working_dir("/"))
            .await
            .expect_err("working_dir = / precisa ser recusado antes de rodar o agente");
        assert!(
            err.contains("outside") && err.contains("#1244"),
            "mensagem deve dizer que o working_dir esta fora das raizes: {err}"
        );
    }

    /// O contrapeso: a regra e "pode estreitar ou ficar dentro", nao "nunca
    /// aceita". Um `working_dir` que E uma raiz do jail passa a validacao.
    /// Sem este teste a guarda poderia virar um `deny-all` sem nada ficar
    /// vermelho (medido: mutando a guarda para recusar tudo, a recusa acima
    /// continua verde e este aqui cai).
    #[test]
    fn working_dir_dentro_das_raizes_passa_pela_validacao() {
        let config = AppConfig::default();
        let jail = file_jail(&config);
        let raiz = jail
            .roots()
            .first()
            .expect("o jail do MCP tem ao menos o CWD do processo")
            .clone();
        let dir = raiz.to_str().expect("CWD com caminho UTF-8");

        working_dir_dentro_do_jail(&jail, dir)
            .expect("working_dir dentro da raiz nao pode virar invalid_params");
    }

    /// Doc e codigo tem de dizer a mesma coisa. A frase antiga ("Validated
    /// for existence only — not against allowed_dirs") foi escrita quando o
    /// `working_dir` nao era raiz; depois da #1244 ela instruia o modelo a
    /// usar exatamente o buraco.
    #[test]
    fn schema_do_working_dir_nao_promete_fail_open() {
        let t = garra_agent_tool();
        let schema = (*t.input_schema).clone();
        let desc = schema
            .get("properties")
            .and_then(|p| p.get("working_dir"))
            .and_then(|w| w.get("description"))
            .and_then(|d| d.as_str())
            .expect("working_dir tem description")
            .to_string();
        assert!(
            !desc.contains("not against allowed_dirs"),
            "schema ainda promete fail-open: {desc}"
        );
        assert!(
            desc.contains("narrow") && desc.contains("never widen"),
            "schema deve declarar a regra do #1244: {desc}"
        );
    }

    /// A terceira copia da mesma frase, e a de maior alavancagem: esta e
    /// injetada no system prompt a cada turno, e portanto e o texto que o
    /// MODELO le. Um modelo que leia "validado apenas quanto a existencia"
    /// trata a recusa do jail como erro de caminho e roteia pelo `bash`,
    /// que de fato passa por fora (#1272) — exatamente o defeito que a
    /// #1244 cita como justificativa.
    ///
    /// Esta varredura e sobre a string PRODUZIDA em runtime, nao sobre o
    /// fonte: ela le o mesmo artefato que vai para o modelo.
    #[test]
    fn system_prompt_do_working_dir_nao_promete_fail_open() {
        let pairs = vec![("bash".to_string(), "Executa comandos".to_string())];
        let prompt = agent_system_prompt(&pairs, Some("/x"), &ExposicaoDoBash::HostDoPod);
        assert!(
            !prompt.contains("validado apenas quanto a existencia"),
            "system prompt ainda promete fail-open: {prompt}"
        );
        assert!(
            prompt.contains("confinado as raizes"),
            "prompt deve dizer que o working_dir e confinado ao jail: {prompt}"
        );
        // #1272: o prompt nao promete mais que o bash alcanca o host "sem
        // sandbox" — ele so existe sandboxado ou no host de um pod.
        assert!(
            !prompt.contains("sem sandbox"),
            "prompt ainda promete bash irrestrito: {prompt}"
        );
    }
}
