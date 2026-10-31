//! #1776: 200 vazio nao e sucesso — entra no circuito de retry/fallback.
//!
//! Molde: primario controlado pelo teste + o Ollama local no fallback, como
//! no wizard. O defeito e sobre **conteudo** (resposta 200 sem texto e sem
//! ferramenta), nao sobre transporte — por isso o mock vive aqui e nao em
//! `portao_e_fixtures`, onde moram as falhas simuladas do #1249.

use super::fallback_local::{FalhaSimulada, LocalQueResponde, RESPOSTA_LOCAL, pedido_simples};
use super::*;

/// Consumo fixo de cada turno 200-vazio que o mock devolve.
const USO_DO_TURNO_VAZIO: (u32, u32) = (7, 3);

/// Provider que devolve 200 vazio nas N primeiras chamadas e depois responde
/// texto; com N gigante, nunca responde. Com `sem_usage`, as vazias vem sem
/// `usage` — o provider que nem contabiliza tokens (o cenario real de alguns
/// provedores de baixa qualidade).
struct VazioDepoisTexto {
    id: &'static str,
    vazias_restantes: std::sync::atomic::AtomicUsize,
    chamadas_complete: std::sync::atomic::AtomicUsize,
    sem_usage: bool,
}

impl VazioDepoisTexto {
    fn novo(id: &'static str, vazias: usize) -> Self {
        Self::variante(id, vazias, false)
    }

    fn sem_usage(id: &'static str, vazias: usize) -> Self {
        Self::variante(id, vazias, true)
    }

    fn variante(id: &'static str, vazias: usize, sem_usage: bool) -> Self {
        Self {
            id,
            vazias_restantes: std::sync::atomic::AtomicUsize::new(vazias),
            chamadas_complete: std::sync::atomic::AtomicUsize::new(0),
            sem_usage,
        }
    }

