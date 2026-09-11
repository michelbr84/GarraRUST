//! #1120: troca de senha do proprio admin, por HTTP, contra o router real.
//!
//! Os testes de unidade do `admin` provam pecas isoladas; o que este arquivo
//! prova e a **montagem mais o efeito**:
//!
//! * a rota existe dentro do grupo autenticado, entao herda sessao e CSRF;
//! * a senha muda de verdade — conferido com `verify_password` no store, nao
//!   apenas com o status da resposta;
//! * a senha atual errada (e a senha nova curta) nao mudam nada;
//! * cada terminal escreve auditoria, com o outcome certo;
//! * as outras sessoes do usuario sao revogadas, e a que fez o pedido nao.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use garraia_agents::AgentRuntime;
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_gateway::admin::store::{AdminStore, AuditEntry};
use garraia_gateway::admin::users::CHANGE_PASSWORD_ACTION;
use garraia_gateway::push_channels::PushChannelStates;
use garraia_gateway::router::build_router;
use garraia_gateway::state::AppState;
use serde_json::json;
use tokio::sync::Mutex;
use tower::ServiceExt;

const USUARIO: &str = "root";
const SENHA_INICIAL: &str = "senha-inicial-do-admin";
const SENHA_NOVA: &str = "senha-nova-do-admin";
const SENHA_CURTA: &str = "curta";

const CAMINHO: &str = "/admin/api/change-password";

/// Cookie de sessao + token de CSRF que o console usaria.
struct Sessao {
    token: String,
    csrf: String,
}

/// Router de verdade, com um `AdminStore` em memoria que o teste continua
/// enxergando depois do pedido — e assim que ele confere o efeito colateral.
fn router_com_store() -> (Router, Arc<Mutex<AdminStore>>) {
    let config = AppConfig::default();
    let state = Arc::new(AppState::new(
        config,
        Arc::new(AgentRuntime::new()),
        ChannelRegistry::new(),
    ));
    let store = Arc::new(Mutex::new(
        AdminStore::in_memory().expect("store em memoria"),
    ));
    let router = build_router(
        state,
        PushChannelStates::empty(),
        Arc::clone(&store),
        Arc::new(vec![0u8; 32]),
    );
    (router, store)
}

async fn post_admin(
    router: &Router,
    caminho: &str,
    sessao: Option<&Sessao>,
    csrf: Option<&str>,
    corpo: serde_json::Value,
) -> (StatusCode, serde_json::Value, HeaderMap) {
    let mut req = Request::builder()
        .method("POST")
        .uri(caminho)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(s) = sessao {
        req = req.header(header::COOKIE, format!("garraia_admin_session={}", s.token));
    }
    if let Some(t) = csrf {
        req = req.header("x-csrf-token", t);
    }

    let mut req = req
        .body(Body::from(serde_json::to_vec(&corpo).expect("json")))
        .expect("request");
    // O governor le o IP de `ConnectInfo`, que em producao vem do
    // `into_make_service_with_connect_info`. Sem ele o pedido morre em 500
    // antes de chegar na rota.
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            [127, 0, 0, 1],
            41120,
        ))));

    let resp = router
        .clone()
        .oneshot(req)
        .await
        .expect("o router respondeu");
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = axum::body::to_bytes(resp.into_body(), 256 * 1024)
        .await
        .expect("corpo");
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json, headers)
}

