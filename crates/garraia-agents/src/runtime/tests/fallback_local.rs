use super::*;

// ─── #1249: rede caida cai para o provider local ──────────────────────

/// Como o primario falha, e a diferenca que a issue #1249 cobra.
#[derive(Clone, Copy, Debug)]
pub(super) enum FalhaSimulada {
    /// Rede caida: o cabo saiu, o DNS morreu, a porta nao responde. O
    /// provider classifica no ponto de captura (`erro_de_envio`), entao
    /// aqui chega ja como `Error::Transport`.
    Transporte,
    /// O provider **respondeu** dizendo "agora nao". Fica no comportamento
    /// historico: retry com backoff no mesmo endereco antes do fallback.
    Sobrecarga,
    /// #1735: chave invalida/expirada - o provider responde 401.
    Credencial,
    /// #1735: erro do PEDIDO (400) - outro provider nao o corrige.
    PedidoInvalido,
    /// #1743: chave valida mas sem permissao para o modelo - o provider
    /// responde 403. E credencial tambem, e o status anotado e o 403.
    Permissao,
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
            Self::Credencial => Error::Agent(
                "openai API error: status=401 Unauthorized, body=User not found.".to_string(),
            ),
            Self::PedidoInvalido => Error::Agent(
                "openai API error: status=400 Bad Request, body=not a valid model ID".to_string(),
            ),
            Self::Permissao => Error::Agent(
                "openai API error: status=403 Forbidden, body=Key has no access to this model"
                    .to_string(),
            ),
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

// ─── #1541 (defeito 3): o fallback usa o modelo DELE ──────────────────

/// Fallback que anota o `model` que recebeu. Sem isso o teste so veria
/// "respondeu" e nao "respondeu com o modelo certo", que e o defeito.
pub(super) struct FallbackQueAnotaOModelo {
    pub(super) visto: std::sync::Mutex<Vec<String>>,
}

impl FallbackQueAnotaOModelo {
    pub(super) fn novo() -> Self {
        Self {
            visto: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn anota(&self, request: &LlmRequest) {
        self.visto.lock().unwrap().push(request.model.clone());
    }

    pub(super) fn modelos_vistos(&self) -> Vec<String> {
        self.visto.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl LlmProvider for FallbackQueAnotaOModelo {
    fn provider_id(&self) -> &str {
        "ollama"
    }

    async fn complete(&self, request: &LlmRequest) -> Result<LlmResponse> {
        self.anota(request);
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
        request: &LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        self.anota(request);
        Ok(Box::pin(futures::stream::iter(vec![
            Ok(StreamEvent::TextDelta(RESPOSTA_LOCAL.to_string())),
            Ok(StreamEvent::MessageStop),
        ])))
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

fn runtime_com_fallback_que_anota() -> (
    AgentRuntime,
    Arc<PrimarioQueFalha>,
    Arc<FallbackQueAnotaOModelo>,
) {
    let runtime = AgentRuntime::new();
    let primario = Arc::new(PrimarioQueFalha::nova(FalhaSimulada::Transporte));
    let fallback = Arc::new(FallbackQueAnotaOModelo::novo());
    runtime.register_provider(primario.clone());
    runtime.register_provider(fallback.clone());
    runtime.set_fallback_providers(vec!["ollama".to_string()]);
    (runtime, primario, fallback)
}

/// O caso da issue: `gpt-4o` e o nome que **a nuvem** entende. Repassado ao
/// fallback, o openrouter respondia 400 `is not a valid model ID` e o
/// usuario via `all providers failed` — com o fallback de pe o tempo todo.
#[tokio::test]
pub(super) async fn fallback_nao_herda_o_modelo_do_primario_em_batch() {
    let (runtime, primario, fallback) = runtime_com_fallback_que_anota();
    let como_dyn: Arc<dyn LlmProvider> = primario.clone();

    let (_resposta, quem) = runtime
        .complete_reportando_provider(&como_dyn, &pedido_simples())
        .await
        .expect("o fallback completa o turno");

    assert_eq!(quem, "ollama");
    assert_eq!(
        fallback.modelos_vistos(),
        vec![String::new()],
        "modelo vazio = 'provider, use o seu' — nunca o `gpt-4o` do primario"
    );
}

/// O mesmo no braco de streaming, que tem laco de fallback proprio.
#[tokio::test]
pub(super) async fn fallback_nao_herda_o_modelo_do_primario_em_streaming() {
    let (runtime, primario, fallback) = runtime_com_fallback_que_anota();
    let como_dyn: Arc<dyn LlmProvider> = primario.clone();

    let mut stream = runtime
        .stream_complete_with_fallback(&como_dyn, &pedido_simples())
        .await
        .expect("o stream do fallback chega");
    while stream.next().await.is_some() {}

    assert_eq!(fallback.modelos_vistos(), vec![String::new()]);
}

/// A excecao estreita: quando o `model` **nomeia o proprio fallback**, ele
/// sobrevive — `openrouter/auto` indo para o openrouter e um endereco
/// valido, nao herança acidental do primario.
#[tokio::test]
pub(super) async fn modelo_que_nomeia_o_fallback_sobrevive() {
    let (runtime, primario, fallback) = runtime_com_fallback_que_anota();
    let como_dyn: Arc<dyn LlmProvider> = primario.clone();

    let mut pedido = pedido_simples();
    pedido.model = "ollama/qwen3.8:latest".to_string();

    runtime
        .complete_reportando_provider(&como_dyn, &pedido)
        .await
        .expect("o fallback completa o turno");

    assert_eq!(
        fallback.modelos_vistos(),
        vec!["ollama/qwen3.8:latest".to_string()]
    );
}

/// #1752: um preset do OpenRouter sobrevive no fallback apenas quando o
/// fallback E o OpenRouter (o preset nomeia o proprio fallback); para qualquer
/// outro fallback ele zera, a mesma regra estreita da #1541.
#[test]
pub(super) fn preset_sobrevive_so_quando_o_fallback_e_o_openrouter() {
    let mut pedido = pedido_simples();
    pedido.model = "z-ai/x@preset/y".to_string();

    assert_eq!(
        requisicao_para_o_fallback(&pedido, "openrouter").model,
        "z-ai/x@preset/y",
        "o preset roteia para o openrouter, entao sobrevive intacto"
    );
    assert_eq!(
        requisicao_para_o_fallback(&pedido, "ollama").model,
        "",
        "o preset e do openrouter, nao do ollama: zera para o modelo configurado"
    );
}

// ─── #1735: credencial recusada (401/403) cai para o fallback ─────────

/// 401 no primario: uma tentativa (nao e transitorio) e o local responde.
#[tokio::test(start_paused = true)]
pub(super) async fn credencial_recusada_cai_para_o_fallback_em_batch() {
    let (runtime, primario) = runtime_com_fallback_local(FalhaSimulada::Credencial);
    let como_dyn: Arc<dyn LlmProvider> = primario.clone();

    let (_, quem) = runtime
        .complete_reportando_provider(&como_dyn, &pedido_simples())
        .await
        .expect("o fallback local tem de completar o turno");

    assert_eq!(quem, "ollama");
    assert_eq!(primario.completes(), 1, "401 nao e transitorio: sem retry");
}

#[tokio::test]
pub(super) async fn credencial_recusada_cai_para_o_fallback_em_streaming() {
    let (runtime, primario) = runtime_com_fallback_local(FalhaSimulada::Credencial);
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
}

/// 400 e erro do pedido: propaga sem tocar no fallback.
#[tokio::test]
pub(super) async fn erro_do_pedido_nao_aciona_o_fallback() {
    let (runtime, primario) = runtime_com_fallback_local(FalhaSimulada::PedidoInvalido);
    let como_dyn: Arc<dyn LlmProvider> = primario.clone();

    let erro = runtime
        .complete_reportando_provider(&como_dyn, &pedido_simples())
        .await
        .expect_err("400 propaga");
    assert!(erro.to_string().contains("status=400"), "veio: {erro}");
    assert_eq!(primario.completes(), 1);

    assert!(
        runtime
            .stream_complete_with_fallback(&como_dyn, &pedido_simples())
            .await
            .is_err(),
        "400 em streaming tambem propaga"
    );
}

// --- #1743: a recusa de credencial fica anotada para o /api/health ---

/// O primario cuja chave pode ser trocada no meio do teste: morta (401) ate
/// alguem chamar `aceitar()`, e depois responde como qualquer provider.
pub(super) struct PrimarioComChaveTrocavel {
    valida: std::sync::atomic::AtomicBool,
}

impl PrimarioComChaveTrocavel {
    pub(super) fn com_chave_morta() -> Self {
        Self {
            valida: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// O operador trocou a chave: a partir daqui o provider aceita.
    pub(super) fn aceitar(&self) {
        self.valida.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn chave_valida(&self) -> bool {
        self.valida.load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl LlmProvider for PrimarioComChaveTrocavel {
    fn provider_id(&self) -> &str {
        "openai"
    }

    async fn complete(&self, _request: &LlmRequest) -> Result<LlmResponse> {
        if !self.chave_valida() {
            return Err(FalhaSimulada::Credencial.erro());
        }
        Ok(LlmResponse {
            content: vec![ContentBlock::Text {
                text: "respondi da nuvem".to_string(),
            }],
            model: "gpt-4o".to_string(),
            stop_reason: None,
            usage: None,
        })
    }

    async fn stream_complete(
        &self,
        _request: &LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        if !self.chave_valida() {
            return Err(FalhaSimulada::Credencial.erro());
        }
        Ok(Box::pin(futures::stream::iter(vec![
            Ok(StreamEvent::TextDelta("respondi da nuvem".to_string())),
            Ok(StreamEvent::MessageStop),
        ])))
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

/// 401 no primario, caminho batch: o runtime anota status e hora para o
/// health, so do provider que recusou - o fallback que respondeu fica limpo.
#[tokio::test(start_paused = true)]
pub(super) async fn credencial_recusada_fica_anotada_para_o_health_em_batch() {
    let (runtime, primario) = runtime_com_fallback_local(FalhaSimulada::Credencial);
    let como_dyn: Arc<dyn LlmProvider> = primario.clone();
    assert!(
        runtime.recusas_de_credencial().is_empty(),
        "nada antes do turno"
    );

    runtime
        .complete_reportando_provider(&como_dyn, &pedido_simples())
        .await
        .expect("o fallback local completa o turno");

    let recusa = runtime
        .recusa_de_credencial("openai")
        .expect("a recusa do primario fica anotada");
    assert_eq!(recusa.provider, "openai");
    assert_eq!(recusa.status, 401);
    assert_eq!(recusa.seguidas, 1);
    assert!(
        runtime.recusa_de_credencial("ollama").is_none(),
        "o fallback respondeu: nada a anotar dele"
    );

    // Segundo turno com a mesma chave morta: continua UMA linha, contando.
    runtime
        .complete_reportando_provider(&como_dyn, &pedido_simples())
        .await
        .expect("o fallback local completa o turno");
    let todas = runtime.recusas_de_credencial();
    assert_eq!(todas.len(), 1, "uma linha por provider, nao um historico");
    assert_eq!(todas[0].seguidas, 2);
}

/// O mesmo sinal no braco de streaming, que tem laco proprio.
#[tokio::test]
pub(super) async fn credencial_recusada_fica_anotada_para_o_health_em_streaming() {
    let (runtime, primario) = runtime_com_fallback_local(FalhaSimulada::Credencial);
    let como_dyn: Arc<dyn LlmProvider> = primario.clone();

    let _stream = runtime
        .stream_complete_with_fallback(&como_dyn, &pedido_simples())
        .await
        .expect("o stream do fallback local tem de chegar");

    let recusa = runtime
        .recusa_de_credencial("openai")
        .expect("a recusa do primario fica anotada tambem em streaming");
    assert_eq!(recusa.status, 401);
    assert!(runtime.recusa_de_credencial("ollama").is_none());
}

// ─── #1776: 200 vazio nao e sucesso ─────────────────────────────────────

/// Consumo fixo de cada turno 200-vazio que o mock devolve.
const USO_DO_TURNO_VAZIO: (u32, u32) = (7, 3);

/// Provider que devolve 200 vazio (com uso fixo) nas N primeiras chamadas e
/// depois responde texto; com N gigante, nunca responde.
pub(super) struct VazioDepoisTexto {
    id: &'static str,
    vazias_restantes: std::sync::atomic::AtomicUsize,
    chamadas_complete: std::sync::atomic::AtomicUsize,
}

impl VazioDepoisTexto {
    pub(super) fn novo(id: &'static str, vazias: usize) -> Self {
        Self {
            id,
            vazias_restantes: std::sync::atomic::AtomicUsize::new(vazias),
            chamadas_complete: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    pub(super) fn completes(&self) -> usize {
        self.chamadas_complete
            .load(std::sync::atomic::Ordering::SeqCst)
    }
}

#[async_trait::async_trait]
impl LlmProvider for VazioDepoisTexto {
    fn provider_id(&self) -> &str {
        self.id
    }

    async fn complete(&self, _request: &LlmRequest) -> Result<LlmResponse> {
        self.chamadas_complete
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let restantes = self
            .vazias_restantes
            .load(std::sync::atomic::Ordering::SeqCst);
        if restantes > 0 {
            self.vazias_restantes
                .store(restantes - 1, std::sync::atomic::Ordering::SeqCst);
            return Ok(LlmResponse {
                content: Vec::new(),
                model: "gpt-4o".to_string(),
                stop_reason: None,
                usage: Some(Usage {
                    input_tokens: USO_DO_TURNO_VAZIO.0,
                    output_tokens: USO_DO_TURNO_VAZIO.1,
                }),
            });
        }
        Ok(LlmResponse {
            content: vec![ContentBlock::Text {
                text: "respondi da nuvem".to_string(),
            }],
            model: "gpt-4o".to_string(),
            stop_reason: None,
            usage: Some(Usage {
                input_tokens: 11,
                output_tokens: 5,
            }),
        })
    }

    async fn stream_complete(
        &self,
        _request: &LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        Ok(Box::pin(futures::stream::iter(vec![
            Ok(StreamEvent::TextDelta("respondi da nuvem".to_string())),
            Ok(StreamEvent::MessageStop),
        ])))
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

/// Runtime com o primario dado e o Ollama local no fallback - o cenario do
/// wizard, so que com o primario controlado pelo teste.
pub(super) fn runtime_com_primario_e_fallback_local(
    primario: std::sync::Arc<dyn LlmProvider>,
) -> AgentRuntime {
    let runtime = AgentRuntime::new();
    runtime.register_provider(primario);
    runtime.register_provider(std::sync::Arc::new(LocalQueResponde));
    runtime.set_fallback_providers(vec!["ollama".to_string()]);
    runtime
}

/// #1776, caminho batch: o primeiro 200 vazio re-tenta no proprio primario e
/// a segunda tentativa responde - o fallback local nem e consultado. O
/// consumo do turno vazio descartado vai embutido no `usage` final.
#[tokio::test(start_paused = true)]
pub(super) async fn ok_vazio_no_primario_reintenta_e_responde_sem_fallback() {
    let primario = std::sync::Arc::new(VazioDepoisTexto::novo("openai", 1));
    let runtime = runtime_com_primario_e_fallback_local(primario.clone());
    let como_dyn: std::sync::Arc<dyn LlmProvider> = primario.clone();

    let (resp, quem) = runtime
        .complete_reportando_provider(&como_dyn, &pedido_simples())
        .await
        .expect("a segunda tentativa do primario responde");

    assert_eq!(quem, "openai", "o fallback nem e consultado");
    assert_eq!(primario.completes(), 2, "1 vazia + 1 com texto");
    let texto = resp
        .content
        .iter()
        .filter_map(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(texto, "respondi da nuvem");
    let uso = resp.usage.expect("o turno final traz usage");
    assert_eq!(
        (uso.input_tokens, uso.output_tokens),
        (7 + 11, 3 + 5),
        "o consumo do turno vazio descartado nao desaparece"
    );
}

/// #1776: o primario so devolve 200 vazio. O orcamento de retry e gasto
/// inteiro, o fallback local entra e responde - e o consumo de TODAS as
/// tentativas vazias aparece no `usage` da resposta do fallback.
#[tokio::test(start_paused = true)]
pub(super) async fn ok_vazio_esgota_o_orcamento_e_cai_para_o_fallback_local() {
    let primario = std::sync::Arc::new(VazioDepoisTexto::novo("openai", usize::MAX));
    let runtime = runtime_com_primario_e_fallback_local(primario.clone());
    let como_dyn: std::sync::Arc<dyn LlmProvider> = primario.clone();

    let (resp, quem) = runtime
        .complete_reportando_provider(&como_dyn, &pedido_simples())
        .await
        .expect("o fallback local responde depois do esgotamento");

    assert_eq!(
        quem, "ollama",
        "o vazio do primario empurra o turno ao fallback"
    );
    let teto = crate::RetryPolicy::default().max_retries as usize + 1;
    assert_eq!(primario.completes(), teto, "orcamento de retry respeitado");
    assert_eq!(
        resp.content.last().and_then(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        }),
        Some(RESPOSTA_LOCAL)
    );
    let teto = teto as u32;
    let uso = resp.usage.expect("o consumo das vazias nao se perde");
    assert_eq!(
        (uso.input_tokens, uso.output_tokens),
        (USO_DO_TURNO_VAZIO.0 * teto, USO_DO_TURNO_VAZIO.1 * teto),
        "o local respondeu sem usage: o usage final e so das vazias"
    );
}

/// #1776: primario e fallback so devolvem 200 vazio. Nada de erro: a ultima
/// resposta vazia volta, marcada com quem serviu, e o consumo de todas as
/// tentativas (primario + fallback) vai no `usage`.
#[tokio::test(start_paused = true)]
pub(super) async fn ok_vazio_em_todos_volta_vazio_com_o_uso_de_todas_as_tentativas() {
    let runtime = AgentRuntime::new();
    let primario = std::sync::Arc::new(VazioDepoisTexto::novo("openai", usize::MAX));
    let fallback = std::sync::Arc::new(VazioDepoisTexto::novo("ollama", usize::MAX));
    runtime.register_provider(primario.clone());
    runtime.register_provider(fallback.clone());
    runtime.set_fallback_providers(vec!["ollama".to_string()]);
    let como_dyn: std::sync::Arc<dyn LlmProvider> = primario.clone();

    let (resp, quem) = runtime
        .complete_reportando_provider(&como_dyn, &pedido_simples())
        .await
        .expect("a resposta vazia de alguem volta - nunca erro");

    assert_eq!(quem, "ollama", "a ultima vazia e a do fallback");
    assert!(
        resp.content.is_empty(),
        "o turno volta vazio, como hoje, para o guarda do chamador"
    );
    let teto = crate::RetryPolicy::default().max_retries as usize + 1;
    assert_eq!(primario.completes(), teto);
    assert_eq!(fallback.completes(), 1, "o fallback recebe a vez");
    let teto = teto as u32 + 1;
    let uso = resp.usage.expect("o consumo de todos os turnos vazios");
    assert_eq!(
        (uso.input_tokens, uso.output_tokens),
        (USO_DO_TURNO_VAZIO.0 * teto, USO_DO_TURNO_VAZIO.1 * teto)
    );
}

/// #1776: turno so-de-ferramenta (o modelo pediu a execucao) e saudavel -
/// nada de retry, nada de fallback, o bloco volta intacto de primeira.
#[tokio::test(start_paused = true)]
pub(super) async fn turno_so_de_ferramenta_nao_e_vazio() {
    pub(super) struct SoFerramenta {
        chamadas_complete: std::sync::atomic::AtomicUsize,
    }

    impl SoFerramenta {
        fn completes(&self) -> usize {
            self.chamadas_complete
                .load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl LlmProvider for SoFerramenta {
        fn provider_id(&self) -> &str {
            "openai"
        }

        async fn complete(&self, _request: &LlmRequest) -> Result<LlmResponse> {
            self.chamadas_complete
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(LlmResponse {
                content: vec![ContentBlock::ToolUse {
                    id: "call_1".to_string(),
                    name: "busca".to_string(),
                    input: serde_json::json!({ "q": "garraia" }),
                }],
                model: "gpt-4o".to_string(),
                stop_reason: None,
                usage: Some(Usage {
                    input_tokens: 9,
                    output_tokens: 4,
                }),
            })
        }

        async fn stream_complete(
            &self,
            _request: &LlmRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
            Ok(Box::pin(futures::stream::iter(vec![
                Ok(StreamEvent::TextDelta(String::new())),
                Ok(StreamEvent::MessageStop),
            ])))
        }

        async fn health_check(&self) -> Result<bool> {
            Ok(true)
        }
    }

    let primario = std::sync::Arc::new(SoFerramenta {
        chamadas_complete: std::sync::atomic::AtomicUsize::new(0),
    });
    let runtime = runtime_com_primario_e_fallback_local(primario.clone());
    let como_dyn: std::sync::Arc<dyn LlmProvider> = primario.clone();

    let (resp, quem) = runtime
        .complete_reportando_provider(&como_dyn, &pedido_simples())
        .await
        .expect("o turno so-de-ferramenta e sucesso de primeira");

    assert_eq!(quem, "openai");
    assert_eq!(
        primario.completes(),
        1,
        "ferramenta nao e tratada como vazio"
    );
    assert!(
        resp.content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolUse { .. })),
        "o bloco de ferramenta volta intacto"
    );
}

/// Criterio de aceitacao da #1743: um turno que passa expira a recusa - nos
/// dois caminhos.
#[tokio::test(start_paused = true)]
pub(super) async fn turno_que_passa_expira_a_recusa_de_credencial() {
    let runtime = AgentRuntime::new();
    let primario = Arc::new(PrimarioComChaveTrocavel::com_chave_morta());
    runtime.register_provider(primario.clone());
    runtime.register_provider(Arc::new(LocalQueResponde));
    runtime.set_fallback_providers(vec!["ollama".to_string()]);
    let como_dyn: Arc<dyn LlmProvider> = primario.clone();

    let (_, quem) = runtime
        .complete_reportando_provider(&como_dyn, &pedido_simples())
        .await
        .expect("o fallback local completa o turno");
    assert_eq!(quem, "ollama");
    assert!(runtime.recusa_de_credencial("openai").is_some());

    primario.aceitar();
    let (_, quem) = runtime
        .complete_reportando_provider(&como_dyn, &pedido_simples())
        .await
        .expect("com a chave nova o primario responde");
    assert_eq!(quem, "openai");
    assert!(
        runtime.recusa_de_credencial("openai").is_none(),
        "o turno que passou expirou a recusa"
    );

    // E em streaming: recusa anotada de novo, e expirada de novo.
    let primario_stream = Arc::new(PrimarioComChaveTrocavel::com_chave_morta());
    let runtime_stream = AgentRuntime::new();
    runtime_stream.register_provider(primario_stream.clone());
    runtime_stream.register_provider(Arc::new(LocalQueResponde));
    runtime_stream.set_fallback_providers(vec!["ollama".to_string()]);
    let como_dyn_stream: Arc<dyn LlmProvider> = primario_stream.clone();
    let _ = runtime_stream
        .stream_complete_with_fallback(&como_dyn_stream, &pedido_simples())
        .await
        .expect("fallback em streaming");
    assert!(runtime_stream.recusa_de_credencial("openai").is_some());
    primario_stream.aceitar();
    let _ = runtime_stream
        .stream_complete_with_fallback(&como_dyn_stream, &pedido_simples())
        .await
        .expect("primario em streaming");
    assert!(runtime_stream.recusa_de_credencial("openai").is_none());
}

/// 400 e erro do pedido e 429 e sobrecarga: nenhum dos dois e recusa de
/// credencial, e o health nao pode acusar a chave por eles.
#[tokio::test(start_paused = true)]
pub(super) async fn erro_do_pedido_e_sobrecarga_nao_sao_recusa_de_credencial() {
    for falha in [FalhaSimulada::PedidoInvalido, FalhaSimulada::Sobrecarga] {
        let (runtime, primario) = runtime_com_fallback_local(falha);
        let como_dyn: Arc<dyn LlmProvider> = primario.clone();
        let _ = runtime
            .complete_reportando_provider(&como_dyn, &pedido_simples())
            .await;
        assert!(
            runtime.recusas_de_credencial().is_empty(),
            "{falha:?} nao e recusa de credencial"
        );
    }
}

/// 403 tambem e credencial (sem permissao), e o status anotado e o 403.
#[tokio::test(start_paused = true)]
pub(super) async fn recusa_403_fica_anotada_com_o_proprio_status() {
    let (runtime, primario) = runtime_com_fallback_local(FalhaSimulada::Permissao);
    let como_dyn: Arc<dyn LlmProvider> = primario.clone();

    let (_, quem) = runtime
        .complete_reportando_provider(&como_dyn, &pedido_simples())
        .await
        .expect("o fallback local completa o turno");
    assert_eq!(quem, "ollama");
    assert_eq!(
        runtime.recusa_de_credencial("openai").map(|r| r.status),
        Some(403)
    );
}
