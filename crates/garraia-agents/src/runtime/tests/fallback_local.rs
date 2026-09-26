use super::*;

// ─── #1249: rede caida cai para o provider local ──────────────────────

/// Como o primario falha, e a diferenca que a issue #1249 cobra.
#[derive(Clone, Copy)]
pub(super) enum FalhaSimulada {
    /// Rede caida: o cabo saiu, o DNS morreu, a porta nao responde. O
    /// provider classifica no ponto de captura (`erro_de_envio`), entao
    /// aqui chega ja como `Error::Transport`.
    Transporte,
    /// O provider **respondeu** dizendo "agora nao". Fica no comportamento
    /// historico: retry com backoff no mesmo endereco antes do fallback.
    Sobrecarga,
}

impl FalhaSimulada {
    pub(super) fn erro(self) -> Error {
        match self {
            // Texto identico ao que o `reqwest` produz, de proposito: se a
            // classificacao voltar a depender de casar string, este texto
            // nao casa com nenhum padrao de `is_retryable_error` e os
            // testes ficam vermelhos.
            Self::Transporte => Error::Transport(
                "openai request failed: error sending request for url \
                 (https://api.openai.com/v1/chat/completions)"
                    .to_string(),
            ),
            Self::Sobrecarga => {
                Error::Agent("openai API error: status=429, body=rate limit".to_string())
            }
        }
    }
}

/// O provider de nuvem que o wizard grava como primario.
pub(super) struct PrimarioQueFalha {
    pub(super) falha: FalhaSimulada,
    pub(super) chamadas_complete: std::sync::atomic::AtomicUsize,
    pub(super) chamadas_stream: std::sync::atomic::AtomicUsize,
}

