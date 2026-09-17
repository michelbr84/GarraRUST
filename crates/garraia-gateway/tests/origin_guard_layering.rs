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
const CORPO_403: &str = "gateway: cross-origin request refused (see gateway.allowed_origins)";

/// Headers de um handshake WebSocket de navegador, sem os de chave
/// (`Sec-WebSocket-Key` etc.): basta para a guarda decidir; o que vier
/// depois dela (400/426 do `WebSocketUpgrade`) é resposta do handler.
const HANDSHAKE: &[(&str, &str)] = &[("upgrade", "websocket"), ("connection", "Upgrade")];

/// As rotas mutantes reais nomeadas pela #1182, cada uma vinda de um pedaço
/// diferente da montagem (cadeia principal, `build_skill_skin_routes`,
/// `nest("/admin")`, `/v1`, `/chat`, `/a2a`). Rodar contra o `build_router`
/// de verdade é o que prova que a guarda está montada DEPOIS de todo
/// `merge`/`nest` — um `.merge()` novo colocado abaixo do `.layer()` faria
/// a rota dele nascer desprotegida, e este loop pegaria.
const MUTANTES: &[(&str, &str)] = &[
    // provider, modo e settings
    ("POST", "/api/providers"),
    ("POST", "/api/providers/test"),
    ("PATCH", "/api/providers/default"),
    ("PATCH", "/api/settings"),
    ("POST", "/api/mode/select"),
    ("POST", "/api/modes/custom"),
    ("PATCH", "/api/modes/custom/x"),
    ("DELETE", "/api/modes/custom/x"),
    // sessões e memória
    ("POST", "/api/sessions"),
    ("POST", "/api/sessions/x/messages"),
    ("DELETE", "/api/sessions/x"),
    ("DELETE", "/api/memory"),
    ("DELETE", "/api/memory/x"),
    // MCP e integrações
    ("POST", "/api/mcp/marketplace/install"),
    ("POST", "/api/openclaw/connect"),
    ("POST", "/api/openclaw/disconnect"),
    // disco
    ("POST", "/api/skills"),
    ("PUT", "/api/skills/x"),
    ("DELETE", "/api/skills/x"),
    ("POST", "/api/skins"),
    ("DELETE", "/api/skins/x"),
    ("POST", "/api/projects"),
    ("PUT", "/api/projects/x"),
    ("DELETE", "/api/projects/x"),
    // custo
    ("POST", "/api/tts"),
    ("POST", "/api/stt"),
    // fora de /api/
    ("POST", "/v1/chat/completions"),
    ("POST", "/v1/messages"),
    ("POST", "/v1/messages/count_tokens"),
    ("POST", "/chat"),
    ("POST", "/a2a/tasks"),
    ("POST", "/a2a/tasks/x/cancel"),
    // bootstrap do admin, fora do require_csrf do sub-router
    ("POST", "/admin/api/setup"),
    ("POST", "/admin/api/login"),
    ("POST", "/admin/api/recovery/start"),
];

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

/// Toda rota mutante nomeada pela #1182 recebe o 403 desta guarda no router
/// de verdade — inclusive as que vêm de `merge`/`nest` (skills/skins, admin,
/// `/v1`, `/chat`, `/a2a`), e o bootstrap do admin (`/admin/api/setup`), que
/// não tem `require_csrf` próprio e saiu da skip-list na revisão.
#[tokio::test]
async fn toda_rota_mutante_nomeada_recebe_o_403_no_router_real() {
    for (metodo, uri) in MUTANTES {
        let resp = pedido(
            None,
            &[],
            metodo,
            uri,
            &[
                ("origin", "http://evil.example"),
                ("host", "127.0.0.1:3888"),
            ],
        )
        .await;
        let (status, corpo) = status_corpo(resp).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "{metodo} {uri}: corpo {corpo}"
        );
        assert_eq!(corpo, CORPO_403, "{metodo} {uri}");

        // Controle: a rota EXISTE com esse método. A guarda responde 403
        // antes do roteamento, então sem isto um caminho digitado errado
        // passaria por "guardado". Caminhos com `/x` são ids inventados e
        // podem legitimamente dar 404 no handler; nos literais, um handler
        // ainda pode responder 404 com corpo ("OpenClaw is not configured"),
        // mas o 404 de rota inexistente do axum vem sem corpo.
        let resp = pedido(None, &[], metodo, uri, &[("host", "127.0.0.1:3888")]).await;
        let (status, corpo) = status_corpo(resp).await;
        assert_ne!(
            status,
            StatusCode::METHOD_NOT_ALLOWED,
            "{metodo} {uri} não existe com esse método: {corpo}"
        );
        if !uri.contains("/x") {
            assert!(
                !(status == StatusCode::NOT_FOUND && corpo.is_empty()),
                "{metodo} {uri} não existe no router"
            );
        }
        assert_ne!(
            status,
            StatusCode::FORBIDDEN,
            "{metodo} {uri} sem Origin recusado: {corpo}"
        );
    }
}

