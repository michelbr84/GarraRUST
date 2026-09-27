use super::*;

/// Provider que so sabe responder de uma vez — o `stream_complete` cai no
/// padrao do trait, que devolve erro.
///
/// E o que existe de verdade: Ollama antigo, llama.cpp sem SSE, ou
/// qualquer provider num momento em que o streaming falha. O turno entao
/// segue pelo ramo de fallback nao-streaming, e era **exatamente** esse
/// ramo que nao anotava nada.
pub(super) struct SoBatch;

#[async_trait::async_trait]
impl LlmProvider for SoBatch {
    fn provider_id(&self) -> &str {
        "so_batch"
    }

    async fn complete(&self, _request: &LlmRequest) -> Result<LlmResponse> {
        Ok(LlmResponse {
            content: vec![ContentBlock::Text {
                text: "pronto".to_string(),
            }],
            model: "modelo-que-respondeu".to_string(),
            stop_reason: None,
            usage: Some(crate::providers::Usage {
                input_tokens: 11,
                output_tokens: 7,
            }),
        })
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

/// Provider cujo `stream_complete` **funciona** e mesmo assim nao emite
/// nada: o stream fecha sem `TextDelta` e sem ferramenta.
///
/// E o #1048 visto de perto. `stream_complete_with_fallback` devolve
/// `Ok(stream)` antes de qualquer evento existir, entao nem o retry nem o
/// fallback de provider chegam a acontecer — o turno terminava em `Ok("")`
/// e o canal publicava uma bolha em branco.
pub(super) struct StreamVazio {
    /// O que o caminho batch responde. Vazio simula o provider que nao
    /// responde por nenhum dos dois caminhos.
    pub(super) batch: &'static str,
}

#[async_trait::async_trait]
impl LlmProvider for StreamVazio {
    fn provider_id(&self) -> &str {
        "stream_vazio"
    }

    async fn complete(&self, _request: &LlmRequest) -> Result<LlmResponse> {
        Ok(LlmResponse {
            content: vec![ContentBlock::Text {
                text: self.batch.to_string(),
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
        Ok(Box::pin(futures::stream::iter(vec![Ok(
            StreamEvent::MessageStop,
        )])))
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

/// Roda um turno de streaming ate o fim, drenando o canal de eventos.
///
/// O dreno nao e detalhe: o canal e limitado e o `sink.text` bloquearia
/// sem alguem lendo do outro lado.
pub(super) async fn turno_de_streaming(runtime: &AgentRuntime, sessao: &str) -> Result<String> {
    let (tx, mut rx) = mpsc::channel::<crate::turn_events::TurnEvent>(64);
    let dreno = tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let resultado = runtime
        .process_message_streaming_with_events(
            sessao,
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
        .await;
    dreno.await.expect("dreno");
    resultado
}

/// #1048: a diferenca entre "nao disse nada" e "disse o marcador".
///
/// `extract_text` precisa continuar devolvendo o marcador — ha caminhos
/// que exigem uma `String` sempre. Quem decide se o turno veio vazio usa
/// `extract_text_opt`, senao a decisao passa a depender de comparar texto
/// com uma frase em ingles.
#[test]
pub(super) fn extract_text_opt_distingue_vazio_de_marcador() {
    let vazios = [
        vec![],
        vec![ContentBlock::Text {
            text: String::new(),
        }],
        vec![ContentBlock::Text {
            text: "  \n\t ".to_string(),
        }],
    ];
    for conteudo in vazios {
        assert_eq!(extract_text_opt(&conteudo), None, "veio: {conteudo:?}");
        assert_eq!(
            extract_text(&conteudo),
            "[no textual response provided by the model]",
            "o marcador nao pode mudar sem quebrar quem depende dele"
        );
    }

    let com_texto = vec![ContentBlock::Text {
        text: "oi".to_string(),
    }];
    assert_eq!(extract_text_opt(&com_texto).as_deref(), Some("oi"));
    assert_eq!(extract_text(&com_texto), "oi");
}

/// Provider que entrega texto na primeira volta e depois emudece.
///
/// Reproduz a regressao que a primeira versao do fix do #1048 introduziu:
/// as duas guardas de turno vazio (streaming e batch) discordavam, e a de
/// streaming devolvia `Err` mesmo com texto ja entregue ao sink.
///
/// Sequencia: (1) stream com texto + ferramenta; (2) stream vazio, que
/// gasta o unico redo; (3) batch com ferramenta, que devolve o loop ao
/// streaming; (4) stream vazio de novo, agora com o redo gasto.
pub(super) struct TextoDepoisVazio {
    pub(super) chamadas_stream: std::sync::atomic::AtomicUsize,
    /// Quantas vezes o turno caiu no caminho batch. Como este provider
    /// so vai para o batch por causa do redo, o contador **e** o numero
    /// de redos — e a unica forma direta de afirmar o limite de um por
    /// turno, em vez de depender de um efeito colateral.
    pub(super) chamadas_complete: std::sync::atomic::AtomicUsize,
}

impl TextoDepoisVazio {
    pub(super) fn novo() -> Self {
        Self {
            chamadas_stream: std::sync::atomic::AtomicUsize::new(0),
            chamadas_complete: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

#[async_trait::async_trait]
impl LlmProvider for TextoDepoisVazio {
    fn provider_id(&self) -> &str {
        "texto_depois_vazio"
    }

    /// Chamado uma vez, na volta 3: devolve ferramenta para o loop
    /// continuar ate a volta 4, que e a que importa.
    async fn complete(&self, _request: &LlmRequest) -> Result<LlmResponse> {
        let n = self
            .chamadas_complete
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(LlmResponse {
            // Input diferente a cada volta, de proposito: com input
            // repetido o `detectar_loop_ferramenta` cortaria o giro por
            // conta propria e mascararia a falta do limite de redo.
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
        let n = self
            .chamadas_stream
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let eventos: Vec<Result<StreamEvent>> = if n == 0 {
            vec![
                Ok(StreamEvent::TextDelta("PARTE-UM".to_string())),
                Ok(StreamEvent::ToolUseStart {
                    index: 0,
                    id: "t1".to_string(),
                    name: "eco".to_string(),
                }),
                Ok(StreamEvent::InputJsonDelta("{}".to_string())),
                Ok(StreamEvent::ContentBlockStop { index: 0 }),
                Ok(StreamEvent::MessageStop),
            ]
        } else {
            vec![Ok(StreamEvent::MessageStop)]
        };
        Ok(Box::pin(futures::stream::iter(eventos)))
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

/// #1048: texto ja entregue nunca vira erro.
///
/// Este e o teste que faltava na primeira versao do fix. A guarda de
/// streaming errava com o redo gasto sem olhar `full_response`, enquanto a
/// do batch olhava — as duas nasceram do mesmo commit e discordavam. Com o
/// `Err`, o Telegram edita a mensagem ja publicada para "Sorry, an error
/// occurred", apagando da tela a resposta que o usuario estava lendo, e
/// `remember_turn`/`record_turn_stats` sao pulados depois de as
/// ferramentas ja terem rodado.
#[tokio::test]
pub(super) async fn texto_ja_entregue_nao_vira_erro_quando_o_redo_acaba() {
    let runtime = AgentRuntime::new();
    let provider = std::sync::Arc::new(TextoDepoisVazio::novo());
    runtime.register_provider(provider.clone());
    runtime.register_tool(stub("eco"));

    let resposta = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        turno_de_streaming(&runtime, "sessao-1048-c"),
    )
    .await
    .expect("sem giro infinito")
    .expect("o turno tem de encerrar com o texto que ja foi ao sink, nao em Err");
    assert!(
        resposta.contains("PARTE-UM"),
        "a resposta ja entregue nao pode ser descartada; veio: {resposta:?}"
    );

    // E o turno tem de continuar anotado: o `Err` pulava isto.
    assert!(
        runtime.last_turn_stats("sessao-1048-c").is_some(),
        "o turno precisa ficar registrado no /stats"
    );

    // O limite de um redo por turno, afirmado direto em vez de deduzido.
    // Este provider so cai no batch por causa do redo, entao o contador
    // de `complete` E o numero de redos. Sem a trava `redo_ja_usado` o
    // turno alternaria streaming-vazio e batch-com-ferramenta ate estourar
    // o orcamento de ferramentas, e este assert acusa na primeira volta a
    // mais — sem depender do `detectar_loop_ferramenta`, que so cortaria
    // o giro se o input da ferramenta se repetisse.
    assert_eq!(
        provider
            .chamadas_complete
            .load(std::sync::atomic::Ordering::SeqCst),
        1,
        "o turno so pode refazer em batch uma vez"
    );
}

/// #1048: stream vazio refaz o turno em batch em vez de devolver "".
#[tokio::test]
pub(super) async fn stream_vazio_refaz_o_turno_em_batch() {
    let runtime = AgentRuntime::new();
    runtime.register_provider(std::sync::Arc::new(StreamVazio {
        batch: "resposta que o batch soube dar",
    }));

    let resposta = turno_de_streaming(&runtime, "sessao-1048-a")
        .await
        .expect("o redo em batch tem de completar o turno");
    assert_eq!(resposta, "resposta que o batch soube dar");
}

/// #1048: vazio nos dois caminhos vira erro, e nao bolha em branco.
///
/// O `timeout` aqui e cinto de seguranca, **nao** prova do limite de um
/// redo por turno: neste cenario quem encerra e a guarda de vazio do
/// proprio ramo batch, entao o teste passaria igual sem a trava. Quem
/// afirma o limite e `texto_ja_entregue_nao_vira_erro_quando_o_redo_acaba`,
/// contando as idas ao batch.
#[tokio::test]
pub(super) async fn vazio_no_streaming_e_no_batch_vira_erro() {
    let runtime = AgentRuntime::new();
    runtime.register_provider(std::sync::Arc::new(StreamVazio { batch: "" }));

    let erro = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        turno_de_streaming(&runtime, "sessao-1048-b"),
    )
    .await
    .expect("sem giro infinito: um redo so")
    .expect_err("turno vazio nos dois caminhos tem de virar erro");
    assert!(
        erro.to_string().contains("turno vazio"),
        "o erro precisa nomear a causa; veio: {erro}"
    );
}

/// Provider preso no mesmo golpe: toda volta devolve a MESMA chamada de
/// tool (nome + input identicos). Nao precisa de tool registrada — o
/// `registrar_chamada` acontece antes do portao, dentro da
/// `dispatch_tool_call`, entao a janela de 3 enche e o detector corta na
/// terceira. O nome e `file_read` porque o erro so mostra o campo
/// allow-listed da ferramenta (`path`, no caso), e o teste quer ver o
/// valor chegar ponta a ponta; tool sem caso no `summarize_tool_input`
/// mostra so as chaves, e isso e coberto no `execution_budget`.
pub(super) struct EmLoop;

#[async_trait::async_trait]
impl LlmProvider for EmLoop {
    fn provider_id(&self) -> &str {
        "em_loop"
    }

    async fn complete(&self, _request: &LlmRequest) -> Result<LlmResponse> {
        Ok(LlmResponse {
            content: vec![ContentBlock::ToolUse {
                id: "t-loop".to_string(),
                name: "file_read".to_string(),
                input: serde_json::json!({ "path": "/tmp/alvo-repetido" }),
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
        Ok(Box::pin(futures::stream::iter(vec![
            Ok(StreamEvent::ToolUseStart {
                index: 0,
                id: "t-loop".to_string(),
                name: "file_read".to_string(),
            }),
            Ok(StreamEvent::InputJsonDelta(
                "{\"path\":\"/tmp/alvo-repetido\"}".to_string(),
            )),
            Ok(StreamEvent::ContentBlockStop { index: 0 }),
            Ok(StreamEvent::MessageStop),
        ])))
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

/// #1295: o erro do detector de loop nao pode ser seco. Quem le o erro
/// e o humano no log, no ledger de runs ou no cartao da CLI — "tool loop
/// detected: file_read" nao diz quantas voltas deram nem O QUE estava
/// repetindo, e sem isso nao ha como corrigir. O diagnostico minimo:
/// nome da tool, a contagem da janela (3) e o campo allow-listed do input
/// repetido, redigido e truncado pelo mesmo `summarize_tool_input` da
/// #937. Passa pelo despacho unico do #1311: a mensagem nasce na
/// `dispatch_tool_call` e as copias do loop so a devolvem.
#[tokio::test]
pub(super) async fn loop_detectado_traz_input_repetido_no_erro() {
    let runtime = AgentRuntime::new();
    runtime.register_provider(std::sync::Arc::new(EmLoop));

    let erro = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        turno_de_streaming(&runtime, "sessao-1295-a"),
    )
    .await
    .expect("sem giro infinito: a janela de 3 corta")
    .expect_err("loop de tool identica tem de virar erro");
    let msg = erro.to_string();
    assert!(
        msg.contains("file_read"),
        "o erro precisa nomear a tool em loop; veio: {msg}"
    );
    assert!(
        msg.contains("3 chamadas"),
        "o erro precisa dizer a contagem da janela; veio: {msg}"
    );
    assert!(
        msg.contains("input repetido: /tmp/alvo-repetido"),
        "o erro precisa mostrar o campo allow-listed do input repetido; veio: {msg}"
    );
}
