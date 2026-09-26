use super::*;

/// O portao **recusa**, e nao apenas deixa de oferecer (#988).
///
/// O criterio de aceite e "nenhuma ferramenta proibida e executada, mesmo
/// que solicitada pelo LLM". Um teste que so verificasse a lista de
/// definicoes passaria com o guard removido — o modelo pode pedir um nome
/// que nunca esteve na lista.
#[test]
pub(super) fn o_portao_recusa_ferramenta_proibida_pelo_modo() {
    use crate::exec_context::ExecContext;
    use crate::modes::ToolGate;

    let exec = ExecContext::with_mode(Some("search".to_string()));
    let portao = ToolGate::from_exec(&exec);

    // `search` e somente-leitura.
    assert!(!portao.permite("file_write"), "search deixou escrever");
    assert!(!portao.permite("bash"), "search deixou rodar bash");
    assert!(portao.permite("file_read"));
}

/// Sem modo escolhido, nada muda — e o caminho de todo canal que nunca
/// setou modo, o CLI incluso.
#[test]
pub(super) fn sem_modo_o_portao_nao_muda_nada() {
    use crate::exec_context::ExecContext;
    use crate::modes::ToolGate;

    let portao = ToolGate::from_exec(&ExecContext::default());
    assert!(portao.permite("file_write"));
    assert!(portao.permite("bash"));
}

/// #1226 S-A: um unico ponto de despacho de tool no `AgentRuntime`.
///
/// O gate do modo tinha quatro copias do mesmo `if` — uma por copia do
/// loop de turno — e qualquer caminho novo de despacho reabria o risco de
/// bypass que motivou o #988. O criterio de aceite da issue e mecanico: a
/// chamada do gate sobre o `name` do bloco `ToolUse` aparece **exatamente
/// uma vez**, dentro da `dispatch_tool_call`. Este teste fixa esse numero
/// na varredura do propio fonte; uma copia nova que esqueca o despacho
/// unico reprova aqui.
///
/// O literal do alvo e montado com `concat!` para a varredura nao casar
/// com o propio teste, que mora no mesmo arquivo.
#[test]
pub(super) fn despacho_de_tool_tem_um_unico_ponto_de_gate() {
    let src = include_str!("../../runtime.rs");
    let alvo = concat!("portao.permite", "(name)");
    let copias = src.matches(alvo).count();
    assert_eq!(copias, 1, "esperava 1 ponto de despacho, achei {copias}");
}

// ── #1226 S-B: tool_program intrinseca, gate por passo ──────────────────

/// Eco de inteiro: devolve o campo `n` tal qual, para testar
/// encadeamento de `$var` (so valor inteiro sobrevive a substituicao).
pub(super) struct EcoInteiroTool;

#[async_trait]
impl Tool for EcoInteiroTool {
    fn name(&self) -> &str {
        "eco_inteiro"
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
        i: serde_json::Value,
    ) -> garraia_common::Result<ToolOutput> {
        let texto = match i.get("n") {
            Some(v) => v.to_string(),
            None => i.to_string(),
        };
        Ok(ToolOutput::success(texto))
    }
}

/// Pede confirmacao humana (GAR-187) do jeito que as tools reais pedem:
/// com o marcador de impressao digital de `("precisa_confirmar", alvo)`
/// (`alvo` e o campo `alvo` do input, texto ou numero). Com uma
/// aprovacao no contexto que cubra esse par, roda de verdade — e anota
/// o `alvo` em `rodou`, para o teste saber O QUE rodou.
pub(super) struct ToolQuePedeConfirmacao {
    pub(super) rodou: Arc<std::sync::Mutex<Vec<String>>>,
}

impl ToolQuePedeConfirmacao {
    pub(super) fn nova() -> (Self, Arc<std::sync::Mutex<Vec<String>>>) {
        let rodou = Arc::new(std::sync::Mutex::new(Vec::new()));
        (
            Self {
                rodou: Arc::clone(&rodou),
            },
            rodou,
        )
    }

    pub(super) fn alvo(i: &serde_json::Value) -> String {
        match i.get("alvo") {
            Some(serde_json::Value::String(s)) => s.clone(),
            Some(outro) => outro.to_string(),
            None => String::new(),
        }
    }
}

#[async_trait]
impl Tool for ToolQuePedeConfirmacao {
    fn name(&self) -> &str {
        "precisa_confirmar"
    }
    fn description(&self) -> &str {
        "stub"
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    async fn execute(
        &self,
        c: &ToolContext,
        i: serde_json::Value,
    ) -> garraia_common::Result<ToolOutput> {
        let alvo = Self::alvo(&i);
        if c.approval.covers("precisa_confirmar", &alvo) {
            self.rodou.lock().expect("lock").push(alvo.clone());
            return Ok(ToolOutput::success(format!("feito: {alvo}")));
        }
        let marcador = ApprovalFingerprint::of("precisa_confirmar", &alvo).marker();
        Ok(ToolOutput::confirmation_request(format!(
            "confirme a acao perigosa em {alvo} {marcador}"
        )))
    }
}

/// Conta quantas vezes rodou, para os testes de repeticao.
pub(super) struct ToolQueConta {
    pub(super) vezes: Arc<std::sync::atomic::AtomicUsize>,
}

#[async_trait]
impl Tool for ToolQueConta {
    fn name(&self) -> &str {
        "conta"
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
        self.vezes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(ToolOutput::success("contei"))
    }
}

/// Como `ToolQueMarca`, mas com nome dinamico — para a tabela que tira
/// os nomes do `denied` de cada perfil nativo.
pub(super) struct SondaComNome {
    pub(super) nome: String,
    pub(super) executou: Arc<std::sync::atomic::AtomicBool>,
}

#[async_trait]
impl Tool for SondaComNome {
    fn name(&self) -> &str {
        &self.nome
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
        self.executou
            .store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(ToolOutput::success("rodei"))
    }
}

/// Provider preso: TODA volta pede o mesmo `tool_program` (nunca
/// encerra com texto). Conta as voltas.
pub(super) struct RepetePrograma {
    pub(super) programa: serde_json::Value,
    pub(super) voltas: std::sync::atomic::AtomicUsize,
}

#[async_trait::async_trait]
impl LlmProvider for RepetePrograma {
    fn provider_id(&self) -> &str {
        "repete_programa"
    }