/// O console web same-origin atravessa a guarda no router real: `Origin`
/// igual ao `Host` (IP literal), sem chave. É o pedido que o webchat faz.
#[tokio::test]
async fn mutacao_same_origin_do_console_atravessa() {
    for host in ["127.0.0.1:3888", "192.168.1.10:3888", "localhost:3888"] {
        let origin = format!("http://{host}");
        let resp = pedido(
            None,
            &[],
            "PATCH",
            "/api/settings",
            &[("host", host), ("origin", &origin)],
        )
        .await;
        let (status, corpo) = status_corpo(resp).await;
        assert!(
            !matches!(
                status,
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::SERVICE_UNAVAILABLE
            ),
            "console em {host} bloqueado por camada de auth (status {status}, corpo {corpo})"
        );
    }
}

/// O handshake de WebSocket é um `GET`, mas não passa por CORS: com `Origin`
/// estranho, `/ws` e `/ws/parrot` recusam com 403. Antes deste PR
/// `/ws/parrot` não tinha checagem nenhuma — qualquer página visitada pelo
/// dono abria um turno completo do agente e lia a resposta.
///
/// A outra metade da guarda de `/ws/parrot` — o gate de `gateway.api_key`,
/// que é o que cobre o cliente **sem** `Origin` (app, CLI, `websocat`) — não
/// pode ser medida aqui: ela mora no corpo do handler, e o `WebSocketUpgrade`
/// rejeita antes disso com 426 sempre que não há conexão hyper por trás, como
/// num `oneshot`. Está em `auth_test.rs`, contra um servidor de verdade.
#[tokio::test]
async fn handshake_websocket_cross_origin_da_403_em_ws_e_ws_parrot() {
    for uri in ["/ws", "/ws/parrot"] {
        for (origin, host) in [
            ("http://evil.example", "127.0.0.1:3888"),
            ("http://evil.example:3888", "evil.example:3888"), // rebinding
            ("null", "127.0.0.1:3888"),
        ] {
            let mut headers = vec![("origin", origin), ("host", host)];
            headers.extend_from_slice(HANDSHAKE);
            let resp = pedido(None, &[], "GET", uri, &headers).await;
            let (status, corpo) = status_corpo(resp).await;
            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "{uri} com Origin {origin}: {corpo}"
            );
            assert!(
                corpo.contains("cross-origin"),
                "{uri}: o 403 tem de ser da guarda, corpo {corpo}"
            );
        }
    }
}

/// Os clientes legítimos dos dois WebSockets não são recusados pela guarda:
/// o webchat same-origin em `/ws`, e a webview Tauri do Garra Desktop em
/// `/ws/parrot` (`tauri://localhost`; a variante `http://tauri.localhost` do
/// WebView2 só entra num gateway Windows, e é testada em `origin_guard.rs`),
/// mais o cliente sem `Origin`. O que vier depois (400/426 do
/// `WebSocketUpgrade` sem `Sec-WebSocket-Key`) é do handler; o que NÃO pode
/// vir é 403.
#[tokio::test]
async fn handshake_websocket_legitimo_nao_e_recusado() {
    for uri in ["/ws", "/ws/parrot"] {
        for origin in [
            None,
            Some("http://127.0.0.1:3888"),
            Some("tauri://localhost"),
        ] {
            let mut headers = vec![("host", "127.0.0.1:3888")];
            if let Some(origin) = origin {
                headers.push(("origin", origin));
            }
            headers.extend_from_slice(HANDSHAKE);
            let resp = pedido(None, &[], "GET", uri, &headers).await;
            let (status, corpo) = status_corpo(resp).await;
            assert_ne!(
                status,
                StatusCode::FORBIDDEN,
                "{uri} com Origin {origin:?} recusado: {corpo}"
            );
        }
    }
}

/// `GET` puro não é deste guarda: navegação cross-site (link para o console
/// a partir de outro site) e `GET` com `Origin` estranho passam — quem cuida
/// da resposta é o CORS, que sem `allowed_origins` não a entrega.
#[tokio::test]
async fn leitura_http_atravessa_a_guarda() {
    for headers in [
        vec![("host", "127.0.0.1:3888"), ("sec-fetch-site", "cross-site")],
        vec![
            ("host", "127.0.0.1:3888"),
            ("origin", "http://evil.example"),
        ],
    ] {
        let resp = pedido(None, &[], "GET", "/api/sessions", &headers).await;
        let (status, corpo) = status_corpo(resp).await;
        assert_ne!(status, StatusCode::FORBIDDEN, "{headers:?}: {corpo}");
    }
}

/// `allowed_origins: ["*"]` — o reflexo de quem quer o allow-all antigo —
/// não pode derrubar o gateway no boot (o `AllowOrigin::list` do tower-http
/// entra em pânico com `*`). A entrada é ignorada com aviso, e o default
/// seguro (nenhuma origem anunciada) vale.
#[tokio::test]
async fn curinga_em_allowed_origins_nao_derruba_o_boot() {
    let resp = pedido(
        None,
        &["*"],
        "GET",
        "/api/sessions",
        &[("origin", "http://evil.com"), ("host", "127.0.0.1:3888")],
    )
    .await;
    assert!(
        resp.headers().get("access-control-allow-origin").is_none(),
        "`*` foi anunciado como origem"
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
