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
//!
//! ## Canais (#1347, fatia 2)
//!
//! `channels` sai da MESMA funcao que o `GET /api/channels`
//! ([`crate::channels_view::channel_rows`]), com o status de cada canal
//! (`active` / `offline`). Antes lia so o `ChannelRegistry`, onde o
//! `whatsapp_linked` e os canais push nunca entram — e o Garra conectado ao
//! WhatsApp respondia que nao tinha acesso ao WhatsApp.
//!
//! ## Turno restrito ou canal remoto (#1347, fatia 3)
//!
//! No piso somente leitura de um canal (o `search` do WhatsApp) quem pergunta
//! nao e necessariamente o operador: e qualquer remetente que o portao do
//! canal deixou passar. Nesses turnos o relatorio retem o que so interessa
//! ao operador — `working_dir`, `project_id`, a lista `providers`, os nomes
//! dos `mcp_servers` e a versao exata (sai so `major.minor`) — e diz o que
//! reteve em `withheld`, para o modelo nao confundir "retido" com "nao ha".
//! O turno conta como restrito quando o portao do runtime restringe por
//! whitelist ([`turno_restrito`]) OU quando a sessao e de um canal remoto
//! ([`crate::channels_view::is_remote_channel`]). O `session.id` e mascarado
//! sempre: o do WhatsApp e o numero de telefone inteiro.
//!
//! [`turno_restrito`]: garraia_agents::tools::turn_tools::turno_restrito

use std::sync::{Arc, Weak};

use async_trait::async_trait;
use garraia_agents::tools::{Tool, ToolContext, ToolOutput};
use garraia_common::Result;

use crate::capabilities::{feature_flags, feature_inputs};
use crate::push_channels::PushMounted;
use crate::state::AppState;

/// Os campos que um turno restrito nao recebe, na ordem em que aparecem em
/// `withheld`.
const RETIDOS_NO_TURNO_RESTRITO: &[&str] = &[
    "version_patch",
    "providers",
    "mcp_servers",
    "session.working_dir",
    "session.project_id",
];

pub struct GarraStatusTool {
    /// Weak for the same reason `TelegramSendTool` is: `AppState` owns the
    /// runtime, the runtime owns this tool, and a strong handle back would
    /// close an `Arc` cycle that leaks the whole gateway state.
    state: Weak<AppState>,
    /// So as contagens dos canais push, nunca o `PushChannelStates`: cada
    /// canal push segura um `Arc<AppState>` forte no `on_message`, e guarda-lo
    /// aqui fecharia o mesmo ciclo que o `Weak` acima evita. Ver
    /// [`crate::push_channels::PushChannelStates::contagens`].
    push: PushMounted,
}

impl GarraStatusTool {
    /// `push` e o [`crate::push_channels::PushChannelStates::contagens`] do
    /// boot: o `server.rs` registra a tool depois de montar os canais push
    /// (#1347), para o relatorio e o `/api/channels` verem os mesmos.
    pub fn new(state: &Arc<AppState>, push: PushMounted) -> Self {
        Self {
            state: Arc::downgrade(state),
            push,
        }
    }
}

/// Mascara toda sequencia de 6 ou mais digitos, deixando os 4 ultimos:
/// `whatsapp-linked-5511987654321@s.whatsapp.net` vira
/// `whatsapp-linked-***4321@s.whatsapp.net`. E a convencao `phone_last4` dos
/// logs; pega telefone, id de chat do Telegram e LID do WhatsApp sem
/// precisar saber o formato de cada canal.
fn mascarar_digitos(id: &str) -> String {
    const MINIMO: usize = 6;
    const VISIVEIS: usize = 4;
    fn despejar(seq: &mut String, out: &mut String) {
        if seq.len() >= MINIMO {
            out.push_str("***");
            out.push_str(&seq[seq.len() - VISIVEIS..]);
        } else {
            out.push_str(seq);
        }
        seq.clear();
    }
    let mut out = String::with_capacity(id.len());
    let mut sequencia = String::new();
    for c in id.chars() {
        if c.is_ascii_digit() {
            sequencia.push(c);
        } else {
            despejar(&mut sequencia, &mut out);
            out.push(c);
        }
    }
    despejar(&mut sequencia, &mut out);
    out
}

