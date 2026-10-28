use super::*;

/// Primario cuja credencial e recusada (401): o runtime cai no fallback.
struct ProviderComChaveMorta;

#[async_trait::async_trait]
impl LlmProvider for ProviderComChaveMorta {
    fn provider_id(&self) -> &str {
        "primario"
    }
    fn configured_model(&self) -> Option<&str> {
        Some("primario-1")
    }
    async fn complete(&self, _request: &LlmRequest) -> garraia_common::Result<LlmResponse> {
        Err(garraia_common::Error::Agent(
            "openai API error: status=401 Unauthorized, body=User not found.".to_string(),
        ))
    }
    async fn health_check(&self) -> garraia_common::Result<bool> {
        Ok(true)
    }
}

/// #1733: quando o primario falha e o fallback responde, o corpo e os
/// headers do turno dizem quem respondeu de fato — nao o default.
#[tokio::test]
async fn resposta_do_turno_nomeia_o_fallback_que_respondeu() {
    let dir = tempfile::tempdir().expect("tempdir");
    let st = processo(dir.path(), Some(Arc::new(ProviderQueAnota::default())));
    st.agents.register_provider(Arc::new(ProviderComChaveMorta));
    st.agents.set_default_provider_id("primario");
    st.agents.set_fallback_providers(vec!["anota".to_string()]);
    let sid = criar_sessao_rest(&st).await;

    let resp = send_message(
        State(Arc::clone(&st)),
        Path(sid),
        Json(SendMessageRequest {
            content: "oi".to_string(),
            agent_id: None,
            model: None,
        }),
    )
    .await
    .into_response();
    assert_eq!(resp.status(), StatusCode::OK);
    let h = |n: &str| {
        resp.headers()
            .get(n)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    assert_eq!(h("X-AI-Provider").as_deref(), Some("anota"));
    assert_eq!(h("X-AI-Model").as_deref(), Some("anota-1"));
    let (_, corpo) = em_json(resp).await;
    assert_eq!(corpo["provider"], "anota", "{corpo}");
    assert_eq!(corpo["model"], "anota-1", "{corpo}");
    assert_eq!(corpo["fallback"], true, "{corpo}");
}

/// #1743: primario cuja chave pode ser trocada no meio do teste — 401 ate
/// `aceitar()`, e depois responde normalmente.
struct ProviderComChaveTrocavel {
    valida: std::sync::atomic::AtomicBool,
}

impl ProviderComChaveTrocavel {
    fn com_chave_morta() -> Self {
        Self {
            valida: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn aceitar(&self) {
        self.valida.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

#[async_trait::async_trait]
impl LlmProvider for ProviderComChaveTrocavel {
    fn provider_id(&self) -> &str {
        "primario"
    }
    fn configured_model(&self) -> Option<&str> {
        Some("primario-1")
    }
    async fn complete(&self, _request: &LlmRequest) -> garraia_common::Result<LlmResponse> {
        if !self.valida.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(garraia_common::Error::Agent(
                "openai API error: status=401 Unauthorized, body=User not found.".to_string(),
            ));
        }
        Ok(LlmResponse {
            content: vec![ContentBlock::Text {
                text: "resposta-da-nuvem".to_string(),
            }],
            model: "primario-1".to_string(),
            usage: None,
            stop_reason: Some("end_turn".to_string()),
        })
    }
    async fn health_check(&self) -> garraia_common::Result<bool> {
        Ok(true)
    }
}

/// #1743, os dois criterios de aceitacao: o turno recebe 401 do primario e o
/// `/api/health` passa a acusar o check do provider com `ok: false` e o motivo;
/// um turno que passa depois (chave trocada) devolve o check ao normal.
///
/// A config de teste nao tem `llm:`, entao nenhum check e sondado — o que
/// aparece vem so da recusa observada, e a volta ao normal e o check sumir.
#[tokio::test]
async fn health_acusa_a_credencial_recusada_e_volta_ao_normal_quando_um_turno_passa() {
    let dir = tempfile::tempdir().expect("tempdir");
    let st = processo(dir.path(), Some(Arc::new(ProviderQueAnota::default())));
    let primario = Arc::new(ProviderComChaveTrocavel::com_chave_morta());
    st.agents.register_provider(primario.clone());
    st.agents.set_default_provider_id("primario");
    st.agents.set_fallback_providers(vec!["anota".to_string()]);
    let sid = criar_sessao_rest(&st).await;

    let antes = crate::health::health_handler(State(Arc::clone(&st)))
        .await
        .0;
    assert!(
        antes.checks.iter().all(|c| c.name != "primario"),
        "antes de qualquer turno nao ha o que acusar: {:?}",
        antes.checks
    );
    assert_eq!(antes.status, "healthy");

    // O turno cai no fallback; o health passa a dizer por que.
    let (status, corpo) = mandar(&st, &sid, "oi").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(corpo["fallback"], true, "{corpo}");

    let durante = crate::health::health_handler(State(Arc::clone(&st)))
        .await
        .0;
    let check = durante
        .checks
        .iter()
        .find(|c| c.name == "primario")
        .expect("o check do primario aparece com a recusa");
    assert!(!check.ok);
    let erro = check.error.as_deref().unwrap_or_default();
    assert!(
        erro.starts_with("credencial recusada (401) em 20"),
        "motivo com status e hora: {erro}"
    );
    assert!(erro.ends_with('Z'), "hora em UTC com Z: {erro}");
    assert!(
        !erro.contains("User not found"),
        "o corpo do 401 nao entra no health, que e auth-free: {erro}"
    );
    assert_ne!(durante.status, "healthy");
    assert!(
        durante
            .warnings
            .iter()
            .any(|w| w.starts_with("primario: credencial recusada (401)")),
        "{:?}",
        durante.warnings
    );

    // Chave trocada: o turno passa no primario e a recusa expira.
    primario.aceitar();
    let (status, corpo) = mandar(&st, &sid, "oi de novo").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(corpo["provider"], "primario", "{corpo}");
    assert_eq!(corpo["fallback"], false, "{corpo}");

    let depois = crate::health::health_handler(State(Arc::clone(&st)))
        .await
        .0;
    assert!(
        depois.checks.iter().all(|c| c.name != "primario"),
        "turno que passou expirou a recusa: {:?}",
        depois.checks
    );
    assert_eq!(depois.status, "healthy");
}
