use super::super::{
    AgentRuntime, NOTA_GARRA_STATUS_EN, NOTA_GARRA_STATUS_PT, com_nota_de_capacidades,
};
use super::stub;
use crate::exec_context::ExecContext;
use crate::persona::Lang;
use crate::providers::{
    ContentBlock, LlmProvider, LlmRequest, LlmResponse, StreamEvent, ToolDefinition,
};
use futures::Stream;
use garraia_common::Result;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

pub(super) fn def(nome: &str) -> ToolDefinition {
    ToolDefinition {
        name: nome.to_string(),
        description: String::new(),
        input_schema: serde_json::json!({"type": "object"}),
    }
}

#[test]
pub(super) fn nota_so_entra_com_garra_status_oferecida() {
    let sem = com_nota_de_capacidades(Some("P".into()), &[def("file_read")], Lang::Pt);
    assert_eq!(sem.as_deref(), Some("P"));
    assert_eq!(com_nota_de_capacidades(None, &[], Lang::Pt), None);
}

#[test]
pub(super) fn nota_vai_depois_do_prompt_sem_substituir() {
    let com = com_nota_de_capacidades(
        Some("prompt do modo".into()),
        &[def("file_read"), def("garra_status")],
        Lang::Pt,
    )
    .expect("prompt");
    assert!(com.starts_with("prompt do modo\n\n"), "{com}");
    assert!(com.ends_with(NOTA_GARRA_STATUS_PT), "{com}");
}

#[test]
pub(super) fn nota_sem_prompt_e_a_lingua_da_persona() {
    assert_eq!(
        com_nota_de_capacidades(None, &[def("garra_status")], Lang::En).as_deref(),
        Some(NOTA_GARRA_STATUS_EN)
    );
    assert!(NOTA_GARRA_STATUS_PT.contains("`garra_status`"));
    assert!(NOTA_GARRA_STATUS_EN.contains("`garra_status`"));
}

/// Revisao da onda A: a nota descreve o relatorio que existe — a
/// lista `channels` — e o `status` com `active`/`offline` que a
/// fatia do gateway acrescenta, e nao fala de ferramentas: para elas
/// a lista do turno e a fonte, e o relatorio listava tools negadas.
#[test]
pub(super) fn nota_casa_com_o_formato_do_relatorio_e_nao_fala_de_ferramenta() {
    for nota in [NOTA_GARRA_STATUS_PT, NOTA_GARRA_STATUS_EN] {
        for campo in [
            "`channels`",
            "`status`",
            "`active`",
            "`offline`",
            "`session.channel`",
        ] {
            assert!(nota.contains(campo), "{campo} ausente: {nota}");
        }
        let minuscula = nota.to_lowercase();
        assert!(
            !minuscula.contains("ferramenta") && !minuscula.contains("tool"),
            "{nota}"
        );
    }
}

/// #1347 (C4): a nota nao afirma que TODO canal ausente esta
/// desligado. O web chat, a API, a CLI e o MCP nunca entram na
/// lista, e a frase antiga fazia o usuario do web chat ouvir que o
/// web chat nao estava disponivel. A afirmacao vale so para canal de
/// mensagens, e a nota nomeia as superficies que a lista nao cobre.
#[test]
pub(super) fn nota_so_afirma_ausencia_de_canal_de_mensagens() {
    for (nota, geral, restrita) in [
        (
            NOTA_GARRA_STATUS_PT,
            "Um canal ausente da lista",
            "um canal de mensagens ausente dela nao esta ligado",
        ),
        (
            NOTA_GARRA_STATUS_EN,
            "A channel missing from the list",
            "a messaging channel missing from it is not enabled",
        ),
    ] {
        assert!(!nota.contains(geral), "{nota}");
        assert!(nota.contains(restrita), "{nota}");
        for id in ["`web`", "`api`", "`cli`", "`mcp`"] {
            assert!(nota.contains(id), "{id}: {nota}");
        }
    }
}

/// Guarda o `system` e as `tools` da primeira requisicao; responde
/// em texto (batch e streaming).
#[derive(Default)]
pub(super) struct Captura {
    pub(super) primeira: Mutex<Option<(Option<String>, Vec<String>)>>,
    /// `(max_tokens, temperature)` da primeira requisicao.
    pub(super) parametros: Mutex<Option<(Option<u32>, Option<f64>)>>,
}

impl Captura {
    pub(super) fn anotar(&self, request: &LlmRequest) {
        let mut p = self.primeira.lock().expect("lock");
        if p.is_none() {
            *p = Some((
                request.system.clone(),
                request.tools.iter().map(|t| t.name.clone()).collect(),
            ));
            *self.parametros.lock().expect("lock") =
                Some((request.max_tokens, request.temperature));
        }
    }

