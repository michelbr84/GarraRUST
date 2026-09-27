use super::*;

/// Provider cujo `stream_complete` abre a conexao normalmente
/// (`Ok(stream)`) mas o byte-stream quebra no meio da leitura — o
/// cenario do #1176 (`stream read error: error decoding response
/// body`, rede movel / upstream do provider free cortando o corpo).
///
/// `stream_complete_with_fallback` nao ve essa quebra: ja devolveu
/// `Ok(stream)` antes de qualquer evento existir. Antes do fix, o
/// `event?` no loop de consumo propagava o `Err` direto, matando o
/// turno inteiro sem nenhuma tentativa.
pub(super) struct StreamQuebraNoMeio {
    /// Se `Some`, emite este texto pelo sink antes de quebrar — simula
    /// a quebra **depois** de conteudo ja ter sido entregue ao usuario.
    pub(super) texto_antes_da_quebra: Option<&'static str>,
    pub(super) chamadas_stream: std::sync::atomic::AtomicUsize,
    pub(super) chamadas_complete: std::sync::atomic::AtomicUsize,
    /// O que o caminho batch responde quando o redo cai la.
    pub(super) resposta_batch: &'static str,
}

impl StreamQuebraNoMeio {
    pub(super) fn sem_conteudo(resposta_batch: &'static str) -> Self {
        Self {
            texto_antes_da_quebra: None,
            chamadas_stream: std::sync::atomic::AtomicUsize::new(0),
            chamadas_complete: std::sync::atomic::AtomicUsize::new(0),
            resposta_batch,
        }
    }

    pub(super) fn com_texto_antes(texto: &'static str) -> Self {
        Self {
            texto_antes_da_quebra: Some(texto),
            chamadas_stream: std::sync::atomic::AtomicUsize::new(0),
            chamadas_complete: std::sync::atomic::AtomicUsize::new(0),
            resposta_batch: "",
        }
    }
}

#[async_trait::async_trait]
impl LlmProvider for StreamQuebraNoMeio {
    fn provider_id(&self) -> &str {
        "stream_quebra_no_meio"
    }

    async fn complete(&self, _request: &LlmRequest) -> Result<LlmResponse> {
        self.chamadas_complete
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(LlmResponse {
            content: vec![ContentBlock::Text {
                text: self.resposta_batch.to_string(),
            }],
            model: "modelo-do-batch".to_string(),
            stop_reason: None,
            usage: None,
        })
    }

    async fn stream_complete(
        &self,
        _request: &LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        self.chamadas_stream
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let quebra = Err(Error::Agent(
            "stream read error: error decoding response body".to_string(),
        ));
        let eventos: Vec<Result<StreamEvent>> = match self.texto_antes_da_quebra {
            Some(texto) => vec![Ok(StreamEvent::TextDelta(texto.to_string())), quebra],
            None => vec![quebra],
        };
        Ok(Box::pin(futures::stream::iter(eventos)))
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

/// #1176: stream que quebra no meio sem ter entregue nada refaz o turno
/// em batch, em vez de matar o turno na primeira tentativa.
#[tokio::test]
pub(super) async fn stream_quebra_no_meio_sem_conteudo_refaz_em_batch() {
    let runtime = AgentRuntime::new();
    let provider = std::sync::Arc::new(StreamQuebraNoMeio::sem_conteudo(
        "resposta que o batch soube dar",
    ));
    runtime.register_provider(provider.clone());

    let resposta = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        turno_de_streaming(&runtime, "sessao-1176-a"),
    )
    .await
    .expect("sem giro infinito")
    .expect("o redo em batch tem de completar o turno");
    assert_eq!(resposta, "resposta que o batch soube dar");

    assert_eq!(
        provider
            .chamadas_stream
            .load(std::sync::atomic::Ordering::SeqCst),
        1,
        "so uma tentativa de streaming: a segunda vai por batch"
    );
    assert_eq!(
        provider
            .chamadas_complete
            .load(std::sync::atomic::Ordering::SeqCst),
        1,
        "o redo cai no batch exatamente uma vez"
    );
}

/// #1176: texto ja entregue ao sink antes da quebra nunca e escondido.
///
/// Reenviar duplicaria o que o usuario ja viu, entao aqui o turno tem de
/// terminar em erro (o mesmo que acontecia antes do fix) em vez de
/// arriscar um redo silencioso — a mesma logica que
/// `texto_ja_entregue_nao_vira_erro_quando_o_redo_acaba` cobre para o
/// caminho de turno vazio, na direcao oposta: la o texto salva o turno
/// do erro, aqui o texto e o motivo de nao arriscar reenviar.
#[tokio::test]
pub(super) async fn quebra_no_meio_com_texto_ja_entregue_nao_refaz() {
    let runtime = AgentRuntime::new();
    let provider = std::sync::Arc::new(StreamQuebraNoMeio::com_texto_antes("PARTE-UM"));
    runtime.register_provider(provider.clone());

    let erro = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        turno_de_streaming(&runtime, "sessao-1176-b"),
    )
    .await
    .expect("sem giro infinito")
    .expect_err("texto parcial entregue nao pode virar redo silencioso");
    assert!(
        erro.to_string().contains("stream read error"),
        "o erro original precisa chegar ao chamador; veio: {erro}"
    );
    assert_eq!(
        provider
            .chamadas_complete
            .load(std::sync::atomic::Ordering::SeqCst),
        0,
        "com texto ja entregue, o batch nunca deveria ser tentado"
    );
}

