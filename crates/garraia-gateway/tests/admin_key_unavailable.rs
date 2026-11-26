//! #1992: sem chave mestra do admin utilizavel, o gateway sobe e so o painel
//! responde 503.
//!
//! Os testes de unidade em `admin/shared.rs` provam quando o boot recusa a
//! chave. Este arquivo prova a outra metade, no `build_router` de verdade: o
//! `AdminKey::Unavailable` chega ao painel inteiro (pagina e API, rota publica
//! ou autenticada, qualquer metodo), o motivo nao vaza na resposta, e o resto
//! do gateway continua atendendo.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use garraia_agents::AgentRuntime;
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_gateway::admin::handlers::AdminKey;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::push_channels::PushChannelStates;
use garraia_gateway::router::build_router;
use garraia_gateway::state::AppState;
use serde_json::Value;
use tokio::sync::Mutex;
use tower::ServiceExt;

fn router(admin_key: AdminKey) -> Router {
    let state = Arc::new(AppState::new(
        AppConfig::default(),
        Arc::new(AgentRuntime::new()),
        ChannelRegistry::new(),
    ));
    let admin_store = Arc::new(Mutex::new(
        AdminStore::in_memory().expect("in-memory admin store"),
    ));
    build_router(state, PushChannelStates::empty(), admin_store, admin_key)
}

async fn pedido(router: &Router, metodo: &str, uri: &str) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(metodo)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from("{}"))
        .expect("request");
    // O rate limiter le o IP do par em `ConnectInfo`; sem ele todo pedido
    // morre em 500 antes de chegar a rota (mesmo motivo do `hsts.rs`).
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            [127, 0, 0, 1],
            40422,
        ))));
    let resp = router.clone().oneshot(req).await.expect("resposta");
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("corpo");
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn sem_chave_o_painel_inteiro_responde_503() {
    let router = router(AdminKey::Unavailable);

    for (metodo, uri) in [
        ("GET", "/admin"),
        ("GET", "/admin/api/setup/status"),
        ("POST", "/admin/api/login"),
        ("POST", "/admin/api/setup"),
        ("GET", "/admin/api/secrets"),
        ("POST", "/admin/api/2fa/setup"),
        ("GET", "/admin/rota/que/nao/existe"),
    ] {
        let (status, corpo) = pedido(&router, metodo, uri).await;
        assert_eq!(
            status,
            StatusCode::SERVICE_UNAVAILABLE,
            "{metodo} {uri}: o painel tem que estar fora do ar ({corpo})"
        );
        assert_eq!(corpo["error"], "admin_unavailable", "{metodo} {uri}");
    }
}

/// A resposta vai para quem bater na porta, sem sessao: ela aponta o log e a
/// doc, e nao diz qual parte do `admin.db` esta ruim.
#[tokio::test]
async fn o_503_nao_carrega_o_motivo() {
    let router = router(AdminKey::Unavailable);
    let (_, corpo) = pedido(&router, "GET", "/admin/api/setup/status").await;
    let texto = corpo.to_string();
    for palavra in ["kdf", "salt", "iteration", "legacy", "base64"] {
        assert!(
            !texto.to_lowercase().contains(palavra),
            "o corpo do 503 nao pode descrever a falha: achou `{palavra}` em {texto}"
        );
    }
}

/// O resto do gateway nao depende da chave do admin e segue no ar.
#[tokio::test]
async fn sem_chave_o_resto_do_gateway_continua_atendendo() {
    let router = router(AdminKey::Unavailable);
    let (status, corpo) = pedido(&router, "GET", "/api/health").await;
    assert_eq!(status, StatusCode::OK, "/api/health: {corpo}");
}

/// Controle: com a chave pronta, as mesmas rotas voltam a ser o painel.
#[tokio::test]
async fn com_chave_o_painel_atende_normalmente() {
    let router = router(AdminKey::Ready(Arc::new(vec![0u8; 32])));
    let (status, corpo) = pedido(&router, "GET", "/admin/api/setup/status").await;
    assert_eq!(status, StatusCode::OK, "setup/status: {corpo}");
    let (status, _) = pedido(&router, "GET", "/admin").await;
    assert_eq!(status, StatusCode::OK, "pagina do painel");
}
