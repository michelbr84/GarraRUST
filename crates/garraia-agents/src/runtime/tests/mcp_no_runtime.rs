use super::*;

pub(super) struct StubTool(&'static str);

#[async_trait]
impl Tool for StubTool {
    fn name(&self) -> &str {
        self.0
    }
    fn description(&self) -> &str {
        "stub"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    async fn execute(
        &self,
        _c: &ToolContext,
        _i: serde_json::Value,
    ) -> garraia_common::Result<ToolOutput> {
        Ok(ToolOutput::success("ok"))
    }
}

pub(super) fn stub(name: &'static str) -> Box<dyn Tool> {
    Box::new(StubTool(name))
}

/// O cenario exato do relato: o boot registra so as nativas porque o
/// connect do servidor MCP perdeu a corrida, o servidor conecta depois, e
/// o runtime tem de acabar com as duas metades — nao com seis tools e um
/// servidor reportando catorze.
#[test]
pub(super) fn late_connecting_server_still_lands_in_the_runtime() {
    let rt = AgentRuntime::new();
    rt.register_tool(stub("bash"));
    rt.register_tool(stub("file_read"));
    assert_eq!(rt.tool_names().len(), 2);

    // O health monitor reconecta e sincroniza.
    let delta = rt.replace_mcp_tools(
        "filesystem",
        vec![stub("filesystem__read_file"), stub("filesystem__list_dir")],
    );
    assert_eq!(delta.removed, 0);
    assert_eq!(delta.added, 2);

    assert_eq!(rt.tool_names().len(), 4);
    // E, o que importa de verdade: o LLM as ve.
    let defs: Vec<String> = rt.tool_definitions().into_iter().map(|d| d.name).collect();
    assert!(defs.contains(&"filesystem__read_file".to_string()));
    assert!(rt.find_tool("filesystem__list_dir").is_some());
}

/// Idempotencia: rodar a cada 30s nao pode acumular duplicatas. Como
/// `find_tool` e uma varredura linear, duplicatas se sombreariam em
/// silencio em vez de dar erro.
#[test]
pub(super) fn repeated_sync_does_not_duplicate() {
    let rt = AgentRuntime::new();
    rt.register_tool(stub("bash"));

    for _ in 0..5 {
        rt.replace_mcp_tools("fs", vec![stub("fs__a"), stub("fs__b")]);
    }

    assert_eq!(rt.tool_names().len(), 3);
    assert_eq!(rt.tool_names().iter().filter(|n| *n == "fs__a").count(), 1);
}

/// Um servidor que volta com inventario menor tem de encolher, e nunca
/// levar junto as tools nativas nem as de outro servidor.
#[test]
pub(super) fn sync_is_scoped_to_one_server_and_can_shrink() {
    let rt = AgentRuntime::new();
    rt.register_tool(stub("bash"));
    rt.replace_mcp_tools("fs", vec![stub("fs__a"), stub("fs__b"), stub("fs__c")]);
    rt.replace_mcp_tools("git", vec![stub("git__log")]);
    assert_eq!(rt.tool_names().len(), 5);

    let delta = rt.replace_mcp_tools("fs", vec![stub("fs__a")]);
    assert_eq!(delta.removed, 3);
    assert_eq!(delta.added, 1);

    let names = rt.tool_names();
    assert_eq!(names.len(), 3);
    assert!(names.contains(&"bash".to_string()), "nativa preservada");
    assert!(
        names.contains(&"git__log".to_string()),
        "outro servidor intacto"
    );
    assert!(
        !names.contains(&"fs__b".to_string()),
        "tool sumida foi removida"
    );
}

/// Um servidor que desaparece por completo esvazia so a propria fatia.
#[test]
pub(super) fn empty_inventory_clears_only_that_server() {
    let rt = AgentRuntime::new();
    rt.register_tool(stub("bash"));
    rt.replace_mcp_tools("fs", vec![stub("fs__a")]);

    let delta = rt.replace_mcp_tools("fs", Vec::new());
    assert_eq!(delta.removed, 1);
    assert_eq!(delta.added, 0);
    assert_eq!(rt.tool_names(), vec!["bash".to_string()]);
}

/// O inventario distingue origem — e o que torna as duas contagens da API
/// conferiveis em vez de misteriosas.
#[test]
pub(super) fn inventory_reports_source_and_server() {
    let rt = AgentRuntime::new();
    rt.register_tool(stub("bash"));
    rt.replace_mcp_tools("filesystem", vec![stub("filesystem__read_file")]);

    let inv = rt.tool_inventory();
    let native = inv.iter().find(|t| t.name == "bash").unwrap();
    assert_eq!(native.source, "native");
    assert!(native.server.is_none());

    let mcp = inv
        .iter()
        .find(|t| t.name == "filesystem__read_file")
        .unwrap();
    assert_eq!(mcp.source, "mcp");
    assert_eq!(mcp.server.as_deref(), Some("filesystem"));
}

// ── #1264: o whitelist do modo cobre MCP no caminho do runtime ──────────

/// Provider que anota o que recebeu e pede a ferramenta MCP pelo nome.
///
/// As duas metades do controle sao medidas por ele: `tools` de cada
/// `LlmRequest` diz o que o modelo **viu**, e os `ToolResult` que voltam na
/// volta seguinte dizem o que o guard de pre-execucao respondeu quando ele
/// pediu de todo jeito.
pub(super) struct PedeFerramentaMcp {
    pub(super) alvo: &'static str,
    pub(super) vistas: std::sync::Mutex<Vec<Vec<String>>>,
    pub(super) resultados: std::sync::Mutex<Vec<String>>,
    pub(super) voltas: std::sync::atomic::AtomicUsize,
}

impl PedeFerramentaMcp {
    pub(super) fn novo(alvo: &'static str) -> Self {
        Self {
            alvo,
            vistas: std::sync::Mutex::new(Vec::new()),
            resultados: std::sync::Mutex::new(Vec::new()),
            voltas: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub(super) fn ferramentas_da_primeira_volta(&self) -> Vec<String> {
        self.vistas
            .lock()
            .expect("lock")
            .first()
            .cloned()
            .unwrap_or_default()
    }

    pub(super) fn resultados(&self) -> Vec<String> {
        self.resultados.lock().expect("lock").clone()
    }
}

#[async_trait::async_trait]
impl LlmProvider for PedeFerramentaMcp {
    fn provider_id(&self) -> &str {
        "pede_ferramenta_mcp"
    }

    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        self.vistas
            .lock()
            .expect("lock")
            .push(request.tools.iter().map(|t| t.name.clone()).collect());
        for m in &request.messages {
            if let MessagePart::Parts(blocos) = &m.content {
                for b in blocos {
                    if let ContentBlock::ToolResult { content, .. } = b {
                        self.resultados.lock().expect("lock").push(content.clone());
                    }
                }
            }
        }

        let volta = self
            .voltas
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let content = if volta == 0 {
            vec![ContentBlock::ToolUse {
                id: "chamada-1".to_string(),
                name: self.alvo.to_string(),
                input: serde_json::json!({}),
            }]
        } else {
            vec![ContentBlock::Text {
                text: "segui sem ela".to_string(),
            }]
        };
        Ok(LlmResponse {
            content,
            model: "modelo-de-teste".to_string(),
            stop_reason: None,
            usage: None,
        })
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

/// Ferramenta que registra se chegou a rodar.
pub(super) struct ToolQueMarca {
    pub(super) nome: &'static str,
    pub(super) executou: Arc<std::sync::atomic::AtomicBool>,
}

#[async_trait]
impl Tool for ToolQueMarca {
    fn name(&self) -> &str {
        self.nome
    }
    fn description(&self) -> &str {
        "ferramenta de servidor MCP, para o teste do portao"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    async fn execute(
        &self,
        _c: &ToolContext,
        _i: serde_json::Value,
    ) -> garraia_common::Result<ToolOutput> {
        self.executou
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(ToolOutput::success("rodei"))
    }
}

/// **#1264 (P1) no caminho de producao.** O turno que o runtime monta tem de
/// barrar ferramenta de servidor MCP que o modo whitelist nao declarou.
///
/// Nada aqui e montado a mao: o `ExecContext` com `agent_mode` e o que os
/// canais passam, `process_message_with_agent_config` e a entrada que o
/// gateway e a CLI chamam, e o portao sai de
/// `ToolGate::para_o_turno` **dentro** do runtime. As duas camadas do #988
/// sao medidas: o filtro da lista (o modelo nao ve) e o guard de
/// pre-execucao (pedir pelo nome nao executa).
///
/// Antes da #1264 este turno executava `meu-servidor__escreve` no modo
/// `search` — um modo que se anuncia somente-leitura.
#[tokio::test]
pub(super) async fn o_turno_do_runtime_barra_ferramenta_mcp_nao_declarada() {
    let rt = AgentRuntime::new();
    let provider = Arc::new(PedeFerramentaMcp::novo("meu-servidor__escreve"));
    rt.register_provider(provider.clone());

    let executou = Arc::new(std::sync::atomic::AtomicBool::new(false));
    // Registrada como o boot registra: `replace_mcp_tools` e o que o
    // `McpManager` chama ao sincronizar o servidor.
    rt.replace_mcp_tools(
        "meu-servidor",
        vec![Box::new(ToolQueMarca {
            nome: "meu-servidor__escreve",
            executou: Arc::clone(&executou),
        })],
    );
    rt.register_tool(stub("file_read"));

    let exec = ExecContext::with_mode(Some("search".to_string()));
    let resposta = rt
        .process_message_with_agent_config(
            "sessao-1264",
            "procura o handler de login",
            &[],
            None,
            None,
            None,
            None,
            None,
            None,
            &exec,
        )
        .await
        .expect("o turno tem de terminar, e nao explodir");

    let vistas = provider.ferramentas_da_primeira_volta();
    assert!(
        vistas.contains(&"file_read".to_string()),
        "premissa: o que o modo declara continua na lista; veio {vistas:?}"
    );
    assert!(
        !vistas.contains(&"meu-servidor__escreve".to_string()),
        "o modelo nao pode ver ferramenta MCP que o modo nao declarou; \
         veio {vistas:?}"
    );
    assert!(
        !executou.load(std::sync::atomic::Ordering::SeqCst),
        "a ferramenta MCP RODOU no modo `search` — o guard de \
         pre-execucao nao barrou o nome que o modelo pediu (#1264)"
    );
    let resultados = provider.resultados();
    assert!(
        resultados
            .iter()
            .any(|r| r.contains("nao e permitida no modo `search`")),
        "a recusa tem de voltar ao modelo como resultado de ferramenta; \
         veio {resultados:?}"
    );
    assert_eq!(resposta, "segui sem ela");
}

/// E o outro lado: **declarada**, a mesma ferramenta MCP roda.
///
/// Sem este teste o fix poderia ser "esconder toda ferramenta MCP", que
/// era justamente o medo que sustentava a escapatoria. O perfil vem de
/// `ModeProfile::from_custom` — o caminho do modo customizado do operador —
/// com a sintaxe declarada `meu-servidor/*`.
#[tokio::test]
pub(super) async fn servidor_declarado_com_prefixo_roda_no_turno_do_runtime() {
    let rt = AgentRuntime::new();
    let provider = Arc::new(PedeFerramentaMcp::novo("meu-servidor__escreve"));
    rt.register_provider(provider.clone());

    let executou = Arc::new(std::sync::atomic::AtomicBool::new(false));
    rt.replace_mcp_tools(
        "meu-servidor",
        vec![Box::new(ToolQueMarca {
            nome: "meu-servidor__escreve",
            executou: Arc::clone(&executou),
        })],
    );

    let perfil = crate::modes::ModeProfile::from_custom(
        crate::modes::AgentMode::Search,
        "busca-com-meu-servidor",
        None,
        &serde_json::json!({ "allow": ["file_read", "meu-servidor/*"] }),
        &serde_json::json!({}),
    );
    let exec = ExecContext {
        custom_profile: Some(perfil),
        ..Default::default()
    };
    let resposta = rt
        .process_message_with_agent_config(
            "sessao-1264-b",
            "usa o servidor",
            &[],
            None,
            None,
            None,
            None,
            None,
            None,
            &exec,
        )
        .await
        .expect("turno");

    let vistas = provider.ferramentas_da_primeira_volta();
    assert!(
        vistas.contains(&"meu-servidor__escreve".to_string()),
        "declarar `meu-servidor/*` tem de devolver a ferramenta ao modelo; \
         veio {vistas:?}"
    );
    assert!(
        executou.load(std::sync::atomic::Ordering::SeqCst),
        "ferramenta MCP declarada tem de rodar — o fix nao pode ser \
         'esconder MCP'"
    );
    assert_eq!(resposta, "segui sem ela");
}

/// **#1264 criterio 3, a assercao sobre o AVISO.** O que o runtime emite
/// para a MCP escondida e a linha montada por
/// [`mensagens_mcp_fora_da_whitelist`], e o teste afirma o CONTEUDO dela —
/// modo, nome da ferramenta e a sintaxe que libera — porque asserir sobre
/// `tracing` capturado seria infra que a arvore nao tem.
///
/// **Mutacao que este teste pega**: comente o `avisar_mcp_fora_da_whitelist`
/// de qualquer um dos tres caminhos de turno (ou esvazie o builder) e ele
/// fica vermelho — o criterio 5 da issue, no aviso em vez do retorno.
#[test]
pub(super) fn o_aviso_de_mcp_escondida_nomeia_ferramenta_e_sintaxe() {
    let g = crate::modes::ToolGate::for_mode_name("search");
    let todas = vec![crate::ToolDefinition {
        name: "file_read".to_string(),
        description: "leitura declarada".to_string(),
        input_schema: serde_json::json!({"type": "object"}),
    }];

    // Declarada: nada a avisar.
    assert!(mensagens_mcp_fora_da_whitelist(&g, &todas).is_empty());

    // Nao declarada: o aviso sai, com o nome e o como-liberar.
    let todas = vec![crate::ToolDefinition {
        name: "meu-servidor__escreve".to_string(),
        description: "escreve do terceiro".to_string(),
        input_schema: serde_json::json!({"type": "object"}),
    }];
    let mensagens = mensagens_mcp_fora_da_whitelist(&g, &todas);
    assert_eq!(mensagens.len(), 1, "uma linha por ferramenta escondida");
    let m = &mensagens[0];
    assert!(m.contains("search"), "{m}");
    assert!(m.contains("meu-servidor__escreve"), "{m}");
    assert!(m.contains("`servidor/*`"), "{m}");

    // Portao sem politica: nao restringe, nao avisa.
    assert!(
        mensagens_mcp_fora_da_whitelist(&crate::modes::ToolGate::sem_politica(), &todas).is_empty()
    );
}

/// **#1264 criterio 3, o aviso do caso vazio.** Perfil com whitelist
/// ligada e vazia emite o aviso cujo texto pede povoar ou desligar — e
/// nao emite nada quando a lista esta populada ou quando nao ha politica.
///
/// **Mutacao que este teste pega**: remova `avisar_whitelist_vazia` dos
/// caminhos de turno (ou o `Some` do builder) e ele fica vermelho.
#[test]
pub(super) fn o_aviso_da_whitelist_vazia_pedir_povoar_ou_desligar() {
    let perfil = crate::modes::ModeProfile::from_custom(
        crate::modes::AgentMode::Search,
        "so-leitura-vazia",
        None,
        &serde_json::json!({ "allow": [], "deny": [] }),
        &serde_json::json!({}),
    );
    let exec = ExecContext {
        custom_profile: Some(perfil),
        ..Default::default()
    };
    let portao = crate::modes::ToolGate::para_o_turno(&exec, "olha os arquivos");

    let mensagem = mensagem_whitelist_vazia(&portao).expect("o aviso sai");
    assert!(mensagem.contains("so-leitura-vazia"), "{mensagem}");
    assert!(
        mensagem.contains("Popule `allowed` ou desligue `whitelist_mode`"),
        "{mensagem}"
    );

    // E os casos sem aviso: lista populada, e sem politica nenhum.
    let g = crate::modes::ToolGate::for_mode_name("search");
    assert!(mensagem_whitelist_vazia(&g).is_none());
    assert!(mensagem_whitelist_vazia(&crate::modes::ToolGate::sem_politica()).is_none());
}

/// `register_tool` toma `&self`: o runtime ja esta dentro de um `Arc`
/// quando as tools de schedule sao registradas, e antes disso o
/// `Arc::get_mut` pulava o registro em silencio se o rc fosse > 1.
#[test]
pub(super) fn registration_works_through_a_shared_arc() {
    let rt = Arc::new(AgentRuntime::new());
    let clone = Arc::clone(&rt);
    assert_eq!(Arc::strong_count(&rt), 2);

    clone.register_tool(stub("schedule_heartbeat"));
    assert!(rt.find_tool("schedule_heartbeat").is_some());
}
