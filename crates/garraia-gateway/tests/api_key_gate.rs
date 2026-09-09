//! #1045: o gate de `gateway.api_key` sobre `/api/*`, montado no
//! `build_router` de verdade.
//!
//! Os testes de unidade em `gateway_auth.rs` provam a decisão do middleware
//! sobre um router de mentira. Só isto aqui prova a **montagem**: que o layer
//! cobre as rotas reais, incluindo as que `build_skill_skin_routes` e
//! `build_plugin_routes` montam sob `/api/` por caminhos próprios; que as
//! três rotas abertas continuam abertas; e que `/v1/*` e `/health` ficam de
//! fora. Um layer no lugar errado da cadeia passaria nos testes de unidade e
//! falharia aqui.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use garraia_agents::AgentRuntime;
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::router::build_router;
use garraia_gateway::state::AppState;
use tokio::sync::Mutex;
use tower::ServiceExt;

const CHAVE: &str = "chave-de-teste-do-gateway";

fn router_com(chave: Option<&str>) -> Router {
    let mut config = AppConfig::default();
    config.gateway.api_key = chave.map(str::to_string);
    let state = Arc::new(AppState::new(
        config,
        Arc::new(AgentRuntime::new()),
        ChannelRegistry::new(),
    ));
    let admin_store = Arc::new(Mutex::new(
        AdminStore::in_memory().expect("in-memory admin store"),
    ));
    build_router(
        state,
        // whatsapp
        Arc::new(Vec::new()),
        // google chat (#1050) — o gate nao toca em `/webhooks/*`, entao a
        // lista vazia basta; o que este teste exercita e o layer sobre
        // `/api/*`.
        Arc::new(Vec::new()),
        // teams (#1050), idem
        Arc::new(Vec::new()),
        // line (#1050), idem
        Arc::new(Vec::new()),
        admin_store,
        Arc::new(vec![0u8; 32]),
    )
}

async fn status(chave: Option<&str>, uri: &str, auth: Option<&str>) -> StatusCode {
    resposta(chave, uri, auth).await.status()
}

async fn resposta(chave: Option<&str>, uri: &str, auth: Option<&str>) -> axum::response::Response {
    let mut req = Request::builder().uri(uri);
    if let Some(a) = auth {
        req = req.header(header::AUTHORIZATION, a);
    }
    let mut req = req.body(Body::empty()).expect("request");
    // O rate limiter le o IP do par em `ConnectInfo`, que em producao vem do
    // `into_make_service_with_connect_info`. Sem ele todo pedido morre em 500
    // no governor, **antes** do gate — que fica por dentro dele de proposito,
    // para uma sondagem sem credencial ainda gastar cota.
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            [127, 0, 0, 1],
            40404,
        ))));
    router_com(chave).oneshot(req).await.expect("resposta")
}

/// Rotas que representam cada forma de montagem sob `/api/`: a cadeia
/// principal, o sub-router de skills/skins e o de plugins.
const GATEADAS: &[&str] = &[
    "/api/status",
    "/api/sessions",
    "/api/memory/recent",
    "/api/skills",
    "/api/skins",
    "/api/plugins",
];

#[tokio::test]
async fn com_chave_as_rotas_de_api_exigem_o_header() {
    for rota in GATEADAS {
        assert_eq!(
            status(Some(CHAVE), rota, None).await,
            StatusCode::UNAUTHORIZED,
            "{rota} respondeu sem a chave"
        );
    }
}

#[tokio::test]
async fn com_chave_e_com_header_o_gate_sai_da_frente() {
    // Estas duas não têm autorização própria depois do gate, então com a
    // chave certa a resposta chega de verdade. As outras da lista têm (o
    // `/api/plugins` ainda quer o cookie de admin fora de loopback), e por
    // isso não entram neste teste — o que importa lá é o 401 sem chave, que
    // os dois testes acima cobrem.
    assert_eq!(
        status(Some(CHAVE), "/api/status", Some(&format!("Bearer {CHAVE}"))).await,
        StatusCode::OK
    );
    assert_eq!(
        status(
            Some(CHAVE),
            "/api/sessions",
            Some(&format!("Bearer {CHAVE}"))
        )
        .await,
        StatusCode::OK
    );
}

#[tokio::test]
async fn a_chave_errada_nao_abre_nada() {
    for rota in GATEADAS {
        assert_eq!(
            status(Some(CHAVE), rota, Some("Bearer nao-e-a-chave")).await,
            StatusCode::UNAUTHORIZED,
            "{rota} aceitou uma chave errada"
        );
    }
}

/// O onboarding do app bate em `/api/health` e `/api/capabilities` antes de
/// o usuário digitar a chave; o console pergunta em `/api/auth-check` se
/// precisa pedi-la. Fechar qualquer uma quebraria a primeira tela.
#[tokio::test]
async fn as_rotas_do_onboarding_continuam_abertas() {
    for aberta in ["/api/health", "/api/capabilities", "/api/auth-check"] {
        assert_eq!(
            status(Some(CHAVE), aberta, None).await,
            StatusCode::OK,
            "{aberta} deixou de ser aberta"
        );
    }
}

#[tokio::test]
async fn fora_de_api_o_gate_nao_age() {
    for fora in ["/health", "/ping", "/v1/models"] {
        assert_ne!(
            status(Some(CHAVE), fora, None).await,
            StatusCode::UNAUTHORIZED,
            "{fora} foi gateado sem precisar"
        );
    }
}

/// A garantia de "zero mudança": sem chave configurada, tudo responde como
/// antes deste PR.
#[tokio::test]
async fn sem_chave_configurada_nada_muda() {
    for rota in ["/api/status", "/api/sessions", "/api/health"] {
        assert_ne!(
            status(None, rota, None).await,
            StatusCode::UNAUTHORIZED,
            "{rota} passou a exigir chave sem haver chave configurada"
        );
    }
}

/// O `/api/auth-check` é como o console decide se pede a chave. Ele tem de
/// concordar com o gate — inclusive quando a chave configurada é só espaço.
#[tokio::test]
async fn auth_check_concorda_com_o_gate() {
    async fn auth_required(chave: Option<&str>) -> bool {
        let resp = resposta(chave, "/api/auth-check", None).await;
        let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .expect("corpo");
        let v: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        v["auth_required"].as_bool().expect("auth_required")
    }

    assert!(auth_required(Some(CHAVE)).await);
    assert!(!auth_required(None).await);
    // Chave só com espaço: o gate fica desligado, e o console tem de saber.
    assert!(!auth_required(Some("   ")).await);
    assert_ne!(
        status(Some("   "), "/api/sessions", None).await,
        StatusCode::UNAUTHORIZED
    );
}
