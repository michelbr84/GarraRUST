//! `GET /v1/models`.
//!
//! Vive fora do `openai_api` porque a regra que decide *quais* modelos o
//! gateway serve (#1029) e uma responsabilidade propria: ela espelha o
//! roteamento que o `POST /v1/chat/completions` aplica ao campo `model`, e
//! um teste dela nao precisa de nada do caminho de chat.

use axum::{
    body::Body,
    extract::State,
    http::Response,
    response::{IntoResponse, Json},
};

use crate::state::SharedState;

/// GET /v1/models — os modelos que `POST /v1/chat/completions` de fato serve.
///
/// Era uma lista fixa (`gpt-4`, `claude-3-opus`, …) que nao lia a config: um
/// cliente OpenAI-compatible listava, escolhia um e recebia 500 do provider
/// real (#1029). Agora a lista sai dos providers registrados, filtrada pela
/// mesma regra de roteamento que o chat aplica ao campo `model` — ver
/// [`servable_models`]. `owned_by` e o id do provider no GarraIA, nao o
/// fabricante, porque e o que diz a quem chama por onde a requisicao vai.
pub async fn list_models(State(state): State<SharedState>) -> Response<Body> {
    let default = state.agents.default_provider_id();
    let mut providers = state.agents.provider_ids();
    providers.sort();
    providers.dedup();
    let configured: Vec<(String, String)> = providers
        .iter()
        .filter_map(|pid| {
            state
                .agents
                .get_provider(pid)
                .and_then(|p| p.configured_model().map(|m| (pid.clone(), m.to_string())))
        })
        .collect();

    // OpenAI manda a data de criacao do modelo; o gateway nao a conhece e
    // inventar uma seria mentir. `0` e o que clientes tratam como "sem data".
    let data: Vec<serde_json::Value> = servable_models(default.as_deref(), &configured)
        .into_iter()
        .map(|(id, owned_by)| {
            serde_json::json!({
                "id": id,
                "object": "model",
                "created": 0,
                "owned_by": owned_by,
            })
        })
        .collect();

    Json(serde_json::json!({ "object": "list", "data": data })).into_response()
}

