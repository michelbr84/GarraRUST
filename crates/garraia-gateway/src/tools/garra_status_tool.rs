//! `garra_status` — the agent's view of its own runtime.
//!
//! Field report, v0.4.0 on a phone: asked what it was running, the Garra
//! answered that it "cannot inspect its own runtime from this conversation".
//! It was right — nothing let it. `/api/health` and `/api/capabilities`
//! exist for humans and the console; `web_fetch` refuses loopback by design
//! (SSRF guard, with a regression test); `bash` would have to guess host and
//! port. This tool closes the gap from the inside: it reads the same
//! `AppState` those endpoints read, and nothing else. No request is made, so
//! the SSRF surface stays exactly where it was.
//!
//! Secret-free by construction: provider *ids* and model *names* only —
//! never keys, never URLs with credentials, and no path beyond the session's
//! own working directory (which the session set itself).

use std::sync::{Arc, Weak};

use async_trait::async_trait;
use garraia_agents::tools::{Tool, ToolContext, ToolOutput};
use garraia_common::Result;

use crate::capabilities::{feature_flags, feature_inputs};
use crate::state::AppState;

pub struct GarraStatusTool {
    /// Weak for the same reason `TelegramSendTool` is: `AppState` owns the
    /// runtime, the runtime owns this tool, and a strong handle back would
    /// close an `Arc` cycle that leaks the whole gateway state.
    state: Weak<AppState>,
}

impl GarraStatusTool {
    pub fn new(state: &Arc<AppState>) -> Self {
        Self {
            state: Arc::downgrade(state),
        }
    }
}

#[async_trait]
impl Tool for GarraStatusTool {
    fn name(&self) -> &str {
        "garra_status"
    }

    fn description(&self) -> &str {
        "Describes the Garra runtime you are running in: version, uptime, active \
         provider and model, the tools available in this turn, advertised features, \
         connected channels, and this session's mode and working directory. Use it whenever \
         the user asks what you are, what you can do, or how you are configured — \
         instead of guessing or saying you cannot inspect yourself. Takes no input."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    async fn execute(&self, ctx: &ToolContext, _input: serde_json::Value) -> Result<ToolOutput> {
        let Some(state) = self.state.upgrade() else {
            return Ok(ToolOutput::error("gateway state is gone (shutting down?)"));
        };

        let provider = state.agents.default_provider_id();
        let model = provider
            .as_deref()
            .and_then(|p| state.agents.get_provider(p))
            .and_then(|p| p.configured_model().map(str::to_string));

        let mut providers = state.agents.provider_ids();
        providers.sort();
        providers.dedup();

        // #1347 (revisao da onda A): as ferramentas que o portao DESTE turno
        // libera, publicadas pelo runtime em volta do `execute`. Nos modos
        // restritos (o piso `search` do WhatsApp) a lista registrada traz
        // `bash`/`file_write`, que o turno nega, e o modelo e mandado
        // responder a partir deste relatorio. Fora de um turno do runtime
        // (teste, chamada direta) nao ha portao a consultar, e vale a lista
        // registrada, como antes.
        let mut tools = garraia_agents::tools::turn_tools::ferramentas_do_turno()
            .unwrap_or_else(|| state.agents.tool_names());
        tools.sort();

        let features = feature_flags(&feature_inputs(&state));
        // The only writers of `channels` after boot are the connect-retry
        // tasks in `server.rs`; a read here waits for one to finish, which is
        // milliseconds, never a cycle — the retry task takes nothing this tool
        // holds.
        let channels: Vec<String> = state
            .channels
            .read()
            .await
            .list()
            .into_iter()
            .map(|s| s.to_string())
            .collect();
        let mode = state.chosen_agent_mode_for(&ctx.session_id).await;

        let report = serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "uptime_secs": state.boot_time.elapsed().as_secs(),
            "provider": provider,
            "model": model,
            "providers": providers,
            "tools": tools,
            "features": features,
            "channels": channels,
            "memory_enabled": state.agents.memory_provider().is_some(),
            "session": {
                "id": ctx.session_id,
                "mode": mode,
                "working_dir": ctx.working_dir,
                "project_id": ctx.project_id,
            },
        });