/// `major.minor` da versao do crate, para o turno restrito.
fn versao_curta() -> String {
    format!(
        "{}.{}",
        env!("CARGO_PKG_VERSION_MAJOR"),
        env!("CARGO_PKG_VERSION_MINOR")
    )
}

#[async_trait]
impl Tool for GarraStatusTool {
    fn name(&self) -> &str {
        "garra_status"
    }

    fn description(&self) -> &str {
        "Describes the Garra runtime you are running in: version, uptime, active \
         provider and model, the tools available in this turn, advertised features, \
         each channel with its status (`active` = connected now, `offline` = configured \
         but down), the execution profile, MCP servers, and this session's channel and \
         mode. Use it whenever the user asks what you are, what you can do, which \
         channels or integrations you have, or how you are configured — instead of \
         guessing or saying you cannot inspect yourself. Takes no input."
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

        let canal_da_sessao = crate::channels_view::channel_of_session(&ctx.session_id);
        // Os dois sinais valem: o portao do turno (piso somente leitura,
        // modo restrito) e o canal da sessao. Fora de um turno do runtime
        // (`None`) nao ha portao a consultar e vale so o canal.
        let restrito = garraia_agents::tools::turn_tools::turno_restrito().unwrap_or(false)
            || canal_da_sessao.is_some_and(crate::channels_view::is_remote_channel);

        let provider = state.agents.default_provider_id();
        let model = provider
            .as_deref()
            .and_then(|p| state.agents.get_provider(p))
            .and_then(|p| p.configured_model().map(str::to_string));

        let providers = (!restrito).then(|| {
            let mut providers = state.agents.provider_ids();
            providers.sort();
            providers.dedup();
            providers
        });

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
        // A mesma leitura do `/api/channels`. Canal `optional` (ninguem
        // ligou) fica de fora: o relatorio lista o que existe neste Garra.
        let channels: Vec<serde_json::Value> =
            crate::channels_view::channel_rows(&state, self.push)
                .await
                .into_iter()
                .filter(|row| row.status != "optional")
                .map(|row| {
                    serde_json::json!({
                        "id": row.id,
                        "name": row.display_name,
                        "status": row.status,
                    })
                })
                .collect();

        let mcp_servers = if restrito {
            None
        } else {
            let mut servers = Vec::new();
            if let Some(mgr) = &state.mcp_manager_arc {
                // So nome, estado e contagem: `last_error`/`cause` podem citar
                // caminho ou comando da config do operador.
                for s in mgr.server_statuses().await {
                    servers.push(serde_json::json!({
                        "name": s.name,
                        "connected": s.state == garraia_agents::McpServerState::Connected,
                        "tools": s.tool_count,
                    }));
                }
            }
            Some(servers)
        };

        let execution_profile = crate::bootstrap::politica_de_execucao(&state.config)
            .perfil
            .as_str();
        let mode = state.chosen_agent_mode_for(&ctx.session_id).await;
        let withheld: &[&str] = if restrito {
            RETIDOS_NO_TURNO_RESTRITO
        } else {
            &[]
        };
        let version = if restrito {
            versao_curta()
        } else {
            env!("CARGO_PKG_VERSION").to_string()
        };
        let working_dir = (!restrito).then_some(ctx.working_dir.as_deref()).flatten();
        let project_id = (!restrito).then_some(ctx.project_id.as_deref()).flatten();

        let report = serde_json::json!({
            "version": version,
            "uptime_secs": state.boot_time.elapsed().as_secs(),
            "provider": provider,
            "model": model,
            "providers": providers,
            "tools": tools,
            "features": features,
            "channels": channels,
            "execution_profile": execution_profile,
            "mcp_servers": mcp_servers,
            "memory_enabled": state.agents.memory_provider().is_some(),
            "session": {
                "id": mascarar_digitos(&ctx.session_id),
                "channel": canal_da_sessao,
                "mode": mode,
                "working_dir": working_dir,
                "project_id": project_id,
            },
            "withheld": withheld,
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
    use garraia_channels::whatsapp_linked::health::BridgeView;
    use garraia_config::AppConfig;

    fn state() -> Arc<AppState> {
        state_com(AppConfig::default())
    }

    fn state_com(config: AppConfig) -> Arc<AppState> {
        // Com data dir de teste, o config dir e o mesmo tempdir: o
        // `AppState` provisiona `mcp.json` no config dir, e sem isto o
        // gravaria no do desenvolvedor (ver `AppState::with_config_dir`).
        match config.data_dir.clone() {
            Some(dir) => Arc::new(AppState::with_config_dir(
                config,
                Arc::new(AgentRuntime::new()),
                ChannelRegistry::new(),
                &dir,
            )),
            None => Arc::new(AppState::new(
                config,
                Arc::new(AgentRuntime::new()),
                ChannelRegistry::new(),
            )),
        }
    }

    fn ctx(working_dir: Option<&str>) -> ToolContext {
        ctx_na_sessao("sessao-teste", working_dir)
    }

    fn ctx_na_sessao(session_id: &str, working_dir: Option<&str>) -> ToolContext {
        ToolContext {
            session_id: session_id.to_string(),
            user_id: None,
            is_heartbeat: false,
            approval: garraia_agents::tools::approval::ToolApproval::None,
            working_dir: working_dir.map(str::to_string),
            project_id: None,
        }
    }

    fn tool(st: &Arc<AppState>) -> GarraStatusTool {
        GarraStatusTool::new(st, PushMounted::default())
    }

    async fn relatorio(tool: &GarraStatusTool, ctx: &ToolContext) -> serde_json::Value {
        let out = tool
            .execute(ctx, serde_json::json!({}))
            .await
            .expect("executa");
        assert!(!out.is_error, "{}", out.content);
        serde_json::from_str(&out.content).expect("json")
    }

    /// O relatorio dentro de um turno do runtime, com o bit `restrito` do
    /// portao.
    async fn relatorio_no_turno(
        tool: &GarraStatusTool,
        ctx: &ToolContext,
        restrito: bool,
    ) -> (serde_json::Value, String) {
        let out = garraia_agents::tools::turn_tools::com_ferramentas_do_turno(
            vec!["file_read".to_string(), "garra_status".to_string()],
            restrito,
            tool.execute(ctx, serde_json::json!({})),
        )
        .await
        .expect("executa");
        assert!(!out.is_error, "{}", out.content);
        (
            serde_json::from_str(&out.content).expect("json"),
            out.content,
        )
    }

    fn canal<'a>(json: &'a serde_json::Value, id: &str) -> Option<&'a serde_json::Value> {
        json["channels"]
            .as_array()
            .expect("channels e lista")
            .iter()
            .find(|c| c["id"] == id)
    }

    /// Grava um `session.enc` e um `node_modules` de mentira: os dois fatos de
    /// disco que a classificacao do `whatsapp_linked` le (o mesmo `vincula`
    /// dos testes do `/api/channels`).
    fn vincula(dir: &std::path::Path) {
        let store = garraia_channels::whatsapp_linked::SessionStore::for_data_dir(
            dir,
            garraia_channels::whatsapp_linked::DEFAULT_ACCOUNT,
        )
        .expect("DEFAULT_ACCOUNT e um segmento valido");
        let key = garraia_channels::whatsapp_linked::SessionKey::resolve(store.dir(), None)
            .expect("chave");
        store
            .save(
                &garraia_channels::whatsapp_linked::SessionBlob::new("eyJhIjoxfQ=="),
                &key,
            )
            .expect("grava sessao");
        std::fs::create_dir_all(dir.join("whatsapp/bridge/node_modules")).expect("node_modules");
    }

    fn config_no(dir: &std::path::Path) -> AppConfig {
        AppConfig {
            data_dir: Some(dir.to_path_buf()),
            ..Default::default()
        }
    }

    /// O relatorio traz o que o console ve — e o que o agente nao via.
    #[tokio::test]
    async fn relata_versao_ferramentas_e_diretorio_da_sessao() {
        let st = state();
        // Two instances on purpose: the registered one is what `tool_names()`
        // must list; the local one is executed directly, without the runtime
        // dispatch, so the test reads the report and nothing else.
        st.agents.register_tool(Box::new(tool(&st)));
        let tool = tool(&st);

        let json = relatorio(&tool, &ctx(Some("/tmp/garra-projeto"))).await;
        assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(json["session"]["id"], "sessao-teste");
        assert_eq!(json["session"]["working_dir"], "/tmp/garra-projeto");
        assert!(json["session"]["channel"].is_null());
        assert_eq!(json["execution_profile"], "standard");
        assert_eq!(json["mcp_servers"], serde_json::json!([]));
        assert_eq!(json["withheld"], serde_json::json!([]));
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
        st.agents.register_tool(Box::new(tool(&st)));
        let registradas = st.agents.tool_names();
        let portao = garraia_agents::modes::ToolGate::for_mode_name("search");
        let liberadas: Vec<String> = ["bash", "file_write", "file_read", "garra_status"]
            .into_iter()
            .map(str::to_string)
            .filter(|n| portao.permite(n))
            .collect();
        assert!(registradas.iter().any(|n| n == "garra_status"));

        let tool = tool(&st);
        let ctx = ctx(None);
        let out = garraia_agents::tools::turn_tools::com_ferramentas_do_turno(
            liberadas,
            portao.restringe_por_whitelist(),
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

    /// A nota do runtime (#1347) fala do formato deste relatorio: cada item
    /// de `channels` com um `status` `active`/`offline`, e `withheld`. Se um
    /// lado mudar, este teste cai junto.
    #[tokio::test]
    async fn a_nota_do_runtime_casa_com_o_formato_de_channels() {
        let dir = tempfile::tempdir().expect("tempdir");
        vincula(dir.path());
        let st = state_com(config_no(dir.path()));
        st.whatsapp_linked.set_bridge(BridgeView::Down);
        let json = relatorio(&tool(&st), &ctx(None)).await;
        let linha = canal(&json, "whatsapp_linked").expect("canal ligado aparece");
        assert!(linha["status"].is_string(), "{linha}");
        assert!(json["withheld"].is_array(), "{}", json["withheld"]);
        for nota in [
            garraia_agents::NOTA_GARRA_STATUS_PT,
            garraia_agents::NOTA_GARRA_STATUS_EN,
        ] {
            assert!(nota.contains("`channels`"), "{nota}");
            assert!(nota.contains("`status`"), "{nota}");
            assert!(nota.contains("`active`"), "{nota}");
            assert!(nota.contains("`offline`"), "{nota}");
            assert!(nota.contains("`withheld`"), "{nota}");
        }
    }

    /// Nada de segredo: sem provider configurado, `provider`/`model` sao null,
    /// e a saida nunca carrega chave — o teste garante que os campos que
    /// existem sao so os enumerados aqui.
    #[tokio::test]
    async fn saida_e_secret_free_e_tem_so_os_campos_previstos() {
        let st = state();
        let json = relatorio(&tool(&st), &ctx(None)).await;
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
                "execution_profile",
                "features",
                "mcp_servers",
                "memory_enabled",
                "model",
                "provider",
                "providers",
                "session",
                "tools",
                "uptime_secs",
                "version",
                "withheld",
            ]
        );
        let mut sessao: Vec<&str> = json["session"]
            .as_object()
            .expect("objeto")
            .keys()
            .map(String::as_str)
            .collect();
        sessao.sort_unstable();
        assert_eq!(
            sessao,
            ["channel", "id", "mode", "project_id", "working_dir"]
        );
        assert!(json["provider"].is_null());
        assert!(json["model"].is_null());
        assert!(json["session"]["working_dir"].is_null());
    }

    // ─── #1347: o `whatsapp_linked` no relatorio ──────────────────────────

    /// O sintoma da issue: vinculado e com a ponte de pe, o canal sai
    /// `active` — antes nem aparecia, porque so o `ChannelRegistry` era lido.
    #[tokio::test]
    async fn whatsapp_linked_conectado_sai_active() {
        let dir = tempfile::tempdir().expect("tempdir");
        vincula(dir.path());
        let st = state_com(config_no(dir.path()));
        st.whatsapp_linked.set_bridge(BridgeView::Connected);
        let json = relatorio(&tool(&st), &ctx(None)).await;
        let linha = canal(&json, "whatsapp_linked").expect("o canal aparece");
        assert_eq!(linha["status"], "active", "{json}");
    }

    /// Ninguem vinculou: o canal nao aparece (e `optional` no console).
    #[tokio::test]
    async fn whatsapp_linked_sem_sessao_fica_de_fora() {
        let dir = tempfile::tempdir().expect("tempdir");
        let st = state_com(config_no(dir.path()));
        st.whatsapp_linked.set_bridge(BridgeView::Connected);
        let json = relatorio(&tool(&st), &ctx(None)).await;
        assert!(canal(&json, "whatsapp_linked").is_none(), "{json}");
    }

    /// Vinculado e com a ponte caida: `offline`, nao `active` e nao ausente.
    #[tokio::test]
    async fn whatsapp_linked_com_ponte_caida_sai_offline() {
        let dir = tempfile::tempdir().expect("tempdir");
        vincula(dir.path());
        let st = state_com(config_no(dir.path()));
        st.whatsapp_linked.set_bridge(BridgeView::Down);
        let json = relatorio(&tool(&st), &ctx(None)).await;
        let linha = canal(&json, "whatsapp_linked").expect("o canal aparece");
        assert_eq!(linha["status"], "offline", "{json}");
    }

    /// Canal push montado sai `active`; os push que nao subiram e precisam de
    /// segredo saem `offline`, como no `/api/channels`.
    #[tokio::test]
    async fn canal_push_montado_sai_active() {
        let st = state();
        let push = PushMounted {
            whatsapp: 1,
            ..PushMounted::default()
        };
        let json = relatorio(&GarraStatusTool::new(&st, push), &ctx(None)).await;
        assert_eq!(
            canal(&json, "whatsapp").expect("push montado")["status"],
            "active",
            "{json}"
        );
        assert_eq!(
            canal(&json, "teams").expect("push sem canal")["status"],
            "offline",
            "{json}"
        );
    }

    /// `mcp_servers`: nome, estado e contagem — e nada da config do servidor
    /// (comando, env), nem a causa da falha.
    #[tokio::test]
    async fn mcp_servers_lista_nome_estado_e_contagem_sem_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mgr = Arc::new(garraia_agents::McpManager::new());
        let mut env = std::collections::HashMap::new();
        env.insert(
            "TOKEN_DO_MCP".to_string(),
            "mcp-segredo-plantado-7788".to_string(),
        );
        let comando = dir.path().join("comando-secreto-do-mcp");
        let comando = comando.to_string_lossy().into_owned();
        mgr.register_pending_stdio(
            "filesystem",
            &comando,
            &["--raiz-secreta".to_string()],
            &env,
            5,
            vec![],
            None,
            5,
            1,
            false,
        )
        .await;
        let mut state = AppState::with_config_dir(
            config_no(dir.path()),
            Arc::new(AgentRuntime::new()),
            ChannelRegistry::new(),
            dir.path(),
        );
        state.mcp_manager_arc = Some(mgr);
        let st = Arc::new(state);
        let tool = tool(&st);
        let out = tool
            .execute(&ctx(None), serde_json::json!({}))
            .await
            .expect("executa");
        let json: serde_json::Value = serde_json::from_str(&out.content).expect("json");
        assert_eq!(
            json["mcp_servers"],
            serde_json::json!([{ "name": "filesystem", "connected": false, "tools": 0 }]),
            "{json}"
        );
        for plantado in [
            "mcp-segredo-plantado-7788",
            "comando-secreto-do-mcp",
            "--raiz-secreta",
        ] {
            assert!(
                !out.content.contains(plantado),
                "{plantado}: {}",
                out.content
            );
        }

        // No turno restrito, nem os nomes.
        let (json, _) = relatorio_no_turno(&tool, &ctx(None), true).await;
        assert!(json["mcp_servers"].is_null(), "{json}");
    }

