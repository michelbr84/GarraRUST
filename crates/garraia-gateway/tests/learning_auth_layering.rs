//! #1093: a guarda das rotas mutantes de learning, montada no `build_router`
//! de verdade — e a **ordem** da cadeia.
//!
//! Os testes de unidade em `learning_auth.rs` provam a decisão da guarda
//! sobre um router de mentira. Só isto aqui prova a montagem na cadeia real:
//! que o gate global de `gateway.api_key` roda **antes** da guarda (layer do
//! router pai por fora, layer de rota por dentro), que o anti-CSRF vale para
//! pedido **autenticado** (o bearer não dispensa a origem certa — sem isso,
//! um POST cross-site do navegador de um dono autenticado passa), que um
//! pedido legítimo atravessa as duas camadas e chega ao handler, e que o
//! DNS rebinding contra a instalação default (sem api_key) morre na âncora
//! do `Host` de loopback — com o console local de verdade continuando
//! passando.

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
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

/// POST mutante em learning com os headers que cada teste quiser, no gate
/// global configurado conforme `chave` (`None` = default de instalação,
/// passa-direto).
///
/// O `ConnectInfo` é inserido porque o rate limiter (fora do gate, de
/// propósito) lê o IP do par: sem ele o pedido morre em 500 no governor
/// antes de qualquer camada de auth — o mesmo comentário do `api_key_gate.rs`.
async fn post(chave: Option<&str>, uri: &str, headers: &[(&str, &str)]) -> (StatusCode, String) {
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
    let router = build_router(
        state,
        PushChannelStates::empty(),
        admin_store,
        Arc::new(vec![0u8; 32]),
    );

    let mut builder = Request::builder().method("POST").uri(uri);
    for (nome, valor) in headers {
        builder = builder.header(*nome, *valor);
    }
    let mut req = builder.body(Body::empty()).expect("request");
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            [127, 0, 0, 1],
            40404,
        ))));
    let resp = router.oneshot(req).await.expect("resposta");
    let status = resp.status();
    let corpo = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("corpo da resposta");
    (status, String::from_utf8_lossy(&corpo).into_owned())
}

/// O gate global roda **antes** da guarda: um POST sem bearer e com
/// `Origin` de outra origem morre no 401 do gate, não no 403 da guarda.
/// Se a ordem fosse invertida (guarda por fora do gate), o mesmo pedido
/// viraria 403 `cross-origin` — e este teste pegaria.
#[tokio::test]
async fn o_gate_global_roda_antes_da_guarda() {
    let (status, corpo) = post(
        Some(CHAVE),
        "/api/learning/skills/nao-existe/approve",
        &[("origin", "http://evil.com"), ("host", "127.0.0.1:3888")],
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "corpo: {corpo}");
    assert_eq!(corpo, "gateway: invalid or missing api key");
}

/// O anti-CSRF vale para pedido autenticado: o bearer correto NÃO dispensa
/// a origem certa. Um guarda que só exigisse "bearer OU origem correta"
/// deixaria passar o POST cross-site do navegador de um dono autenticado —
/// exatamente o ataque que a #1093 fecha.
#[tokio::test]
async fn o_anti_csrf_vale_para_pedido_autenticado() {
    let (status, corpo) = post(
        Some(CHAVE),
        "/api/learning/skills/nao-existe/approve",
        &[
            ("authorization", BEARER),
            ("origin", "http://evil.com"),
            ("host", "127.0.0.1:3888"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "corpo: {corpo}");
    assert_eq!(corpo, "learning: cross-origin mutating request refused");
}

/// Pedido legítimo atravessa as duas camadas e chega ao handler: bearer
/// correto, sem `Origin` — o app (Dio) e o `curl` não mandam Origin, só o
/// navegador manda. Aqui cada status proibido denuncia uma camada: 401 = o
/// gate rejeitou o bearer; 403 = a guarda tratou o pedido como cross-origin;
/// 503 = a guarda alegou falta de peer ou de auth. O que vier (404 da
/// skill inexistente, ou 500 se o registry do ambiente de teste reclamar) é
/// resposta do handler — a prova é que nenhuma guarda segurou o pedido.
#[tokio::test]
async fn pedido_legitimo_atravessa_as_duas_camadas() {
    let (status, corpo) = post(
        Some(CHAVE),
        "/api/learning/skills/nao-existe/approve",
        &[("authorization", BEARER)],
    )
    .await;
    assert!(
        !matches!(
            status,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::SERVICE_UNAVAILABLE
        ),
        "pedido legítimo bloqueado por camada de auth (status {status}, corpo {corpo})"
    );
}

/// DNS rebinding no `build_router` de verdade, na config default (sem
/// `gateway.api_key`): o domínio do atacante re-resolvido para 127.0.0.1
/// faz o navegador mandar `Origin` e `Host` iguais ao alias — casam entre
/// si e o passo 1 passava, o peer é loopback e o passo 3 liberava. A
/// âncora é o `Host` de loopback: nome de DNS público nenhum entra.
/// Com gate ligado o bearer já fecha o cenário; este é o buraco da
/// instalação default que a #1093 fecha.
#[tokio::test]
async fn rebinding_de_dominio_externo_da_403_sem_chave() {
    let (status, corpo) = post(
        None,
        "/api/learning/skills/nao-existe/approve",
        &[
            ("origin", "http://evil.example:3888"),
            ("host", "evil.example:3888"),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "corpo: {corpo}");
    assert_eq!(corpo, "learning: cross-origin mutating request refused");

    // Controle negativo da âncora: o console local em `localhost` continua
    // entrando — o 404/500 que vier é do handler, nenhuma camada segurou.
    let (status, corpo) = post(
        None,
        "/api/learning/skills/nao-existe/approve",
        &[
            ("origin", "http://localhost:3888"),
            ("host", "localhost:3888"),
        ],
    )
    .await;
    assert!(
        !matches!(
            status,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::SERVICE_UNAVAILABLE
        ),
        "console local bloqueado pela âncora (status {status}, corpo {corpo})"
    );
}