        let text = serde_json::to_string_pretty(&report).unwrap_or_else(|_| report.to_string());
        Ok(ToolOutput::success(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_agents::AgentRuntime;
    use garraia_channels::ChannelRegistry;
    use garraia_config::AppConfig;

    fn state() -> Arc<AppState> {
        Arc::new(AppState::new(
            AppConfig::default(),
            Arc::new(AgentRuntime::new()),
            ChannelRegistry::new(),
        ))
    }

    fn ctx(working_dir: Option<&str>) -> ToolContext {
        ToolContext {
            session_id: "sessao-teste".to_string(),
            user_id: None,
            is_heartbeat: false,
            approval: garraia_agents::tools::approval::ToolApproval::None,
            working_dir: working_dir.map(str::to_string),
            project_id: None,
        }
    }

    /// O relatorio traz o que o console ve — e o que o agente nao via.
    #[tokio::test]
    async fn relata_versao_ferramentas_e_diretorio_da_sessao() {
        let st = state();
        // Two instances on purpose: the registered one is what `tool_names()`
        // must list; the local one is executed directly, without the runtime
        // dispatch, so the test reads the report and nothing else.
        st.agents.register_tool(Box::new(GarraStatusTool::new(&st)));
        let tool = GarraStatusTool::new(&st);

        let out = tool
            .execute(&ctx(Some("/tmp/garra-projeto")), serde_json::json!({}))
            .await
            .expect("executa");
        assert!(!out.is_error, "{}", out.content);

        let json: serde_json::Value = serde_json::from_str(&out.content).expect("json");
        assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(json["session"]["id"], "sessao-teste");
        assert_eq!(json["session"]["working_dir"], "/tmp/garra-projeto");
        assert!(
            json["tools"]
                .as_array()
                .is_some_and(|t| t.iter().any(|n| n == "garra_status")),
            "a propria ferramenta aparece na lista: {}",
            json["tools"]
        );
        // Os always-on do contrato mobile vem junto.
        assert!(
            json["features"]
                .as_array()
                .is_some_and(|f| f.iter().any(|n| n == "chat")),
            "{}",
            json["features"]
        );
    }

    /// Revisao da onda A: dentro de um turno, `tools` e o que o portao do
    /// turno libera, nao tudo que esta registrado — no piso `search`, `bash`
    /// e `file_write` ficam de fora.
    #[tokio::test]
    async fn dentro_do_turno_relata_so_as_ferramentas_liberadas() {
        let st = state();
        st.agents.register_tool(Box::new(GarraStatusTool::new(&st)));
        let registradas = st.agents.tool_names();
        let portao = garraia_agents::modes::ToolGate::for_mode_name("search");
        let liberadas: Vec<String> = ["bash", "file_write", "file_read", "garra_status"]
            .into_iter()
            .map(str::to_string)
            .filter(|n| portao.permite(n))
            .collect();
        assert!(registradas.iter().any(|n| n == "garra_status"));

        let tool = GarraStatusTool::new(&st);
        let ctx = ctx(None);
        let out = garraia_agents::tools::turn_tools::com_ferramentas_do_turno(
            liberadas,
            tool.execute(&ctx, serde_json::json!({})),
        )
        .await
        .expect("executa");
        let json: serde_json::Value = serde_json::from_str(&out.content).expect("json");
        assert_eq!(
            json["tools"],
            serde_json::json!(["file_read", "garra_status"]),
            "{}",
            json["tools"]
        );
    }

    /// A nota do runtime (#1347) fala do formato deste relatorio: a lista
    /// `channels`. Quando a fatia do gateway trocar o formato (estado por
    /// canal em `status`, `active`/`offline`), este teste e a nota mudam
    /// juntos.
    #[tokio::test]
    async fn a_nota_do_runtime_casa_com_o_formato_de_channels() {
        let st = state();
        let tool = GarraStatusTool::new(&st);
        let out = tool
            .execute(&ctx(None), serde_json::json!({}))
            .await
            .expect("executa");
        let json: serde_json::Value = serde_json::from_str(&out.content).expect("json");
        assert!(json["channels"].is_array(), "{}", json["channels"]);
        for nota in [
            garraia_agents::NOTA_GARRA_STATUS_PT,
            garraia_agents::NOTA_GARRA_STATUS_EN,
        ] {
            assert!(nota.contains("`channels`"), "{nota}");
            assert!(nota.contains("`status`"), "{nota}");
            assert!(nota.contains("`active`"), "{nota}");
            assert!(nota.contains("`offline`"), "{nota}");
        }
    }

    /// Nada de segredo: sem provider configurado, `provider`/`model` sao null,
    /// e a saida nunca carrega chave — o teste garante que os campos que
    /// existem sao so os enumerados aqui.
    #[tokio::test]
    async fn saida_e_secret_free_e_tem_so_os_campos_previstos() {
        let st = state();
        let tool = GarraStatusTool::new(&st);
        let out = tool
            .execute(&ctx(None), serde_json::json!({}))
            .await
            .expect("executa");
        let json: serde_json::Value = serde_json::from_str(&out.content).expect("json");
        let mut keys: Vec<&str> = json
            .as_object()
            .expect("objeto")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "channels",
                "features",
                "memory_enabled",
                "model",
                "provider",
                "providers",
                "session",
                "tools",
                "uptime_secs",
                "version",
            ]
        );
        assert!(json["provider"].is_null());
        assert!(json["model"].is_null());
        assert!(json["session"]["working_dir"].is_null());
    }

    /// Estado derrubado nao vira panic: a tool devolve erro legivel.
    #[tokio::test]
    async fn estado_derrubado_vira_erro_legivel() {
        let st = state();
        let tool = GarraStatusTool::new(&st);
        drop(st);
        let out = tool
            .execute(&ctx(None), serde_json::json!({}))
            .await
            .expect("executa");
        assert!(out.is_error);
        assert!(out.content.contains("gateway state is gone"));
    }
}
