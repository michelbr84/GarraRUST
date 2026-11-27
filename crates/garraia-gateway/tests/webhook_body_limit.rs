//! Teto do corpo nas quatro rotas de webhook push, no `build_router` de
//! verdade.
//!
//! O handler de cada webhook confere o tamanho antes de parsear
//! (`garraia_channels::webhook_body`), mas ali o corpo já está inteiro na
//! memória. O que impede o buffer de crescer é a rota recusar antes — e é
//! essa propriedade, de ponta a ponta, que este arquivo trava: nenhuma das
//! quatro rotas aceita um corpo acima do teto, **antes** de qualquer
//! autenticação.
//!
//! ## Por que o router vem embrulhado num limite mais largo
//!
//! O teto declarado é igual ao default do Axum para `Bytes` (2 MiB). Um POST
//! de `MAX + 1` bytes no `build_router` puro receberia 413 **com ou sem** a
//! camada das rotas, e o teste não distinguiria as duas coisas. Por isso cada
//! pedido passa por um `DefaultBodyLimit` de fora, quatro vezes mais largo —
//! o que um router pai faria por causa de uma rota de upload. Aí só a camada
//! de dentro explica o 413: tirar o `.layer(teto_do_corpo_de_webhook())` de
//! uma rota faz o corpo passar e a resposta virar 401/403.
//!
//! `toda_rota_de_webhook_do_router_esta_na_lista` cuida do outro lado: uma
//! rota de webhook nova que ninguém pôs em `ROTAS` não escapa dos dois testes
//! de tamanho por esquecimento.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::DefaultBodyLimit;
use axum::http::{Request, StatusCode};
use garraia_agents::AgentRuntime;
use garraia_channels::ChannelRegistry;
use garraia_channels::webhook_body::MAX_WEBHOOK_BODY_BYTES;
use garraia_config::AppConfig;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::push_channels::PushChannelStates;
use garraia_gateway::router::build_router;
use tokio::sync::Mutex;
use tower::ServiceExt;

mod config_dir_descartavel;

const ROTAS: [&str; 4] = [
    "/webhooks/whatsapp",
    "/webhooks/google-chat",
    "/webhooks/teams",
    "/webhooks/line",
];

/// `ROTAS` é uma lista escrita à mão, e uma quinta rota de webhook montada
/// sem a camada não apareceria nela. Este teste fecha a lacuna pelo outro
/// lado: toda rota `/webhooks/...` que o `router.rs` declara tem de estar em
/// `ROTAS` — e, estando, passa pelos dois testes abaixo.
#[test]
fn toda_rota_de_webhook_do_router_esta_na_lista() {
    let fonte = include_str!("../src/router.rs");
    let mut declaradas: Vec<&str> = fonte
        .split('"')
        .filter(|trecho| trecho.starts_with("/webhooks/"))
        .collect();
    declaradas.sort_unstable();
    declaradas.dedup();

    let mut esperadas = ROTAS.to_vec();
    esperadas.sort_unstable();

    assert_eq!(
        declaradas, esperadas,
        "rota de webhook nova no router.rs: ponha a camada \
         `teto_do_corpo_de_webhook()` nela e acrescente-a a ROTAS"
    );
}

/// Um POST no `build_router` de verdade, sem nenhum canal push configurado e
/// embrulhado num limite de corpo mais largo que o teto (ver o doc do arquivo).
///
/// `ConnectInfo` é obrigatório: o rate limiter lê o IP do par e devolve 500
/// antes de qualquer camada sem ele (mesmo motivo do `hsts.rs`).
async fn post(uri: &str, corpo: Vec<u8>) -> StatusCode {
    let state = Arc::new(config_dir_descartavel::app_state(
        AppConfig::default(),
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
    )
    .layer(DefaultBodyLimit::max(4 * MAX_WEBHOOK_BODY_BYTES));

    let mut req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("host", "127.0.0.1:3888")
        .header("content-type", "application/json")
        .body(Body::from(corpo))
        .expect("request");
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            [127, 0, 0, 1],
            40404,
        ))));
    router.oneshot(req).await.expect("resposta").status()
}

#[tokio::test]
async fn corpo_acima_do_teto_e_recusado_em_toda_rota_de_webhook() {
    for rota in ROTAS {
        let status = post(rota, vec![b' '; MAX_WEBHOOK_BODY_BYTES + 1]).await;
        assert_eq!(
            status,
            StatusCode::PAYLOAD_TOO_LARGE,
            "{rota} aceitou um corpo acima do teto"
        );
    }
}

/// O controle do teste acima: o 413 é do tamanho, não da rota. Um corpo no
/// teto exato passa da camada e para onde tem de parar — na autenticação,
/// que sem canal configurado recusa tudo.
#[tokio::test]
async fn corpo_no_teto_passa_da_camada_e_para_na_autenticacao() {
    for rota in ROTAS {
        let status = post(rota, vec![b' '; MAX_WEBHOOK_BODY_BYTES]).await;
        assert!(
            matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN),
            "{rota} devia parar na autenticacao, respondeu {status}"
        );
    }
}
