//! #1895: HSTS montado no `build_router` de verdade — a **ligação**, não só a
//! decisão.
//!
//! Os testes de unidade em `hsts.rs` provam a tabela do transporte sobre a
//! função pura. Só o router real prova que a camada está na cadeia: um
//! `.layer()` esquecido, fora de ordem, ou aplicado só numa rota passaria por
//! qualquer unit test e por qualquer inspeção de fonte.
//!
//! O caso do processo servindo TLS nativo não é exercitável aqui (depende da
//! feature `tls` do crate), e é justamente por isso que a decisão de produção
//! vem de `origin_guard::esquema_efetivo` — a mesma régua que o guard de
//! `Origin` usa. O que este arquivo trava é o caminho que o CI default roda:
//! instalação http (nenhum header novo) e proxy TLS declarando
//! `X-Forwarded-Proto: https` (header presente em resposta de sucesso **e** de
//! erro).

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, Response, StatusCode, header::STRICT_TRANSPORT_SECURITY};
use garraia_agents::AgentRuntime;
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::push_channels::PushChannelStates;
use garraia_gateway::router::build_router;
use tokio::sync::Mutex;
use tower::ServiceExt;

mod config_dir_descartavel;

const HSTS: &str = "max-age=31536000";

/// Um pedido no `build_router` de verdade (instalação default: sem api_key).
///
/// `ConnectInfo` é obrigatório: o rate limiter lê o IP do par e devolve 500
/// antes de qualquer camada sem ele (mesmo motivo do `api_key_gate.rs`).
async fn pedido(uri: &str, headers: &[(&str, &str)]) -> Response<Body> {
    let config = AppConfig::default();
    let state = Arc::new(config_dir_descartavel::app_state(
        config,
        Arc::new(AgentRuntime::new()),
        ChannelRegistry::new(),
    ));
    let admin_store = Arc::new(Mutex::new(
        AdminStore::in_memory().expect("in-memory admin store"),
    ));
    let router = build_router(
        state,
        PushChannelStates::empty(),
        admin_store,
        Arc::new(vec![0u8; 32]),
    );

    let mut builder = Request::builder().method("GET").uri(uri);
    for (nome, valor) in headers {
        builder = builder.header(*nome, *valor);
    }
    let mut req = builder.body(Body::empty()).expect("request");
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            [127, 0, 0, 1],
            40404,
        ))));
    router.oneshot(req).await.expect("resposta")
}

fn hsts_da_resposta(resp: &Response<Body>) -> Option<String> {
    resp.headers()
        .get(STRICT_TRANSPORT_SECURITY)
        .map(|v| v.to_str().expect("header ASCII").to_string())
}

/// Default de instalação, http puro: NENHUM header novo. É o contrato de
/// preservação de comportamento — quem hoje roda em `127.0.0.1:3888` não pode
/// ver diferença nenhuma.
#[tokio::test]
async fn http_puro_nao_recebe_hsts() {
    for uri in ["/api/health", "/"] {
        let resp = pedido(uri, &[("host", "127.0.0.1:3888")]).await;
        assert!(
            hsts_da_resposta(&resp).is_none(),
            "{uri}: http puro não pode mandar HSTS (status {})",
            resp.status()
        );
    }
}

/// `X-Forwarded-Proto: https` (proxy TLS na frente): o header sai na resposta
/// de sucesso.
#[tokio::test]
async fn proxy_tls_recebe_hsts() {
    let resp = pedido(
        "/api/health",
        &[("host", "127.0.0.1:3888"), ("x-forwarded-proto", "https")],
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(hsts_da_resposta(&resp).as_deref(), Some(HSTS));
}

/// A camada é a mais externa: resposta de erro do handler/gate também carrega
/// o header — não existe caminho "servido em claro sem instrução".
#[tokio::test]
async fn resposta_de_erro_tambem_recebe_hsts() {
    let resp = pedido(
        "/api/rota-que-nao-existe",
        &[("host", "127.0.0.1:3888"), ("x-forwarded-proto", "https")],
    )
    .await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    assert_eq!(hsts_da_resposta(&resp).as_deref(), Some(HSTS));
}

/// `http` no cabeçalho do proxy (ou lista cujo primeiro valor é http) não emite
/// — quem fala http com o proxy recebe a resposta como veio.
#[tokio::test]
async fn proxy_http_nao_recebe_hsts() {
    for valor in ["http", "http, https"] {
        let resp = pedido(
            "/api/health",
            &[("host", "127.0.0.1:3888"), ("x-forwarded-proto", valor)],
        )
        .await;
        assert!(
            hsts_da_resposta(&resp).is_none(),
            "xfp={valor:?} não pode mandar HSTS"
        );
    }
}
