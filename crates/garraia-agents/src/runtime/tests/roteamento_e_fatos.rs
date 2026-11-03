use super::*;

/// #1299: roteamento impossível não entra em retry/backoff — nem na
/// forma classificada, nem no erro cru, nem se o texto carregar um
/// número de status retryável no meio.
#[test]
pub(super) fn erro_de_roteamento_openrouter_nao_e_retryable() {
    let classificado = "agent error: O modelo 'deepseek/deepseek-v4-flash-20260731' \
         não pode ser servido por nenhum provider permitido pela preferência \
         `provider.only` em vigor — erro de configuração de roteamento \
         (não-transitório; não será retriado). No allowed providers are available";
    assert!(!is_retryable_error(&Error::Agent(classificado.into())));

    let cru = "openai API error: status=404 Not Found, \
               body=No allowed providers are available for the selected model.";
    assert!(!is_retryable_error(&Error::Agent(cru.into())));

    let com_status_retryavel = "openai API error: status=503, \
         body=No allowed providers are available for the selected model.";
    assert!(!is_retryable_error(&Error::Agent(
        com_status_retryavel.into()
    )));
}

/// #1792: com status HTTP de falha na mensagem, o STATUS decide o retry — e
/// nao uma substring solta. O corpo e texto do provider: um "4290 tokens", um
/// "upstream" ou um `status=503` citado no meio dele nao podem fazer o runtime
/// insistir num 400/404.
#[test]
pub(super) fn retry_decide_pelo_status_e_nao_por_substring_do_corpo() {
    let falsos_positivos = [
        // "429" solto dentro do numero de tokens.
        "openai API error: status=400 Bad Request, body=limite de 4290 tokens excedido",
        // "503" citado no texto do corpo, inclusive no formato `status=`.
        "erro na API da Anthropic: status=400 Bad Request, body=o proxy respondeu \
         status=503 antes",
        "openai API error: status=404 Not Found, body=modelo 503-preview nao existe",
        // "upstream" na frase do provider de um erro do pedido.
        "openai API error: status=422 Unprocessable Entity, body=upstream rejected the \
         schema",
        // "rate limit" num 401 nao vira retry: a chave e que esta errada.
        "openai API error: status=401 Unauthorized, body=rate limit tier requires a key",
        // 3xx e erro de configuracao (base_url), nunca transitorio.
        "llama.cpp error: status=308 Permanent Redirect, location=http://h/x?code=503; \
         redirects desligados por seguranca - corrija a base_url do provider",
    ];
    for msg in falsos_positivos {
        assert!(
            !is_retryable_error(&Error::Agent(msg.into())),
            "status de falha nao-retryavel tem de vencer o texto: {msg}"
        );
    }

    let por_status = [
        "openai API error: status=429 Too Many Requests, body=slow down",
        "openai API error: status=502 Bad Gateway, body=",
        "erro na API da Anthropic: status=503 Service Unavailable, body=",
        "erro na API da Anthropic: status=529 <unknown status code>, body=overloaded",
        // `error.code` HTTP de um erro entregue com 200 (#1726).
        "openai API error: status=429 (no corpo, com HTTP 200), body=rate limited",
    ];
    for msg in por_status {
        assert!(
            is_retryable_error(&Error::Agent(msg.into())),
            "status retryavel: {msg}"
        );
    }

    // Sem status (ou com 2xx, quando o erro veio no corpo de uma resposta
    // 200, como o SSE do OpenRouter): quem fala e a frase, e o texto e o
    // fallback.
    let por_texto = [
        "openai API error (SSE): status=200 OK, error=Rate limit exceeded",
        "openai API error (SSE): status=200 OK, error=upstream connect error",
        "openai-codex API error: body=slow down (code=rate_limit_exceeded)",
        "ollama error status: 429 Too Many Requests",
    ];
    for msg in por_texto {
        assert!(
            is_retryable_error(&Error::Agent(msg.into())),
            "sem status de falha, o texto decide: {msg}"
        );
    }

    // Sem status, "429" solto tambem nao basta.
    assert!(!is_retryable_error(&Error::Agent(
        "failed to parse openai response: 4290 tokens".into()
    )));
}

/// #1540: duas entradas `llm:` do mesmo tipo (`provider: openai` em bases
/// distintas) precisam de ids proprios (a chave do perfil) — sem isso o id
/// era o tipo (`"openai"`), `get_provider` so achava a primeira registrada e
/// a ordem de registro vinha da iteracao do `HashMap` de config: o default
/// efetivo era aleatorio a cada boot.
#[test]
pub(super) fn perfis_do_mesmo_tipo_sao_enderecaveis_pela_chave() {
    let rt = AgentRuntime::new();
    rt.register_provider(Arc::new(
        crate::openai::OpenAiProvider::new(
            "k".to_string(),
            Some("glm53-flash".to_string()),
            Some("http://127.0.0.1:8080/v1".to_string()),
        )
        .with_name("glm53"),
    ));
    rt.register_provider(Arc::new(
        crate::ollama::OllamaProvider::new(
            Some("hf.co/unsloth/Qwen3.8-27B-GGUF:UD-Q8_K_XL".to_string()),
            Some("http://127.0.0.1:11434".to_string()),
        )
        .with_name("ollama-qwen3"),
    ));

    assert_eq!(rt.get_provider("glm53").unwrap().provider_id(), "glm53");
    assert_eq!(
        rt.get_provider("ollama-qwen3").unwrap().provider_id(),
        "ollama-qwen3"
    );
    let ids = rt.provider_ids();
    assert!(ids.contains(&"glm53".to_string()));
    assert!(ids.contains(&"ollama-qwen3".to_string()));

    // O default promovido pelo primeiro registro resolve; e o
    // `agent.default_provider` (chave do perfil, #1180) aponta para a
    // instancia certa, nao para "a primeira do tipo".
    assert!(rt.default_provider().is_some());
    assert!(rt.set_default_provider_id("ollama-qwen3"));
    assert_eq!(rt.default_provider().unwrap().provider_id(), "ollama-qwen3");
}

