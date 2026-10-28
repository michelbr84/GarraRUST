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