impl PrimarioQueFalha {
    pub(super) fn nova(falha: FalhaSimulada) -> Self {
        Self {
            falha,
            chamadas_complete: std::sync::atomic::AtomicUsize::new(0),
            chamadas_stream: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub(super) fn completes(&self) -> usize {
        self.chamadas_complete
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    pub(super) fn streams(&self) -> usize {
        self.chamadas_stream
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl LlmProvider for PrimarioQueFalha {
    fn provider_id(&self) -> &str {
        "openai"
    }

    async fn complete(&self, _request: &LlmRequest) -> Result<LlmResponse> {
        self.chamadas_complete
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(self.falha.erro())
    }

    async fn stream_complete(
        &self,
        _request: &LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        self.chamadas_stream
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(self.falha.erro())
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(false)
    }
}

/// O Ollama que ja esta rodando na maquina do usuario.
pub(super) struct LocalQueResponde;

pub(super) const RESPOSTA_LOCAL: &str = "respondi do modelo local";

#[async_trait::async_trait]
impl LlmProvider for LocalQueResponde {
    fn provider_id(&self) -> &str {
        "ollama"
    }

    async fn complete(&self, _request: &LlmRequest) -> Result<LlmResponse> {
        Ok(LlmResponse {
            content: vec![ContentBlock::Text {
                text: RESPOSTA_LOCAL.to_string(),
            }],
            model: "qwen3.8:latest".to_string(),
            stop_reason: None,
            usage: None,
        })
    }

    async fn stream_complete(
        &self,
        _request: &LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        Ok(Box::pin(futures::stream::iter(vec![
            Ok(StreamEvent::TextDelta(RESPOSTA_LOCAL.to_string())),
            Ok(StreamEvent::MessageStop),
        ])))
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

/// A config que o wizard grava: nuvem como primario, Ollama no fallback.
pub(super) fn runtime_com_fallback_local(
    falha: FalhaSimulada,
) -> (AgentRuntime, std::sync::Arc<PrimarioQueFalha>) {
    let runtime = AgentRuntime::new();
    let primario = std::sync::Arc::new(PrimarioQueFalha::nova(falha));
    // Primeiro registrado vira o default, ou seja, o primario do turno.
    runtime.register_provider(primario.clone());
    runtime.register_provider(std::sync::Arc::new(LocalQueResponde));
    runtime.set_fallback_providers(vec!["ollama".to_string()]);
    (runtime, primario)
}

pub(super) fn pedido_simples() -> LlmRequest {
    LlmRequest {
        model: "gpt-4o".to_string(),
        messages: vec![ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text("oi".to_string()),
        }],
        system: None,
        max_tokens: Some(64),
        temperature: None,
        tools: Vec::new(),
    }
}

/// #1249, caminho batch: rede caida no primario termina no Ollama.
///
/// Sem o fix, `Error::Transport` cairia no `Err(e) => return Err(e)` do
/// laco de retry — antes do laco de fallback — e o turno devolveria erro
/// com o modelo local rodando ao lado.
#[tokio::test]
pub(super) async fn transporte_no_primario_cai_para_o_fallback_em_batch() {
    let (runtime, primario) = runtime_com_fallback_local(FalhaSimulada::Transporte);
    let como_dyn: Arc<dyn LlmProvider> = primario.clone();

    let (resposta, quem) = runtime
        .complete_reportando_provider(&como_dyn, &pedido_simples())
        .await
        .expect("o fallback local tem de completar o turno");

    assert_eq!(quem, "ollama", "quem respondeu foi o local");
    assert_eq!(extract_text(&resposta.content), RESPOSTA_LOCAL);
    assert_eq!(
        primario.completes(),
        1,
        "transporte gasta UMA tentativa: insistir num endereco \
         inalcancavel so atrasa o fallback"
    );
}

/// #1249, caminho streaming: o mesmo, no braco que ficou de fora antes.
#[tokio::test]
pub(super) async fn transporte_no_primario_cai_para_o_fallback_em_streaming() {
    let (runtime, primario) = runtime_com_fallback_local(FalhaSimulada::Transporte);
    let como_dyn: Arc<dyn LlmProvider> = primario.clone();

    let mut stream = runtime
        .stream_complete_with_fallback(&como_dyn, &pedido_simples())
        .await
        .expect("o stream do fallback local tem de chegar");

    let mut texto = String::new();
    while let Some(evento) = stream.next().await {
        if let StreamEvent::TextDelta(t) = evento.expect("o stream local nao falha") {
            texto.push_str(&t);
        }
    }

    assert_eq!(texto, RESPOSTA_LOCAL);
    assert_eq!(primario.streams(), 1, "uma tentativa de streaming");
    assert_eq!(
        primario.completes(),
        0,
        "o braco de streaming resolve sozinho: nao ha redo em batch aqui"
    );
}

/// #1249: o turno inteiro — entrada de verdade do agente — completa pelo
/// fallback local quando a rede cai.
///
/// Nao afirma nada sobre `last_turn_stats().provider`: neste ramo o
/// `/stats` anota o provider **pedido**, nao quem serviu, porque
/// `stream_complete_with_fallback` devolve so o stream e nunca diz o id de
/// quem respondeu (`turno.provider = provider.provider_id()` no ramo de
/// streaming, contra o `provider_usado` do caminho batch). E lacuna do
/// #984 no braco de streaming, anterior a este fix e fora do escopo dele:
/// consertar exige mudar o tipo de retorno de um metodo publico que o
/// gateway tambem usa.
#[tokio::test]
pub(super) async fn turno_de_streaming_completa_pelo_fallback_local() {
    let (runtime, primario) = runtime_com_fallback_local(FalhaSimulada::Transporte);

    let resposta = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        turno_de_streaming(&runtime, "sessao-1249"),
    )
    .await
    .expect("sem giro infinito")
    .expect("o turno tem de completar pelo local");

    assert!(resposta.contains(RESPOSTA_LOCAL), "veio: {resposta:?}");
    assert_eq!(primario.streams(), 1, "uma tentativa no primario");
    assert_eq!(
        primario.completes(),
        0,
        "o streaming resolveu no fallback: nenhum redo em batch"
    );
}

/// #1249, o coracao do criterio: **quantas** tentativas o primario leva.
///
/// Tempo virtual (`start_paused`) para o backoff do caminho 429 nao
/// custar os ~3,5s reais — o que o teste afirma e a contagem, nao o
/// relogio.
///
/// As duas metades correm o mesmo arranjo e mudam so a classe do erro:
/// - transporte → 1 tentativa e o fallback entra;
/// - 429 → `max_retries + 1` tentativas, comportamento historico intacto.
///
/// Se o fix tivesse misturado as duas classes (por exemplo tratando
/// transporte dentro do braco retryable), a primeira metade contaria 4.
#[tokio::test(start_paused = true)]
pub(super) async fn transporte_nao_queima_o_orcamento_de_retry() {
    let tentativas = |falha| async move {
        let (runtime, primario) = runtime_com_fallback_local(falha);
        let como_dyn: Arc<dyn LlmProvider> = primario.clone();
        let (_, quem) = runtime
            .complete_reportando_provider(&como_dyn, &pedido_simples())
            .await
            .expect("as duas classes acabam no fallback local");
        assert_eq!(quem, "ollama");
        primario.completes()
    };

    let teto = crate::provider_resilience::RetryPolicy::default().max_retries as usize + 1;
    assert!(teto > 1, "sem orcamento de retry o teste nao prova nada");

    assert_eq!(
        tentativas(FalhaSimulada::Transporte).await,
        1,
        "rede caida: uma tentativa e cai para o local"
    );
    assert_eq!(
        tentativas(FalhaSimulada::Sobrecarga).await,
        teto,
        "429 continua gastando o orcamento inteiro antes do fallback"
    );
}

/// #1249: sem fallback configurado, transporte continua virando erro.
///
/// A classe nova muda a POLITICA de tentativa, nao inventa resposta:
/// quem nao tem provider local ainda tem de ver a falha.
#[tokio::test]
pub(super) async fn transporte_sem_fallback_ainda_devolve_erro() {
    let runtime = AgentRuntime::new();
    let primario = std::sync::Arc::new(PrimarioQueFalha::nova(FalhaSimulada::Transporte));
    runtime.register_provider(primario.clone());
    let como_dyn: Arc<dyn LlmProvider> = primario.clone();

    let erro = runtime
        .complete_reportando_provider(&como_dyn, &pedido_simples())
        .await
        .expect_err("sem fallback nao ha o que responder");
    assert!(
        erro.to_string().contains("all providers failed"),
        "veio: {erro}"
    );
    assert_eq!(primario.completes(), 1);
}

/// O objetivo entra no prompt de sistema, e nao na mensagem (#983).
#[test]
pub(super) fn objetivo_entra_no_system_prompt() {
    let com = com_objetivo(
        Some("Voce e um assistente.".into()),
        Some("revisar a seguranca do gateway"),
    )
    .expect("com prompt e com goal");
    assert!(com.contains("Voce e um assistente."), "o prompt base fica");
    assert!(
        com.contains("revisar a seguranca do gateway"),
        "o objetivo entra"
    );

    // Sem prompt base, o objetivo sozinho ja e um system prompt valido.
    let so_goal = com_objetivo(None, Some("achar o bug")).expect("so o goal");
    assert!(so_goal.contains("achar o bug"));
}

/// Sem objetivo, o prompt nao muda — nem ganha bloco vazio.
#[test]
pub(super) fn sem_objetivo_o_prompt_fica_igual() {
    assert_eq!(
        com_objetivo(Some("Voce e um assistente.".into()), None).as_deref(),
        Some("Voce e um assistente.")
    );
    assert_eq!(com_objetivo(None, None), None);

    // `/goal` sem argumento e consulta; string vazia nao e objetivo.
    for vazio in ["", "   ", "\n\t "] {
        assert_eq!(
            com_objetivo(Some("base".into()), Some(vazio)).as_deref(),
            Some("base"),
            "objetivo {vazio:?} nao deveria entrar"
        );
        assert_eq!(com_objetivo(None, Some(vazio)), None);
    }
}