    pub(super) fn parametros(&self) -> (Option<u32>, Option<f64>) {
        self.parametros
            .lock()
            .expect("lock")
            .expect("houve requisicao")
    }

    pub(super) fn primeira(&self) -> (Option<String>, Vec<String>) {
        self.primeira
            .lock()
            .expect("lock")
            .clone()
            .expect("houve requisicao")
    }
}

#[async_trait::async_trait]
impl LlmProvider for Captura {
    fn provider_id(&self) -> &str {
        "captura"
    }

    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        self.anotar(request);
        Ok(LlmResponse {
            content: vec![ContentBlock::Text {
                text: "nao sei".to_string(),
            }],
            model: "m".to_string(),
            stop_reason: None,
            usage: None,
        })
    }

    async fn stream_complete(
        &self,
        request: &LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        self.anotar(request);
        Ok(Box::pin(futures::stream::iter(vec![
            Ok(StreamEvent::TextDelta("nao sei".to_string())),
            Ok(StreamEvent::MessageStop),
        ])))
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Caminho {
    AgentConfig,
    Streaming,
    Heartbeat,
}

pub(super) async fn primeira_requisicao(
    caminho: Caminho,
    com_tool: bool,
    exec: &ExecContext,
) -> (Option<String>, Vec<String>) {
    rodar(caminho, com_tool, exec).await.primeira()
}

pub(super) async fn rodar(caminho: Caminho, com_tool: bool, exec: &ExecContext) -> Arc<Captura> {
    let rt = AgentRuntime::new();
    rt.register_tool(stub("file_read"));
    if com_tool {
        rt.register_tool(stub("garra_status"));
    }
    let provider = Arc::new(Captura::default());
    rt.register_provider(provider.clone());
    let texto = "voce tem acesso ao WhatsApp?";
    match caminho {
        Caminho::AgentConfig => {
            rt.process_message_with_agent_config(
                "s-1347",
                texto,
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                exec,
            )
            .await
            .expect("turno");
        }
        Caminho::Streaming => {
            let (tx, mut rx) = tokio::sync::mpsc::channel::<crate::turn_events::TurnEvent>(64);
            let dreno = tokio::spawn(async move { while rx.recv().await.is_some() {} });
            rt.process_message_streaming_with_events(
                "s-1347",
                texto,
                &[],
                tx,
                None,
                None,
                None,
                None,
                None,
                None,
                exec,
            )
            .await
            .expect("turno");
            dreno.await.expect("dreno");
        }
        Caminho::Heartbeat => {
            rt.process_heartbeat("s-1347", texto, &[], None, None)
                .await
                .expect("turno");
        }
    }
    provider
}

/// #1347 (N1): o changelog diz que o streaming e o batch
/// (`process_message_with_agent_config`) seguem a mesma precedencia.
/// Este teste prende a afirmacao no pedido inteiro que sai para o
/// provider, num modo customizado com `temperature` e `max_tokens`
/// proprios: o mesmo prompt, o mesmo `max_tokens` do modo e a mesma
/// `temperature` nos dois ramos. Hoje nenhum dos dois manda a
/// `temperature` do modo — so o `process_message_impl` manda, e ele so
/// roda pelo heartbeat, que nunca tem modo (o A2A vai pelo
/// `process_message_with_agent_config`) —, e o que o teste impede e um
/// ramo mudar sem o outro.
#[tokio::test]
pub(super) async fn streaming_e_batch_mandam_o_mesmo_pedido_no_modo() {
    let perfil = crate::modes::ModeProfile::from_custom(
        crate::modes::AgentMode::Search,
        "busca-fina",
        None,
        &serde_json::json!({}),
        &serde_json::json!({ "temperature": 0.3, "max_tokens": 8192 }),
    );
    let exec = ExecContext::with_custom_profile("busca-fina".to_string(), perfil);
    let batch = rodar(Caminho::AgentConfig, true, &exec).await;
    let streaming = rodar(Caminho::Streaming, true, &exec).await;
    assert_eq!(batch.primeira().0, streaming.primeira().0, "prompt");
    assert_eq!(batch.parametros(), streaming.parametros());
    assert_eq!(batch.parametros().0, Some(8192), "max_tokens do modo");
}

/// O cenario do relato: piso `search` (o do WhatsApp), pergunta sobre
/// acesso. A tool chega ao modelo e o prompt tem o template do modo
/// E a nota — nos tres ramos do runtime.
#[tokio::test]
pub(super) async fn modo_search_oferece_garra_status_e_a_nota() {
    let search = ExecContext::with_mode(Some("search".to_string()));
    for caminho in [Caminho::AgentConfig, Caminho::Streaming] {
        let (system, tools) = primeira_requisicao(caminho, true, &search).await;
        assert!(
            tools.iter().any(|t| t == "garra_status"),
            "{caminho:?}: {tools:?}"
        );
        let system = system.expect("prompt");
        // #1347 (fatia 3): o streaming tambem aplica o template do
        // modo — antes so o batch aplicava, e o mesmo turno recebia
        // prompts diferentes conforme o ramo.
        assert!(
            system.contains("You are a search assistant"),
            "{caminho:?}: {system}"
        );
        assert!(
            system.ends_with(NOTA_GARRA_STATUS_PT),
            "{caminho:?}: {system}"
        );
    }
    // O heartbeat nao escolhe modo: persona + nota.
    let (system, _) = primeira_requisicao(Caminho::Heartbeat, true, &ExecContext::default()).await;
    assert!(system.expect("prompt").contains(NOTA_GARRA_STATUS_PT));
}

/// Sem a tool registrada (a CLI), ou com um modo que a nega, nada
/// de nota: o modelo nunca e mandado chamar o que nao tem.
#[tokio::test]
pub(super) async fn sem_garra_status_oferecida_nao_ha_nota() {
    let search = ExecContext::with_mode(Some("search".to_string()));
    for caminho in [Caminho::AgentConfig, Caminho::Streaming] {
        let (system, tools) = primeira_requisicao(caminho, false, &search).await;
        assert!(!tools.iter().any(|t| t == "garra_status"));
        // Nem a nota nem a persona (#1347, fatia 3) citam a tool.
        let system = system.unwrap_or_default();
        assert!(!system.contains("garra_status"), "{caminho:?}: {system}");
    }
    // O heartbeat e o ramo que cai na persona (sem modo): o caso da
    // CLI, que nunca registra `garra_status`.
    let (system, _) = primeira_requisicao(Caminho::Heartbeat, false, &ExecContext::default()).await;
    let system = system.unwrap_or_default();
    assert!(system.contains("Garra"), "a persona entrou: {system}");
    assert!(!system.contains("garra_status"), "{system}");

    let perfil = crate::modes::ModeProfile::from_custom(
        crate::modes::AgentMode::Search,
        "sem-status",
        None,
        &serde_json::json!({ "deny": ["garra_status"] }),
        &serde_json::json!({}),
    );
    let nega = ExecContext::with_custom_profile("sem-status".to_string(), perfil);
    let (system, tools) = primeira_requisicao(Caminho::AgentConfig, true, &nega).await;
    assert!(!tools.iter().any(|t| t == "garra_status"), "{tools:?}");
    assert!(!system.unwrap_or_default().contains(NOTA_GARRA_STATUS_PT));
}

/// Uma tool de mentira que anota o que `ferramentas_do_turno` e
/// `turno_restrito` devolveram quando o RUNTIME a executou.
///
/// O nome e parametro desde a revisao da onda C (#1380): a sonda
/// precisa poder se passar por uma tool qualquer, e nao so por
/// `garra_status`, justamente para provar que o despacho publica o
/// bit do turno para todas.
pub(super) type Visto = Option<(Option<Vec<String>>, Option<bool>)>;

pub(super) struct SondaDeStatus {
    pub(super) viu: Arc<Mutex<Visto>>,
    pub(super) nome: &'static str,
}

#[async_trait::async_trait]
impl crate::tools::Tool for SondaDeStatus {
    fn name(&self) -> &str {
        self.nome
    }
    fn description(&self) -> &str {
        "sonda"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    async fn execute(
        &self,
        _c: &crate::tools::ToolContext,
        _i: serde_json::Value,
    ) -> Result<crate::tools::ToolOutput> {
        *self.viu.lock().expect("lock") = Some((
            crate::tools::turn_tools::ferramentas_do_turno(),
            crate::tools::turn_tools::turno_restrito(),
        ));
        Ok(crate::tools::ToolOutput::success("{}"))
    }
}

/// Pede a tool nomeada na primeira volta; responde em texto depois.
pub(super) struct PedeStatus(&'static str);

#[async_trait::async_trait]
impl LlmProvider for PedeStatus {
    fn provider_id(&self) -> &str {
        "pede_status"
    }

    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        let ja_rodou = request.messages.iter().any(|m| {
            matches!(&m.content, crate::providers::MessagePart::Parts(p)
                if p.iter().any(|b| matches!(b, ContentBlock::ToolResult { .. })))
        });
        let content = if ja_rodou {
            vec![ContentBlock::Text {
                text: "ok".to_string(),
            }]
        } else {
            vec![ContentBlock::ToolUse {
                id: "t-status".to_string(),
                name: self.0.to_string(),
                input: serde_json::json!({}),
            }]
        };
        Ok(LlmResponse {
            content,
            model: "m".to_string(),
            stop_reason: None,
            usage: None,
        })
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

/// Revisao da onda A: no piso `search`, `garra_status` recebe so as
/// ferramentas que o portao do turno libera — `bash` e `file_write`
/// estao registradas mas negadas, e nao podem aparecer.
#[tokio::test]
pub(super) async fn garra_status_recebe_so_as_ferramentas_liberadas_no_turno() {
    async fn visto_em(exec: &ExecContext) -> Visto {
        let rt = AgentRuntime::new();
        for nome in ["bash", "file_write", "file_read"] {
            rt.register_tool(stub(nome));
        }
        let viu = Arc::new(Mutex::new(None));
        rt.register_tool(Box::new(SondaDeStatus {
            viu: Arc::clone(&viu),
            nome: "garra_status",
        }));
        rt.register_provider(Arc::new(PedeStatus("garra_status")));
        let r = rt
            .process_message_with_agent_config(
                "s-1347-tools",
                "o que voce pode fazer?",
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                exec,
            )
            .await
            .expect("turno");
        assert_eq!(r, "ok");
        viu.lock().expect("lock").clone()
    }

    let search = ExecContext::with_mode(Some("search".to_string()));
    assert_eq!(
        visto_em(&search).await,
        Some((
            Some(vec!["file_read".to_string(), "garra_status".to_string()]),
            Some(true)
        )),
        "a tool so ve o que o portao do search libera, e sabe que o turno e restrito"
    );
    // #1347 (fatia 3): sem modo, o portao nao restringe — e a tool
    // recebe `false`, nao `None` (esta dentro de um turno).
    let (_, restrito) = visto_em(&ExecContext::default()).await.expect("rodou");
    assert_eq!(restrito, Some(false));
}

/// Regressao do B1 (#1380, revisao da onda C): o bit do turno chega a
/// uma tool que NAO e a `garra_status`.
///
/// O escopo de `ferramentas_do_turno` e aberto so em volta da
/// `garra_status`. Quando a recusa do `repo_search` passou a
/// consultar `turno_restrito()`, ela lia `None` em producao para
/// sempre — e o teste que existia montava o `task_local` a mao, entao
/// provava a funcao e nao o DESPACHO. Este roda a sonda com o nome
/// `repo_search` por um turno de verdade
/// (`process_message_with_agent_config`) e olha o que ela viu de
/// dentro da propria execucao.
#[tokio::test]
pub(super) async fn bit_do_turno_chega_a_toda_tool_e_nao_so_a_garra_status() {
    async fn restrito_visto_por_repo_search(exec: &ExecContext) -> Option<bool> {
        let rt = AgentRuntime::new();
        let viu = Arc::new(Mutex::new(None));
        rt.register_tool(Box::new(SondaDeStatus {
            viu: Arc::clone(&viu),
            nome: "repo_search",
        }));
        rt.register_provider(Arc::new(PedeStatus("repo_search")));
        let r = rt
            .process_message_with_agent_config(
                "s-1380-bit",
                "procure no repo",
                &[],
                None,
                None,
                None,
                None,
                None,
                None,
                exec,
            )
            .await
            .expect("turno");
        assert_eq!(r, "ok");
        let visto = viu.lock().expect("lock").clone();
        let (ferramentas, restrito) = visto.expect("a sonda tem de ter rodado");
        // A lista continua sendo privilegio da `garra_status`: o
        // custo de monta-la a cada tool nao se paga.
        assert_eq!(
            ferramentas, None,
            "a lista nao deve vazar para outras tools"
        );
        restrito
    }

    // O piso `search` (WhatsApp e afins) libera `repo_search`, e e
    // exatamente o turno em que o caminho do host nao pode sair.
    let search = ExecContext::with_mode(Some("search".to_string()));
    assert_eq!(
        restrito_visto_por_repo_search(&search).await,
        Some(true),
        "sem isto a recusa do repo_search nunca sabe que o turno e restrito"
    );
    // E o turno aberto e distinguivel de "fora de turno" (`None`): e
    // essa diferenca que devolve o caminho ao operador local.
    assert_eq!(
        restrito_visto_por_repo_search(&ExecContext::default()).await,
        Some(false),
        "o turno aberto tem de ser Some(false), nao None"
    );
}