    // ─── #1347 (fatia 3): turno restrito e sessao de canal remoto ─────────

    #[test]
    fn mascara_sequencias_longas_de_digitos() {
        assert_eq!(
            mascarar_digitos("whatsapp-linked-5511987654321@s.whatsapp.net"),
            "whatsapp-linked-***4321@s.whatsapp.net"
        );
        assert_eq!(
            mascarar_digitos("telegram--1001234567890"),
            "telegram--***7890"
        );
        // Curtas ficam: nao sao numero de ninguem.
        assert_eq!(mascarar_digitos("sessao-12345"), "sessao-12345");
        assert_eq!(mascarar_digitos("123456"), "***3456");
        assert_eq!(mascarar_digitos(""), "");
    }

    /// A sessao do WhatsApp carrega o numero inteiro no id. O relatorio
    /// entrega so os 4 ultimos digitos, diz de que canal e a sessao e, por
    /// ser canal remoto, retem o que e do operador mesmo sem portao restrito.
    #[tokio::test]
    async fn sessao_do_whatsapp_mascara_o_numero_e_retem_o_do_operador() {
        let st = state();
        let ctx = ctx_na_sessao(
            "whatsapp-linked-5511987654321@s.whatsapp.net",
            Some("/home/operador/projeto-secreto"),
        );
        let out = tool(&st)
            .execute(&ctx, serde_json::json!({}))
            .await
            .expect("executa");
        assert!(!out.content.contains("5511987654321"), "{}", out.content);
        assert!(!out.content.contains("projeto-secreto"), "{}", out.content);
        let json: serde_json::Value = serde_json::from_str(&out.content).expect("json");
        assert_eq!(
            json["session"]["id"],
            "whatsapp-linked-***4321@s.whatsapp.net"
        );
        assert_eq!(json["session"]["channel"], "whatsapp_linked");
        assert!(json["session"]["working_dir"].is_null());
        assert!(json["providers"].is_null());
        assert_eq!(
            json["withheld"],
            serde_json::json!(RETIDOS_NO_TURNO_RESTRITO),
            "{json}"
        );
    }