/// Cria o primeiro admin via `/setup`, que ja devolve sessao e CSRF.
async fn criar_admin(router: &Router) -> Sessao {
    let (status, corpo, headers) = post_admin(
        router,
        "/admin/api/setup",
        None,
        None,
        json!({ "username": USUARIO, "password": SENHA_INICIAL }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{corpo}");

    let csrf = corpo["csrf_token"]
        .as_str()
        .expect("csrf_token")
        .to_string();
    let cookie = headers
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .expect("set-cookie")
        .to_string();
    let token = cookie
        .split(';')
        .next()
        .and_then(|parte| parte.trim().strip_prefix("garraia_admin_session="))
        .expect("token da sessao")
        .to_string();

    Sessao { token, csrf }
}

/// Uma segunda sessao do mesmo usuario, como a de outro navegador.
async fn abrir_outra_sessao(router: &Router) -> Sessao {
    let (status, corpo, headers) = post_admin(
        router,
        "/admin/api/login",
        None,
        None,
        json!({ "username": USUARIO, "password": SENHA_INICIAL }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{corpo}");

    let csrf = corpo["csrf_token"]
        .as_str()
        .expect("csrf_token")
        .to_string();
    let cookie = headers
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .expect("set-cookie")
        .to_string();
    let token = cookie
        .split(';')
        .next()
        .and_then(|parte| parte.trim().strip_prefix("garraia_admin_session="))
        .expect("token da sessao")
        .to_string();

    Sessao { token, csrf }
}

async fn senha_confere(store: &Arc<Mutex<AdminStore>>, senha: &str) -> bool {
    let guard = store.lock().await;
    guard.verify_password(USUARIO, senha).is_some()
}

async fn sessao_valida(store: &Arc<Mutex<AdminStore>>, token: &str) -> bool {
    let guard = store.lock().await;
    guard.validate_session(token).is_some()
}

/// A trilha de auditoria da propria rota, filtrada por acao e recurso.
async fn trilha(store: &Arc<Mutex<AdminStore>>) -> Vec<AuditEntry> {
    let guard = store.lock().await;
    guard.list_audit_log(50, 0, Some("user"), Some(CHANGE_PASSWORD_ACTION))
}

#[tokio::test]
async fn troca_a_senha_do_proprio_usuario_e_audita() {
    let (router, store) = router_com_store();
    let sessao = criar_admin(&router).await;

    let (status, corpo, _) = post_admin(
        &router,
        CAMINHO,
        Some(&sessao),
        Some(&sessao.csrf),
        json!({ "current_password": SENHA_INICIAL, "new_password": SENHA_NOVA }),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "{corpo}");
    assert_eq!(corpo["ok"], serde_json::Value::Bool(true), "{corpo}");

    // A prova que importa: a senha nova abre e a antiga nao abre mais.
    assert!(senha_confere(&store, SENHA_NOVA).await, "a senha nao mudou");
    assert!(
        !senha_confere(&store, SENHA_INICIAL).await,
        "a senha antiga continua valendo"
    );

    let trilha = trilha(&store).await;
    assert_eq!(trilha.len(), 1, "exatamente um evento: {trilha:?}");
    assert_eq!(trilha[0].outcome, "success");
    assert_eq!(trilha[0].username.as_deref(), Some(USUARIO));
}

#[tokio::test]
async fn senha_atual_errada_nao_troca_nada() {
    let (router, store) = router_com_store();
    let sessao = criar_admin(&router).await;

    let (status, corpo, _) = post_admin(
        &router,
        CAMINHO,
        Some(&sessao),
        Some(&sessao.csrf),
        json!({ "current_password": "nao-e-a-senha", "new_password": SENHA_NOVA }),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED, "{corpo}");
    assert_eq!(
        corpo["error"],
        serde_json::Value::String("password verification failed".to_string())
    );

    assert!(
        senha_confere(&store, SENHA_INICIAL).await,
        "a senha nao pode mudar com a atual errada"
    );
    assert!(!senha_confere(&store, SENHA_NOVA).await);

    let trilha = trilha(&store).await;
    assert_eq!(trilha.len(), 1, "{trilha:?}");
    assert_eq!(trilha[0].outcome, "failure");
}

#[tokio::test]
async fn senha_nova_curta_e_rejeitada_sem_tocar_no_hash() {
    let (router, store) = router_com_store();
    let sessao = criar_admin(&router).await;

    let (status, corpo, _) = post_admin(
        &router,
        CAMINHO,
        Some(&sessao),
        Some(&sessao.csrf),
        json!({ "current_password": SENHA_INICIAL, "new_password": SENHA_CURTA }),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{corpo}");
    assert!(
        senha_confere(&store, SENHA_INICIAL).await,
        "a senha antiga tem de continuar valendo"
    );
    assert!(!senha_confere(&store, SENHA_CURTA).await);

    let trilha = trilha(&store).await;
    assert_eq!(trilha.len(), 1, "{trilha:?}");
    assert_eq!(trilha[0].outcome, "failure");
}

#[tokio::test]
async fn sem_sessao_o_pedido_nem_chega_no_handler() {
    let (router, store) = router_com_store();
    let sessao = criar_admin(&router).await;

    let (status, _, _) = post_admin(
        &router,
        CAMINHO,
        None,
        Some(&sessao.csrf),
        json!({ "current_password": SENHA_INICIAL, "new_password": SENHA_NOVA }),
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(senha_confere(&store, SENHA_INICIAL).await);
    assert!(
        trilha(&store).await.is_empty(),
        "sem auditoria de quem nao entrou"
    );
}

#[tokio::test]
async fn sem_csrf_o_pedido_e_barrado() {
    let (router, store) = router_com_store();
    let sessao = criar_admin(&router).await;

    let (status, _, _) = post_admin(
        &router,
        CAMINHO,
        Some(&sessao),
        None,
        json!({ "current_password": SENHA_INICIAL, "new_password": SENHA_NOVA }),
    )
    .await;

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(senha_confere(&store, SENHA_INICIAL).await);
    assert!(trilha(&store).await.is_empty());
}

/// Depois da troca, as outras sessoes do usuario caem; a que fez o pedido
/// continua de pe, senao o console se desloga no meio do proprio submit.
#[tokio::test]
async fn revoga_as_outras_sessoes_e_mantem_a_atual() {
    let (router, store) = router_com_store();
    let primeira = criar_admin(&router).await;
    let segunda = abrir_outra_sessao(&router).await;
    assert_ne!(primeira.token, segunda.token);

    let (status, corpo, _) = post_admin(
        &router,
        CAMINHO,
        Some(&segunda),
        Some(&segunda.csrf),
        json!({ "current_password": SENHA_INICIAL, "new_password": SENHA_NOVA }),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{corpo}");

    assert!(
        sessao_valida(&store, &segunda.token).await,
        "a sessao que pediu a troca tem de sobreviver"
    );
    assert!(
        !sessao_valida(&store, &primeira.token).await,
        "a outra sessao tem de ser revogada"
    );
}
