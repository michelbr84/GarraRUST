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
}