    /// O portao restrito do turno (o piso `search`) basta, mesmo numa sessao
    /// sem prefixo de canal. Fora dele, o mesmo contexto recebe tudo.
    #[tokio::test]
    async fn portao_restrito_retem_diretorio_provedores_e_versao_exata() {
        let st = state();
        let tool = tool(&st);
        let ctx = ctx(Some("/home/operador/projeto"));

        let (aberto, _) = relatorio_no_turno(&tool, &ctx, false).await;
        assert_eq!(aberto["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(aberto["session"]["working_dir"], "/home/operador/projeto");
        assert!(aberto["providers"].is_array(), "{aberto}");
        assert!(aberto["mcp_servers"].is_array(), "{aberto}");
        assert_eq!(aberto["withheld"], serde_json::json!([]));

        let (restrito, texto) = relatorio_no_turno(&tool, &ctx, true).await;
        assert_eq!(restrito["version"], versao_curta());
        assert_ne!(restrito["version"], env!("CARGO_PKG_VERSION"));
        assert!(restrito["session"]["working_dir"].is_null(), "{restrito}");
        assert!(restrito["session"]["project_id"].is_null(), "{restrito}");
        assert!(restrito["providers"].is_null(), "{restrito}");
        assert!(restrito["mcp_servers"].is_null(), "{restrito}");
        assert!(!texto.contains("/home/operador"), "{texto}");
        assert_eq!(
            restrito["withheld"],
            serde_json::json!(RETIDOS_NO_TURNO_RESTRITO)
        );
        // O que responde "voce tem acesso ao canal X?" continua la.
        assert!(restrito["channels"].is_array());
        assert!(restrito["tools"].is_array());
        assert_eq!(restrito["execution_profile"], "standard");
    }

    /// Varredura: uma config cheia de segredo plantado (token de bot, chave
    /// de LLM com URL, api key e bind do gateway, donos e allowlist do
    /// WhatsApp, raiz do pod, env de MCP) e uma sessao do WhatsApp. Nenhum
    /// valor plantado aparece no relatorio — nem no turno aberto (onde so o
    /// que e da sessao pode aparecer), nem no restrito.
    #[tokio::test]
    async fn varredura_de_segredos_plantados() {
        let dir = tempfile::tempdir().expect("tempdir");
        vincula(dir.path());
        let mut config = config_no(dir.path());
        config.gateway.host = "10.77.66.55".to_string();
        config.gateway.port = 45123;
        config.gateway.api_key = Some("gw-chave-plantada-0001".to_string());
        config.channels.insert(
            "telegram".to_string(),
            serde_json::from_value(serde_json::json!({
                "type": "telegram",
                "bot_token": "123456789:TELEGRAM-TOKEN-PLANTADO",
            }))
            .expect("canal"),
        );
        config.channels.insert(
            "whatsapp_linked".to_string(),
            serde_json::from_value(serde_json::json!({
                "type": "whatsapp_linked",
                "owners": ["+5511911112222"],
                "allow": ["+5511933334444"],
            }))
            .expect("canal"),
        );
        config.llm.insert(
            "openrouter".to_string(),
            garraia_config::LlmProviderConfig {
                provider: "openrouter".to_string(),
                model: None,
                api_key: Some("sk-or-chave-plantada-0002".to_string()),
                base_url: Some("https://usuario:senha@llm.interno.example/v1".to_string()),
                extra: Default::default(),
            },
        );
        config.mcp.insert(
            "segredo".to_string(),
            serde_json::from_value(serde_json::json!({
                "command": "/opt/mcp-plantado/bin",
                "env": { "MCP_TOKEN": "mcp-token-plantado-0003" },
            }))
            .expect("mcp"),
        );
        config.execution.profile = Some(garraia_config::execution::ExecutionProfile::IsolatedPod);
        config.execution.pod_root = Some(std::path::PathBuf::from("/srv/pod-raiz-plantada"));

        let st = state_com(config);
        st.whatsapp_linked.set_bridge(BridgeView::Connected);
        let tool = tool(&st);
        let sessao = ctx_na_sessao(
            "whatsapp-linked-5511987654321@s.whatsapp.net",
            Some("/home/operador/dir-plantado"),
        );
        let local = ctx(None);

        let plantados = [
            "10.77.66.55",
            "45123",
            "gw-chave-plantada-0001",
            "TELEGRAM-TOKEN-PLANTADO",
            "123456789",
            "5511911112222",
            "5511933334444",
            "5511987654321",
            "sk-or-chave-plantada-0002",
            "usuario:senha",
            "llm.interno.example",
            "mcp-plantado",
            "mcp-token-plantado-0003",
            "/srv/pod-raiz-plantada",
            "dir-plantado",
        ];
        let data_dir = dir.path().to_string_lossy().into_owned();
        for restrito in [false, true] {
            for c in [&sessao, &local] {
                let (json, texto) = relatorio_no_turno(&tool, c, restrito).await;
                for p in plantados {
                    assert!(
                        !texto.contains(p),
                        "{p} vazou (restrito={restrito}): {texto}"
                    );
                }
                assert!(!texto.contains(&data_dir), "data dir vazou: {texto}");
                // O que o relatorio existe para dizer continua dito.
                assert_eq!(json["execution_profile"], "isolated-pod");
                assert_eq!(
                    canal(&json, "whatsapp_linked").expect("canal")["status"],
                    "active"
                );
            }
        }
    }

    /// #1347: o `server.rs` registra `garra_status` DEPOIS de montar os
    /// canais push, e passa as contagens deles. Registrada antes (onde
    /// ficava), a tool so poderia receber zero canais push e diria
    /// `offline` para um webhook que esta respondendo.
    #[test]
    fn server_registra_a_tool_depois_dos_canais_push_e_com_as_contagens() {
        let fonte = include_str!("../server.rs");
        let push = fonte
            .find("let push_channels = crate::push_channels::PushChannelStates {")
            .expect("o server monta os canais push numa variavel");
        let registro = fonte
            .find("GarraStatusTool::new(")
            .expect("o server registra garra_status");
        assert_eq!(
            fonte.matches("GarraStatusTool::new(").count(),
            1,
            "um registro so"
        );
        assert!(registro > push, "registro antes dos canais push");
        let chamada = &fonte[registro..];
        let fim = chamada.find(";").expect("fim da chamada");
        assert!(
            chamada[..fim].contains("push_channels.contagens()"),
            "a tool recebe as contagens dos canais push: {}",
            &chamada[..fim]
        );
    }

    /// Estado derrubado nao vira panic: a tool devolve erro legivel.
    #[tokio::test]
    async fn estado_derrubado_vira_erro_legivel() {
        let st = state();
        let tool = tool(&st);
        drop(st);
        let out = tool
            .execute(&ctx(None), serde_json::json!({}))
            .await
            .expect("executa");
        assert!(out.is_error);
        assert!(out.content.contains("gateway state is gone"));
    }
}
