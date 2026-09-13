//! #1182: a guarda anti-CSRF genérica das rotas mutantes e o CORS default,
//! montados no `build_router` de verdade — e a **ordem** da cadeia.
//!
//! Os testes de unidade em `origin_guard.rs` provam a decisão da guarda sobre
//! um router de mentira. Só isto aqui prova a montagem na cadeia real: que o
//! gate global de `gateway.api_key` roda **antes** da guarda (um POST sem
//! bearer e com `Origin` estranho morre no 401, não no 403), que o anti-CSRF
//! vale para pedido **autenticado**, que um pedido legítimo atravessa as duas
//! camadas, e que o `CorsLayer` deixou de anunciar `allow_origin(Any)` na
//! configuração default.

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, Response, StatusCode};
use garraia_agents::AgentRuntime;
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::push_channels::PushChannelStates;
use garraia_gateway::router::build_router;
use garraia_gateway::state::AppState;
use tokio::sync::Mutex;
use tower::ServiceExt;

const CHAVE: &str = "chave-de-teste-do-gateway";
/// Pares com `CHAVE`: o valor do header que o gate global aceita.
const BEARER: &str = "Bearer chave-de-teste-do-gateway";

/// Corpo do 403 da guarda genérica — o contrato de resposta que este PR
/// introduz. Diferente do `"learning: …"`, de propósito: quem responde diz
/// quem é.
const CORPO_403: &str =
    "gateway: cross-origin mutating request refused (see gateway.allowed_origins)";