    async fn complete(&self, _request: &LlmRequest) -> Result<LlmResponse> {
        let volta = self
            .voltas
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(LlmResponse {
            content: vec![ContentBlock::ToolUse {
                id: format!("programa-{volta}"),
                name: TOOL_PROGRAM_NAME.to_string(),
                input: self.programa.clone(),
            }],
            model: "modelo-de-teste".to_string(),
            stop_reason: None,
            usage: None,
        })
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

/// Roda um turno de streaming ate o fim e devolve, alem do resultado,
/// TODOS os eventos que o sink recebeu, na ordem.
pub(super) async fn turno_de_streaming_com_eventos(
    runtime: &AgentRuntime,
    sessao: &str,
    exec: &ExecContext,
) -> (Result<String>, Vec<crate::turn_events::TurnEvent>) {
    let (tx, mut rx) = mpsc::channel::<crate::turn_events::TurnEvent>(64);
    let coletor = tokio::spawn(async move {
        let mut eventos = Vec::new();
        while let Some(e) = rx.recv().await {
            eventos.push(e);
        }
        eventos
    });
    let resultado = runtime
        .process_message_streaming_with_events(
            sessao,
            "roda",
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
        .await;
    let eventos = coletor.await.expect("coletor");
    (resultado, eventos)
}

/// Todo `ToolStarted` tem o seu `ToolFinished`, casados como uma pilha
/// (o do programa envolve os dos passos). Devolve os nomes na ordem de
/// inicio, para o teste conferir quais passos apareceram.
pub(super) fn inicios_casados_com_fins(eventos: &[crate::turn_events::TurnEvent]) -> Vec<String> {
    use crate::turn_events::TurnEvent;
    let mut abertos: Vec<String> = Vec::new();
    let mut iniciados = Vec::new();
    for e in eventos {
        match e {
            TurnEvent::ToolStarted { name, .. } => {
                abertos.push(name.clone());
                iniciados.push(name.clone());
            }
            TurnEvent::ToolFinished { name, .. } => {
                let aberto = abertos.pop();
                assert_eq!(
                    aberto.as_deref(),
                    Some(name.as_str()),
                    "ToolFinished de `{name}` sem o ToolStarted casado; eventos: {eventos:?}"
                );
            }
            TurnEvent::TextDelta(_) => {}
        }
    }
    assert!(
        abertos.is_empty(),
        "ToolStarted sem ToolFinished: {abertos:?}; eventos: {eventos:?}"
    );
    iniciados
}

/// Dorme o tanto pedido em `segundos`, para testar o teto agregado sem
/// depender de relogio real (`#[tokio::test(start_paused = true)]`).
pub(super) struct FerramentaLenta;

#[async_trait]
impl Tool for FerramentaLenta {
    fn name(&self) -> &str {
        "lenta"
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
        i: serde_json::Value,
    ) -> garraia_common::Result<ToolOutput> {
        let segundos = i.get("segundos").and_then(|v| v.as_u64()).unwrap_or(1);
        tokio::time::sleep(std::time::Duration::from_secs(segundos)).await;
        Ok(ToolOutput::success("feito"))
    }
}

/// Provider que, na primeira volta, pede `tool_program` com o programa
/// dado; na segunda, encerra com texto. Registra cada `ToolResult` que
/// recebeu de volta, para inspecionar o que o `tool_program` devolveu.
pub(super) struct RodaPrograma {
    pub(super) programa: serde_json::Value,
    pub(super) resultados: std::sync::Mutex<Vec<String>>,
    pub(super) voltas: std::sync::atomic::AtomicUsize,
}

impl RodaPrograma {
    pub(super) fn novo(programa: serde_json::Value) -> Self {
        Self {
            programa,
            resultados: std::sync::Mutex::new(Vec::new()),
            voltas: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub(super) fn resultados(&self) -> Vec<String> {
        self.resultados.lock().expect("lock").clone()
    }
}

#[async_trait::async_trait]
impl LlmProvider for RodaPrograma {
    fn provider_id(&self) -> &str {
        "roda_programa"
    }

    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
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
                id: "programa-1".to_string(),
                name: TOOL_PROGRAM_NAME.to_string(),
                input: self.programa.clone(),
            }]
        } else {
            vec![ContentBlock::Text {
                text: "concluido".to_string(),
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