    fn completes(&self) -> usize {
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
        // `fetch_sub` devolve o valor ANTERIOR: race-free sem load+store em
        // duas ordenacoes (revisao do #1777). O circuito nunca chama mais que
        // vazias + 1: quando o valor anterior chega a 0 (agota as vazias), o
        // texto e servido e o wrap do contador (usize::MAX) fica inacessivel.
        let restantes = self
            .vazias_restantes
            .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        if restantes > 0 {
            let usage = if self.sem_usage {
                None
            } else {
                Some(Usage {
                    input_tokens: USO_DO_TURNO_VAZIO.0,
                    output_tokens: USO_DO_TURNO_VAZIO.1,
                })
            };
            return Ok(LlmResponse {
                content: Vec::new(),
                model: "gpt-4o".to_string(),
                stop_reason: None,
                usage,
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

/// Fallback local que erra de verdade (transporte) — para o turno terminar
/// "so de vazias no primario e erro no resto", que volta `Ok` com a ultima
/// vazia, nao `Err`.
struct LocalQueErra;

#[async_trait::async_trait]
impl LlmProvider for LocalQueErra {
    fn provider_id(&self) -> &str {
        "ollama"
    }

    async fn complete(&self, _request: &LlmRequest) -> Result<LlmResponse> {
        Err(FalhaSimulada::Transporte.erro())
    }

    async fn stream_complete(
        &self,
        _request: &LlmRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>> {
        Err(FalhaSimulada::Transporte.erro())
    }

    async fn health_check(&self) -> Result<bool> {
        Ok(true)
    }
}

/// Runtime com o primario dado e o Ollama local no fallback - o cenario do
/// wizard, so que com o primario controlado pelo teste.
fn runtime_com_primario_e_fallback_local(
    primario: std::sync::Arc<dyn LlmProvider>,
) -> AgentRuntime {
    let runtime = AgentRuntime::new();
    runtime.register_provider(primario);
    runtime.register_provider(std::sync::Arc::new(LocalQueResponde));
    runtime.set_fallback_providers(vec!["ollama".to_string()]);
    runtime
}

/// #1776, caminho batch: o primeiro 200 vazio re-tenta no proprio primario e
/// a segunda tentativa responde - o fallback local nem e consultado. O gasto
/// da vazia descartada volta em `uso_descartado`; o `usage` da resposta e o
/// dela propria, porque o medidor de contexto do shim `/v1/messages` so pode
/// ver quem de fato serviu.
#[tokio::test(start_paused = true)]
async fn ok_vazio_no_primario_reintenta_e_responde_sem_fallback() {
    let primario = std::sync::Arc::new(VazioDepoisTexto::novo("openai", 1));
    let runtime = runtime_com_primario_e_fallback_local(primario.clone());
    let como_dyn: std::sync::Arc<dyn LlmProvider> = primario.clone();

    let RespostaDoCircuito {
        resp,
        provider: quem,
        uso_descartado,
    } = runtime
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
    let uso = resp.usage.expect("a resposta final traz o usage dela");
    assert_eq!(
        (uso.input_tokens, uso.output_tokens),
        (11, 5),
        "o usage da resposta nao embute a vazia descartada"
    );
    assert_eq!(
        uso_descartado.map(|u| (u.input_tokens, u.output_tokens)),
        Some(USO_DO_TURNO_VAZIO),
        "o gasto da vazia nao desaparece - vai no descartado"
    );
}

/// #1776: o primario so devolve 200 vazio. Um vazio re-tenta UMA vez; o
/// segundo vazio seguido e deterministico o bastante para nao queimar o
/// orcamento de retry inteiro (precedente do #1249) - o turno vai para o
/// fallback local, que responde. E um unico turno de vazias grava UMA falha
/// no breaker (nao uma por tentativa) sem abrir o circuito.
#[tokio::test(start_paused = true)]
async fn ok_vazio_esgota_o_orcamento_e_cai_para_o_fallback_local() {
    let primario = std::sync::Arc::new(VazioDepoisTexto::novo("openai", usize::MAX));
    let runtime = runtime_com_primario_e_fallback_local(primario.clone());
    let como_dyn: std::sync::Arc<dyn LlmProvider> = primario.clone();

    let RespostaDoCircuito {
        resp,
        provider: quem,
        uso_descartado,
    } = runtime
        .complete_reportando_provider(&como_dyn, &pedido_simples())
        .await
        .expect("o fallback local responde depois do vazio seguido");

    assert_eq!(
        quem, "ollama",
        "o vazio do primario empurra o turno ao fallback"
    );
    assert_eq!(
        primario.completes(),
        2,
        "1 vazia re-tentada + 1 vazia que decide o break; orcamento inteiro preservado"
    );
    assert_eq!(
        resp.content.last().and_then(|b| match b {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        }),
        Some(RESPOSTA_LOCAL)
    );
    assert!(
        resp.usage.is_none(),
        "o local responde sem usage: nada e fabricado para a resposta"
    );
    assert_eq!(
        uso_descartado.map(|u| (u.input_tokens, u.output_tokens)),
        Some((USO_DO_TURNO_VAZIO.0 * 2, USO_DO_TURNO_VAZIO.1 * 2)),
        "o gasto das duas vazias vai no descartado"
    );
    // O breaker do primario recebe UMA falha pelo turno inteiro de vazias —
    // travado pela CONTAGEM, nao so pelo estado (duas tentativas vazias nao
    // valem duas falhas). Limiar 5 em producao: um turno ruim nao abre.
    let breaker = runtime.resilience.circuit_breaker("openai").await;
    assert!(
        breaker.allow_request().await,
        "o breaker do primario nao abre por um unico turno vazio"
    );
    assert_eq!(
        breaker.failures().await,
        1,
        "UMA falha no breaker por turno de vazias, nao uma por tentativa"
    );
}

/// #1776: primario e fallback so devolvem 200 vazio. Nada de erro: a ultima
/// resposta vazia volta vazia de verdade, com o `usage` **intocado** de quem
/// a produziu (o medidor de contexto do shim ve o valor real da ultima
/// tentativa, nao um numero fabricado nem uma soma), marcada com quem serviu;
/// o gasto das vazias ANTERIORES a devolvida vai no descartado. A soma das
/// duas partes e o total de todas as tentativas — cada uma, exatamente uma
/// vez.
#[tokio::test(start_paused = true)]
async fn ok_vazio_em_todos_volta_vazio_com_o_uso_de_todas_as_tentativas() {
    let runtime = AgentRuntime::new();
    let primario = std::sync::Arc::new(VazioDepoisTexto::novo("openai", usize::MAX));
    let fallback = std::sync::Arc::new(VazioDepoisTexto::novo("ollama", usize::MAX));
    runtime.register_provider(primario.clone());
    runtime.register_provider(fallback.clone());
    runtime.set_fallback_providers(vec!["ollama".to_string()]);
    let como_dyn: std::sync::Arc<dyn LlmProvider> = primario.clone();

    let RespostaDoCircuito {
        resp,
        provider: quem,
        uso_descartado,
    } = runtime
        .complete_reportando_provider(&como_dyn, &pedido_simples())
        .await
        .expect("a resposta vazia de alguem volta - nunca erro");

    assert_eq!(quem, "ollama", "a ultima vazia e a do fallback");
    assert!(
        resp.content.is_empty(),
        "o turno volta vazio, como hoje, para o guarda do chamador"
    );
    assert_eq!(
        primario.completes(),
        2,
        "1 re-tentativa no primario, nada mais"
    );
    assert_eq!(fallback.completes(), 1, "o fallback recebe a vez");
    let uso = resp.usage.expect("o usage da devolvida e o de origem");
    assert_eq!(
        (uso.input_tokens, uso.output_tokens),
        USO_DO_TURNO_VAZIO,
        "o `usage` da resposta volta INTOCATO - o de quem a produziu"
    );
    assert_eq!(
        uso_descartado.map(|u| (u.input_tokens, u.output_tokens)),
        Some((USO_DO_TURNO_VAZIO.0 * 2, USO_DO_TURNO_VAZIO.1 * 2)),
        "so as DUAS vazias do primario estao no descartado - a devolvida nao"
    );
}

/// #1776: primario sempre vazio, o fallback erra (transporte). O turno volta
/// `Ok` com a ULTIMA VAZIA DO PRIMARIO - o erro do fallback nao apaga a
/// resposta que ja existia - marcada com o primario, o `usage` dela intocado
/// e o gasto das vazias descartadas em `uso_descartado`.
#[tokio::test(start_paused = true)]
async fn vazio_no_primario_com_erro_no_fallback_volta_a_ultima_vazia() {
    let runtime = AgentRuntime::new();
    let primario = std::sync::Arc::new(VazioDepoisTexto::novo("openai", usize::MAX));
    runtime.register_provider(primario.clone());
    runtime.register_provider(std::sync::Arc::new(LocalQueErra));
    runtime.set_fallback_providers(vec!["ollama".to_string()]);
    let como_dyn: std::sync::Arc<dyn LlmProvider> = primario.clone();

    let RespostaDoCircuito {
        resp,
        provider: quem,
        uso_descartado,
    } = runtime
        .complete_reportando_provider(&como_dyn, &pedido_simples())
        .await
        .expect("a ultima vazia volta - o erro do fallback nao apaga a resposta");

    assert_eq!(
        quem, "openai",
        "quem de fato produziu a resposta devolvida foi o primario"
    );
    assert!(resp.content.is_empty());
    assert_eq!(primario.completes(), 2);
    let uso = resp.usage.expect("o usage da vazia devolvida e intocado");
    assert_eq!(
        (uso.input_tokens, uso.output_tokens),
        USO_DO_TURNO_VAZIO,
        "o `usage` da vazia devolvida e o de origem, sem fabricacao"
    );
    assert_eq!(
        uso_descartado.map(|u| (u.input_tokens, u.output_tokens)),
        Some((USO_DO_TURNO_VAZIO.0, USO_DO_TURNO_VAZIO.1)),
        "uma vazia re-tentada descartada (a devolvida nao esta dentro)"
    );
}

/// #1776: vazias sem `usage` (provider que nem contabiliza). O descarte
/// aconteceu e tem de ser visivel: `uso_descartado` fica `Some(0, 0)` -
/// nada foi contado, mas houve tentativa queimada - e a resposta devolvida
/// segue com `usage` `None`, sem numero fabricado.
#[tokio::test(start_paused = true)]
async fn vazio_sem_usage_fica_some_zero_zero_no_descartado() {
    let primario = std::sync::Arc::new(VazioDepoisTexto::sem_usage("openai", usize::MAX));
    let runtime = runtime_com_primario_e_fallback_local(primario.clone());
    let como_dyn: std::sync::Arc<dyn LlmProvider> = primario.clone();

    let RespostaDoCircuito {
        resp,
        provider: quem,
        uso_descartado,
    } = runtime
        .complete_reportando_provider(&como_dyn, &pedido_simples())
        .await
        .expect("o fallback local responde");

    assert_eq!(quem, "ollama");
    assert!(resp.usage.is_none(), "nada e fabricado para a resposta");
    assert_eq!(
        uso_descartado.map(|u| (u.input_tokens, u.output_tokens)),
        Some((0, 0)),
        "Some(0, 0) = houve descarte e o provider nao contou tokens"
    );
}

/// #1776: `registrar_no_turno` — a flag de contabilidade conhecida sobe SO
/// com o `usage` da resposta servida. O total descartado sozinho nao prova
/// contabilidade conhecida (nao inclui a resposta devolvida): serviu sem
/// `usage` com descarte em maos, o gasto entra no turno e a flag fica
/// `false`.
#[test]
fn registrar_no_turno_flag_de_contabilidade_so_com_o_usage_da_resposta() {
    let uso_resposta = Usage {
        input_tokens: 11,
        output_tokens: 5,
    };
    let uso_descartado = Some(Usage {
        input_tokens: 7,
        output_tokens: 3,
    });

    // (a) Resposta com `usage` + descartado: tudo entra, flag sobe.
    let mut turno = crate::turn_stats::TurnStats::default();
    let resposta = LlmResponse {
        content: Vec::new(),
        model: "gpt-4o".to_string(),
        stop_reason: None,
        usage: Some(uso_resposta.clone()),
    };
    registrar_no_turno(&mut turno, &resposta, uso_descartado.as_ref());
    assert_eq!(turno.input_tokens, 18);
    assert_eq!(turno.output_tokens, 8);
    assert!(turno.tokens_conhecidos);

    // (b) Resposta SEM `usage` + descartado: o gasto entra, a flag NAO sobe.
    let mut turno = crate::turn_stats::TurnStats::default();
    let resposta = LlmResponse {
        content: Vec::new(),
        model: "gpt-4o".to_string(),
        stop_reason: None,
        usage: None,
    };
    registrar_no_turno(&mut turno, &resposta, uso_descartado.as_ref());
    assert_eq!(
        (turno.input_tokens, turno.output_tokens),
        (7, 3),
        "o gasto queimado entra na contagem do turno"
    );
    assert!(
        !turno.tokens_conhecidos,
        "o descartado sozinho nao prova contabilidade conhecida"
    );

    // (c) Nada descartado: so o `usage` da resposta.
    let mut turno = crate::turn_stats::TurnStats::default();
    let resposta = LlmResponse {
        content: Vec::new(),
        model: "gpt-4o".to_string(),
        stop_reason: None,
        usage: Some(uso_resposta.clone()),
    };
    registrar_no_turno(&mut turno, &resposta, None);
    assert_eq!((turno.input_tokens, turno.output_tokens), (11, 5));
    assert!(turno.tokens_conhecidos);
}

/// #1776: turno so-de-ferramenta (o modelo pediu a execucao) e saudavel -
/// nada de retry, nada de fallback, o bloco volta intacto de primeira.
#[tokio::test(start_paused = true)]
async fn turno_so_de_ferramenta_nao_e_vazio() {
    struct SoFerramenta {
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

    let RespostaDoCircuito {
        resp,
        provider: quem,
        uso_descartado,
    } = runtime
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
    let uso = resp.usage.expect("o turno saudavel traz o usage dele");
    assert_eq!((uso.input_tokens, uso.output_tokens), (9, 4));
    assert!(
        uso_descartado.is_none(),
        "nenhuma vazia descartada: o descartado e None, nao Some(0, 0)"
    );
}
