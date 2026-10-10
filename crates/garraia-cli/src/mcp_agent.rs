//! `garra_agent` — a superfície STDIO da tool de agente completo (opt-in).
//!
//! O núcleo (provider, tools, system prompt, o laço de execução e o envelope
//! `garra.agent.v1`) mora em [`garraia_gateway::agente_mcp`] desde a #1615, para
//! a ponte MCP HTTP poder rodar o mesmo agente. Aqui ficam só as coisas do
//! transporte stdio: o descritor de `tools/list`, a forma dos argumentos, a
//! validação de `working_dir` contra o jail deste processo e o despacho de
//! `tools/call`.
//!
//! # Divergencia deliberada dos invariantes do `mcp_server.rs`
//!
//! O `mcp_server.rs` e auditado por dois testes que varrem a propria
//! producao e proibem: registrar ferramentas de runtime, spawnar
//! subprocessos e escrever no stdout. Este modulo NAO e escaneado por
//! aqueles testes — e por desenho: o agente registra ferramentas de
//! verdade (`bash`, `file_read`, `file_write`, `web_fetch`, `git_diff`,
//! `web_search` opcional) e o `bash` spawna processos via o `BashTool`
//! nativo do runtime (mesmo safety_gate do gateway). A auditoria continua
//! valendo para `mcp_server.rs` e `ask.rs`, que permanecem LLM-only; o
//! núcleo do agente vive em `garraia-gateway` e tem a sua propria guarda.
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

use garraia_agents::FileJail;
use garraia_config::AppConfig;
use garraia_gateway::agente_mcp::{AgentOptions, AgentOutcome, agent_oneshot};
use rmcp::model::Tool;
use serde::Deserialize;
use serde_json::{Map as JsonMap, Value as JsonValue, json};

use crate::mcp_server::{PROVIDER_ENUM, ServerPolicy, resolve_overrides, validate_agent_policy};

pub(crate) use garraia_gateway::agente_mcp::{
    AGENT_MESSAGE_MAX_BYTES, AGENT_SYSTEM_PROMPT_MAX_BYTES, AGENT_TIMEOUT_SECS_DEFAULT,
    AGENT_TIMEOUT_SECS_MAX, AGENT_TIMEOUT_SECS_MIN,
};

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
    let outcome: AgentOutcome = agent_oneshot(config, &opts, &jail, "mcp").await;
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
}