/// Quais modelos configurados o `POST /v1/chat/completions` roteia certo, e
/// para quem. Pura, para a regra ser afirmavel sem subir provider nenhum.
///
/// O chat entrega `model` ao runtime como `model_override`, que roteia pelo
/// prefixo (`openrouter/auto` → `openrouter`; prefixo de provider nao
/// registrado com `/` cai no openrouter) e, sem prefixo, no provider default —
/// e passa a string ao provider **sem tirar o prefixo**. Entao um id so
/// funciona quando e o modelo configurado do provider default, ou quando o
/// proprio prefixo dele ja resolve para o provider que o configurou. Um nome
/// sem prefixo num provider que nao e o default nao tem id que chegue nele;
/// lista-lo repetiria o bug que isto substitui (modelo na lista, 500 no uso).
///
/// `configured` e `(provider_id, modelo configurado)`; o conjunto de ids
/// registrados e deduzido dele. O default vem primeiro, porque e o que o
/// cliente recebe quando nao manda `model` nenhum.
pub(crate) fn servable_models(
    default_provider: Option<&str>,
    configured: &[(String, String)],
) -> Vec<(String, String)> {
    let registered: Vec<&str> = configured.iter().map(|(pid, _)| pid.as_str()).collect();
    let is_default = |pid: &str| Some(pid) == default_provider;

    let mut out: Vec<(String, String)> = Vec::new();
    let ordered = configured
        .iter()
        .filter(|(pid, _)| is_default(pid))
        .chain(configured.iter().filter(|(pid, _)| !is_default(pid)));

    for (pid, model) in ordered {
        let by_prefix = garraia_agents::resolve_provider_from_model(model);
        let routes_here = match by_prefix.as_deref() {
            // O prefixo aponta para este provider: chega nele seja ele default ou nao.
            Some(p) if p == pid => true,
            // O prefixo aponta para outro provider registrado: iria para la,
            // com um nome que aquele provider nao conhece.
            Some(p) if registered.contains(&p) => false,
            // Prefixo de provider nao registrado: o runtime tenta o openrouter
            // (nomes `org/model` sao dele) e so entao o default.
            Some(_) => {
                pid == "openrouter" || (is_default(pid) && !registered.contains(&"openrouter"))
            }
            // Sem prefixo: vai para o default, e so para ele.
            None => is_default(pid),
        };
        if !routes_here || out.iter().any(|(id, _)| id == model) {
            continue;
        }
        out.push((model.clone(), pid.clone()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(p, m)| (p.to_string(), m.to_string()))
            .collect()
    }

    /// #1029: so entra na lista o que o `model` do chat consegue rotear.
    #[test]
    fn servable_models_follows_the_chat_routing_rule() {
        // O caso da issue: default openrouter com `openrouter/auto`, e um
        // ollama secundario com nome sem prefixo — que nenhum id alcanca,
        // porque sem prefixo o runtime vai para o default.
        let lista = servable_models(
            Some("openrouter"),
            &cfg(&[("ollama", "qwen3:8b"), ("openrouter", "openrouter/auto")]),
        );
        // A saida e `(id, owned_by)` — o inverso da entrada `(provider, modelo)`.
        assert_eq!(lista, cfg(&[("openrouter/auto", "openrouter")]));

        // Default sem prefixo entra; secundario com prefixo proprio tambem,
        // porque o prefixo o leva ao provider certo. Default primeiro.
        let lista = servable_models(
            Some("ollama"),
            &cfg(&[
                ("anthropic", "anthropic/claude-3-7-sonnet"),
                ("ollama", "qwen3:8b"),
            ]),
        );
        assert_eq!(
            lista,
            cfg(&[
                ("qwen3:8b", "ollama"),
                ("anthropic/claude-3-7-sonnet", "anthropic"),
            ])
        );

        // Prefixo que aponta para OUTRO provider registrado: iria para la com
        // um nome que ele nao conhece — fora, mesmo sendo o default.
        let lista = servable_models(
            Some("ollama"),
            &cfg(&[("ollama", "openai/gpt-4o"), ("openai", "gpt-4o")]),
        );
        // ... e o `gpt-4o` sem prefixo do openai iria para o default (ollama).
        assert!(lista.is_empty(), "{lista:?}");

        // Prefixo de provider nao registrado num default que nao e openrouter e
        // sem openrouter na casa: cai no default, entao serve.
        let lista = servable_models(Some("ollama"), &cfg(&[("ollama", "qwen/qwen3-8b")]));
        assert_eq!(lista, cfg(&[("qwen/qwen3-8b", "ollama")]));

        // O mesmo modelo em dois providers aparece uma vez, com o primeiro dono.
        let lista = servable_models(
            Some("openai"),
            &cfg(&[("openai", "openai/gpt-4o"), ("openrouter", "openai/gpt-4o")]),
        );
        assert_eq!(lista, cfg(&[("openai/gpt-4o", "openai")]));

        // Sem provider nenhum, lista vazia — nao a lista fixa de antes.
        assert!(servable_models(None, &[]).is_empty());
    }

    /// Provider de mentira so com o que o `/v1/models` le: id e modelo
    /// configurado. O `EchoProvider` fica atras de feature, e este teste
    /// tem de rodar no build padrao.
    struct Stub(&'static str, &'static str);

    #[async_trait::async_trait]
    impl garraia_agents::LlmProvider for Stub {
        fn provider_id(&self) -> &str {
            self.0
        }
        async fn complete(
            &self,
            _request: &garraia_agents::LlmRequest,
        ) -> garraia_common::Result<garraia_agents::LlmResponse> {
            Err(garraia_common::Error::Agent("stub nao completa".into()))
        }
        fn configured_model(&self) -> Option<&str> {
            Some(self.1)
        }
        async fn health_check(&self) -> garraia_common::Result<bool> {
            Ok(true)
        }
    }

    /// O handler responde no formato OpenAI com o que esta registrado — o
    /// cenario da issue: openrouter default com `openrouter/auto` e um ollama
    /// secundario cujo nome sem prefixo nenhum id alcanca — e nada da lista
    /// fixa antiga sobrevive.
    #[tokio::test]
    async fn list_models_reports_registered_providers_only() {
        let runtime = garraia_agents::AgentRuntime::new();
        // O primeiro registrado vira o default.
        runtime.register_provider(std::sync::Arc::new(Stub("openrouter", "openrouter/auto")));
        runtime.register_provider(std::sync::Arc::new(Stub("ollama", "qwen3:8b")));
        let state = std::sync::Arc::new(crate::state::AppState::new(
            garraia_config::AppConfig::default(),
            std::sync::Arc::new(runtime),
            garraia_channels::ChannelRegistry::new(),
        ));
        assert_eq!(
            state.agents.default_provider_id().as_deref(),
            Some("openrouter")
        );

        let resp = list_models(State(state)).await;
        assert_eq!(resp.status(), axum::http::StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .expect("corpo");
        let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");

        assert_eq!(body["object"], "list");
        let data = body["data"].as_array().expect("data e lista");
        assert_eq!(data.len(), 1, "{body}");
        assert_eq!(data[0]["id"], "openrouter/auto");
        assert_eq!(data[0]["object"], "model");
        assert_eq!(data[0]["owned_by"], "openrouter");
        assert!(
            !bytes.windows(5).any(|w| w == b"gpt-4"),
            "a lista fixa antiga nao pode voltar: {body}"
        );
    }
}