/// Um pedido no `build_router` de verdade.
///
/// O `ConnectInfo` é inserido porque o rate limiter (fora do gate, de
/// propósito) lê o IP do par: sem ele o pedido morre em 500 no governor antes
/// de qualquer camada de auth — o mesmo comentário do `api_key_gate.rs`.
async fn pedido(
    chave: Option<&str>,
    allowed_origins: &[&str],
    metodo: &str,
    uri: &str,
    headers: &[(&str, &str)],
) -> Response<Body> {
    let mut config = AppConfig::default();
    config.gateway.api_key = chave.map(str::to_string);
    config.gateway.allowed_origins = allowed_origins.iter().map(|o| (*o).to_string()).collect();
    let state = Arc::new(AppState::new(
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

    let mut builder = Request::builder().method(metodo).uri(uri);
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

async fn status_corpo(resp: Response<Body>) -> (StatusCode, String) {
    let status = resp.status();
    let corpo = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("corpo da resposta");
    (status, String::from_utf8_lossy(&corpo).into_owned())
}

/// O gate global roda **antes** da guarda: sem bearer e com `Origin` de outra
/// origem, o 401 do gate vem primeiro. Se a ordem dos `.layer()` fosse
/// invertida, o mesmo pedido viraria 403 — e este teste pegaria.
#[tokio::test]
async fn o_gate_global_roda_antes_da_guarda() {
    let resp = pedido(
        Some(CHAVE),
        &[],
        "PATCH",
        "/api/settings",
        &[("origin", "http://evil.com"), ("host", "127.0.0.1:3888")],
    )
    .await;
    let (status, corpo) = status_corpo(resp).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "corpo: {corpo}");
    assert_eq!(corpo, "gateway: invalid or missing api key");
}

/// O anti-CSRF vale para pedido autenticado: o bearer correto NÃO dispensa a
/// origem certa. Sem isso, um `PATCH` cross-site do navegador de um dono
/// autenticado passaria — exatamente o ataque que a #1182 fecha.
#[tokio::test]
async fn o_anti_csrf_vale_para_pedido_autenticado() {
    let resp = pedido(
        Some(CHAVE),
        &[],
        "PATCH",
        "/api/settings",
        &[
            ("authorization", BEARER),
            ("origin", "http://evil.com"),
            ("host", "127.0.0.1:3888"),
        ],
    )
    .await;
    let (status, corpo) = status_corpo(resp).await;
    assert_eq!(status, StatusCode::FORBIDDEN, "corpo: {corpo}");
    assert_eq!(corpo, CORPO_403);
}

/// Instalação default (sem `api_key`) e pedido de cliente não-navegador (sem
/// `Origin`): atravessa. Cada status proibido aqui denuncia uma camada — 401
/// = gate, 403 = guarda, 503 = fail-closed. O que vier (400 do corpo vazio,
/// 200, o que for) é resposta do handler.
#[tokio::test]
async fn pedido_sem_origin_e_sem_chave_atravessa() {
    let resp = pedido(
        None,
        &[],
        "PATCH",
        "/api/settings",
        &[("host", "127.0.0.1:3888")],
    )
    .await;
    let (status, corpo) = status_corpo(resp).await;
    assert!(
        !matches!(
            status,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::SERVICE_UNAVAILABLE
        ),
        "pedido legítimo bloqueado por camada de auth (status {status}, corpo {corpo})"
    );
}

/// O app mobile na LAN sem `api_key` continua entrando. É o recorte
/// deliberado do `origin_guard`: ele NÃO herdou o `503 auth not configured`
/// do `learning_mutations_guard`, que mataria este cenário suportado.
#[tokio::test]
async fn app_na_lan_sem_chave_nao_leva_503() {
    let resp = pedido(
        None,
        &[],
        "POST",
        "/api/sessions",
        &[("host", "192.168.1.10:3888")],
    )
    .await;
    let (status, corpo) = status_corpo(resp).await;
    assert_ne!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "o guarda generico nao pode fail-closed sem chave (corpo {corpo})"
    );
    assert_ne!(status, StatusCode::FORBIDDEN, "corpo: {corpo}");
}

/// A skip-list continua sendo dona do caminho dela: um POST cross-origin em
/// `/api/learning/*` leva o corpo `"learning: …"`, não o desta guarda. Prova
/// que não há dupla aplicação (que trocaria o contrato de resposta que o
/// `learning_auth_layering.rs` trava).
#[tokio::test]
async fn learning_continua_com_a_guarda_propria() {
    let resp = pedido(
        None,
        &[],
        "POST",
        "/api/learning/skills/nao-existe/approve",
        &[
            ("origin", "http://evil.example:3888"),
            ("host", "evil.example:3888"),
        ],
    )
    .await;
    let (status, corpo) = status_corpo(resp).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(corpo, "learning: cross-origin mutating request refused");
}

/// O CORS default deixou de ser `allow_origin(Any)`: sem
/// `gateway.allowed_origins`, nenhuma resposta carrega
/// `access-control-allow-origin`, então a página do atacante até pode
/// disparar o `GET` mas não lê a resposta. Com a origem listada, o header
/// volta — quem tem front externo declara e segue.
#[tokio::test]
async fn cors_default_nao_anuncia_origem_nenhuma() {
    let resp = pedido(
        None,
        &[],
        "GET",
        "/api/sessions",
        &[("origin", "http://evil.com"), ("host", "127.0.0.1:3888")],
    )
    .await;
    assert!(
        resp.headers().get("access-control-allow-origin").is_none(),
        "o default anunciou uma origem cross-origin"
    );

    let resp = pedido(
        None,
        &["http://evil.com"],
        "GET",
        "/api/sessions",
        &[("origin", "http://evil.com"), ("host", "127.0.0.1:3888")],
    )
    .await;
    assert_eq!(
        resp.headers()
            .get("access-control-allow-origin")
            .and_then(|v| v.to_str().ok()),
        Some("http://evil.com"),
        "a origem declarada pelo dono tem de continuar valendo"
    );
}

/// A mesma origem declarada em `allowed_origins` também destrava a âncora
/// anti-rebinding da guarda: é o perfil "reverse proxy com domínio próprio".
#[tokio::test]
async fn dominio_declarado_atravessa_a_ancora() {
    let resp = pedido(
        None,
        &["http://meu.dominio:3888"],
        "POST",
        "/api/sessions",
        &[
            ("origin", "http://meu.dominio:3888"),
            ("host", "meu.dominio:3888"),
        ],
    )
    .await;
    let (status, corpo) = status_corpo(resp).await;
    assert_ne!(status, StatusCode::FORBIDDEN, "corpo: {corpo}");

    // Controle negativo: o mesmo pedido sem a declaração morre na âncora.
    let resp = pedido(
        None,
        &[],
        "POST",
        "/api/sessions",
        &[
            ("origin", "http://meu.dominio:3888"),
            ("host", "meu.dominio:3888"),
        ],
    )
    .await;
    let (status, corpo) = status_corpo(resp).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(corpo, CORPO_403);
}