pub(super) fn fato(tipo: &str, key: &str, value: &str, confidence: f32) -> StructuredFact {
    StructuredFact {
        fact_type: tipo.into(),
        key: key.into(),
        value: value.into(),
        confidence,
    }
}

#[test]
pub(super) fn select_facts_mantem_somente_validos() {
    let out = select_learned_facts(
        vec![
            fato("preference", "food", "sushi", 0.95),
            // confidence abaixo do gate
            fato("preference", "cor", "azul", 0.5),
            // key vazia
            fato("preference", "  ", "algo", 0.9),
            // value vazia
            fato("preference", "cidade", " ", 0.9),
        ],
        None,
    );
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].key, "food");
}

#[test]
pub(super) fn select_facts_respeita_teto_por_confidence() {
    let out = select_learned_facts(
        vec![
            fato("a", "baixa", "1", 0.81),
            fato("a", "alta", "2", 0.99),
            fato("a", "media", "3", 0.9),
        ],
        Some(2),
    );
    assert_eq!(out.len(), 2);
    // Os dois de maior confidence sobrevivem.
    assert_eq!(out[0].key, "alta");
    assert_eq!(out[1].key, "media");
}

#[test]
pub(super) fn select_facts_sem_teto_preserva_ordem_de_chegada() {
    let out = select_learned_facts(
        vec![fato("a", "um", "1", 0.9), fato("a", "dois", "2", 0.95)],
        None,
    );
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].key, "um");
    assert_eq!(out[1].key, "dois");
}

/// #1541 (defeito 2, o resto depois da #1540): modelo HuggingFace servido
/// pelo Ollama nao pode ser mandado ao openrouter.
///
/// `hf.co/unsloth/Qwen3.8-27B-GGUF:UD-Q8_K_XL` tem prefixo nao registrado e
/// uma barra a mais — id do openrouter e sempre `vendor/modelo`. Mandar assim
/// e 400 garantido; cair no provider default e estritamente melhor.
#[test]
pub(super) fn so_uma_barra_pode_ser_id_do_openrouter() {
    // Ids reais do openrouter continuam passando.
    for real in [
        "openrouter/auto",
        "z-ai/glm-5.3-flash",
        "meta-llama/llama-3.1-70b-instruct:free",
        "minimax/minimax-01",
    ] {
        assert!(pode_ser_id_do_openrouter(real), "{real} e id de openrouter");
    }

    // Duas barras nunca formam id do openrouter.
    assert!(!pode_ser_id_do_openrouter(
        "hf.co/unsloth/Qwen3.8-27B-GGUF:UD-Q8_K_XL"
    ));
    assert!(!pode_ser_id_do_openrouter("a/b/c"));

    // Sem barra nao entra neste caminho de qualquer forma.
    assert!(!pode_ser_id_do_openrouter("glm53-flash"));

    // #1752: um preset bem formado passa mesmo com duas barras — a regra de
    // uma barra so nao se aplica a ele. `a/b/c` e `hf.co/...` continuam falsos
    // (nao sao presets).
    assert!(pode_ser_id_do_openrouter("z-ai/glm-5.3-flash@preset/foo"));
    assert!(pode_ser_id_do_openrouter("@preset/foo"));

    // #1764: uma variante de roteamento (`:floor`) nao muda a contagem de
    // barras — `vendor/modelo:floor` segue sendo id de uma barra, roteado ao
    // OpenRouter como qualquer outro id (o `/model` e quem valida a variante).
    for variante in [
        "z-ai/glm-5.3-flash:floor",
        "vendor/x:nitro",
        "meta-llama/x:online",
        "a/b:exacto",
    ] {
        assert!(pode_ser_id_do_openrouter(variante), "{variante}");
    }
}

/// #1752: um preset do OpenRouter (`@preset/<slug>`) e sempre roteado para o
/// OpenRouter, nas duas formas e inclusive com um prefixo de provider nativo.
#[test]
pub(super) fn preset_do_openrouter_roteia_para_o_openrouter() {
    for preset in [
        "@preset/z-ai-glm-5-3-flash",
        "z-ai/glm-5.3-flash@preset/z-ai-glm-5-3-flash",
        // Prefixo `openai` nativo nao importa: o preset so existe no OpenRouter.
        "openai/gpt-4o@preset/x",
    ] {
        assert_eq!(
            resolve_provider_from_model(preset).as_deref(),
            Some("openrouter"),
            "{preset} deve ir ao OpenRouter"
        );
        assert!(pode_ser_id_do_openrouter(preset), "{preset}");
    }
    assert!(pode_ser_id_do_openrouter("z-ai/x@preset/y"));

    // Base de duas barras nao e um preset: cai no prefixo normal (`a`), e nao
    // pode ser id do OpenRouter (tres barras).
    assert_eq!(
        resolve_provider_from_model("a/b/c@preset/y").as_deref(),
        Some("a")
    );
    assert!(!pode_ser_id_do_openrouter("a/b/c@preset/y"));
}