/// #1176: quebra no meio duas vezes seguidas usa o redo uma unica vez.
///
/// Sequencia: (1) stream quebra sem conteudo, gasta o unico redo; (2) o
/// batch devolve uma ferramenta, o que devolve o loop ao streaming; (3)
/// o stream quebra de novo, agora com o redo ja gasto — tem de virar
/// erro em vez de girar para sempre.
pub(super) struct StreamQuebraDuasVezes {
    pub(super) chamadas_stream: std::sync::atomic::AtomicUsize,
    pub(super) chamadas_complete: std::sync::atomic::AtomicUsize,
}

#[async_trait::async_trait]
impl LlmProvider for StreamQuebraDuasVezes {
    fn provider_id(&self) -> &str {
        "stream_quebra_duas_vezes"
    }

    async fn complete(&self, _request: &LlmRequest) -> Result<LlmResponse> {
        let n = self
            .chamadas_complete
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(LlmResponse {
            content: vec![ContentBlock::ToolUse {
                id: format!("t{n}"),
                name: "eco".to_string(),
                input: serde_json::json!({ "volta": n }),
            }],
            model: "m".to_string(),
            stop_reason: None,
            usage: None,
        })
    }

    async fn stream_complete(
        &self,
        _request: &LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        self.chamadas_stream
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(Box::pin(futures::stream::iter(vec![Err(Error::Agent(
            "stream read error: conexao caiu de novo".to_string(),
        ))])))
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

#[tokio::test]
pub(super) async fn quebra_no_meio_duas_vezes_usa_o_redo_uma_unica_vez() {
    let runtime = AgentRuntime::new();
    let provider = std::sync::Arc::new(StreamQuebraDuasVezes {
        chamadas_stream: std::sync::atomic::AtomicUsize::new(0),
        chamadas_complete: std::sync::atomic::AtomicUsize::new(0),
    });
    runtime.register_provider(provider.clone());
    runtime.register_tool(stub("eco"));

    let erro = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        turno_de_streaming(&runtime, "sessao-1176-c"),
    )
    .await
    .expect("sem giro infinito: um redo so")
    .expect_err("com o redo gasto, a segunda quebra tem de virar erro");
    assert!(
        erro.to_string().contains("stream read error"),
        "veio: {erro}"
    );
    assert_eq!(
        provider
            .chamadas_stream
            .load(std::sync::atomic::Ordering::SeqCst),
        2,
        "streaming: a tentativa original e a que segue depois do redo"
    );
    assert_eq!(
        provider
            .chamadas_complete
            .load(std::sync::atomic::Ordering::SeqCst),
        1,
        "batch chamado uma unica vez, pelo redo"
    );
}

/// O turno que caiu no fallback nao-streaming tambem e anotado.
///
/// O `/stats` (#984) e o `/status` (#940) respondiam "nenhum turno ainda"
/// depois de um turno inteiro sempre que o provider nao fazia streaming:
/// dos tres `return Ok` do `stream_turn_with_sink`, so um anotava. Achado
/// rodando o binario — `Turnos 1` e `Ultimo turno nenhum ainda` na mesma
/// tela.
#[tokio::test]
pub(super) async fn turno_que_caiu_no_fallback_tambem_e_anotado() {
    let runtime = AgentRuntime::new();
    runtime.register_provider(std::sync::Arc::new(SoBatch));

    let (tx, mut rx) = mpsc::channel::<crate::turn_events::TurnEvent>(64);
    // O receptor precisa existir enquanto o turno roda: o canal e
    // limitado e o `sink.text` bloquearia.
    let dreno = tokio::spawn(async move { while rx.recv().await.is_some() {} });

    let resposta = runtime
        .process_message_streaming_with_events(
            "sessao-de-teste",
            "oi",
            &[],
            tx,
            None,
            None,
            None,
            None,
            None,
            None,
            &ExecContext::default(),
        )
        .await
        .expect("o turno completa pelo fallback");
    assert!(resposta.contains("pronto"), "veio: {resposta:?}");
    dreno.await.expect("dreno");

    let st = runtime
        .last_turn_stats("sessao-de-teste")
        .expect("o turno tem de ficar registrado");
    assert_eq!(st.provider, "so_batch");
    // O modelo vem da **resposta**, e nao do pedido: neste ramo ha uma
    // `LlmResponse`, entao da para confirmar — e o `/stats` diz isso.
    assert_eq!(st.model, "modelo-que-respondeu");
    assert!(st.model_confirmado, "aqui o modelo e confirmado");
    assert!(st.tokens_conhecidos, "e os tokens existem");
    assert_eq!(st.input_tokens, 11);
    assert_eq!(st.output_tokens, 7);
}
