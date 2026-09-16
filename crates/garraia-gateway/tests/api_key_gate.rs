//! #1045 + #1240: o gate de `gateway.api_key` sobre `/api/*` **e** sobre o
//! plano de conversa (`/v1/chat/completions`, `/v1/messages`,
//! `/v1/messages/count_tokens`) e o A2A (`/a2a/*`), montado no `build_router`
//! de verdade.
//!
//! Os testes de unidade em `gateway_auth.rs` provam a decisão do middleware
//! sobre um router de mentira. Só isto aqui prova a **montagem**: que o layer
//! cobre as rotas reais, incluindo as que `build_skill_skin_routes` e
//! `build_plugin_routes` montam sob `/api/` por caminhos próprios; que as
//! rotas de descoberta continuam abertas; e que o plano de conversa, montado
//! no mesmo router cru, passou a ser coberto. Um layer no lugar errado da
//! cadeia passaria nos testes de unidade e falharia aqui.
//!
//! **Por que o arquivo mudou de opinião sobre `/v1/*` (#1240).** Até esta
//! issue este teste dizia, no doc acima e em `fora_de_api_o_gate_nao_age`,
//! que "`/v1/*` fica de fora" — e prendia assim o fail-open: o operador
//! configurava `gateway.api_key` achando que tinha fechado a porta, e
//! `POST /v1/chat/completions` (que executa as tools do GarraIA na máquina
//! dele) seguia respondendo a qualquer `curl`. A justificativa original —
//! "`/v1/*` tem JWT próprio" — vale para o `rest_v1` do workspace, **não**
//! para as rotas compat OpenAI/Anthropic, que só compartilham o prefixo.
//! Quem for "consertar" isto de volta para um `starts_with("/api/")` único
//! está reabrindo a #1240: o recorte agora é por conjunto explícito.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
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
        // O gate nao toca em `/webhooks/*`; o que este teste exercita e o
        // layer sobre `/api/*`. Antes da #1079 eram quatro
        // `Arc::new(Vec::new())` posicionais distinguidos so por comentario.
        PushChannelStates::empty(),
        admin_store,
        Arc::new(vec![0u8; 32]),
    )
}

async fn status(chave: Option<&str>, uri: &str, auth: Option<&str>) -> StatusCode {
    resposta(chave, uri, auth).await.status()
}

async fn resposta(chave: Option<&str>, uri: &str, auth: Option<&str>) -> axum::response::Response {
    pedido(chave, Method::GET, uri, auth, None).await
}

/// A forma geral: método, `Authorization` e `x-api-key` (o header que o
/// Claude Code e o SDK da Anthropic mandam, e que só as rotas Anthropic
/// aceitam — #1240).
async fn pedido(
    chave: Option<&str>,
    metodo: Method,
    uri: &str,
    auth: Option<&str>,
    x_api_key: Option<&str>,
) -> axum::response::Response {
    let mut req = Request::builder().method(metodo).uri(uri);
    if let Some(a) = auth {
        req = req.header(header::AUTHORIZATION, a);
    }
    if let Some(k) = x_api_key {
        req = req.header("x-api-key", k);
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

async fn status_de(
    chave: Option<&str>,
    metodo: Method,
    uri: &str,
    auth: Option<&str>,
    x_api_key: Option<&str>,
) -> StatusCode {
    pedido(chave, metodo, uri, auth, x_api_key).await.status()
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

/// A descoberta continua sem credencial. `/v1/models` e
/// `/.well-known/agent.json` são como um cliente descobre o que há do outro
/// lado antes de ter a chave — o mesmo papel de `/api/capabilities`.
///
/// **#1240**: esta lista era `["/health", "/ping", "/v1/models"]` sob o nome
/// "fora de /api o gate não age", e o nome afirmava mais do que a lista.
/// Continuam abertas as mesmas rotas; o que mudou é que "fora de `/api/`"
/// deixou de ser sinônimo de "aberto". Ver o doc no topo do arquivo.
#[tokio::test]
async fn as_rotas_de_descoberta_continuam_abertas() {
    for aberta in ["/health", "/ping", "/v1/models", "/.well-known/agent.json"] {
        assert_ne!(
            status(Some(CHAVE), aberta, None).await,
            StatusCode::UNAUTHORIZED,
            "{aberta} foi gateado sem precisar"
        );
    }
}

// ── #1240: o plano de conversa e o A2A ────────────────────────────────────

/// As rotas que a #1240 trouxe para dentro do gate. São o router cru — não
/// o `rest_v1`, que tem JWT próprio e continua fora deste assunto.
const PLANO_DE_CONVERSA: &[(&str, &str)] = &[
    ("POST", "/v1/chat/completions"),
    ("POST", "/v1/messages"),
    ("POST", "/v1/messages/count_tokens"),
    ("POST", "/a2a/tasks"),
    ("GET", "/a2a/tasks/alguma-tarefa"),
    ("POST", "/a2a/tasks/alguma-tarefa/cancel"),
];

fn metodo(m: &str) -> Method {
    Method::from_bytes(m.as_bytes()).expect("metodo")
}

/// O coração da #1240: com a chave configurada, a superfície que executa
/// tools na máquina do dono deixa de responder a quem não a tem.
#[tokio::test]
async fn com_chave_o_plano_de_conversa_exige_credencial() {
    for (m, rota) in PLANO_DE_CONVERSA {
        let resp = pedido(Some(CHAVE), metodo(m), rota, None, None).await;
        assert_eq!(
            resp.status(),
            StatusCode::UNAUTHORIZED,
            "{m} {rota} respondeu sem a chave"
        );
        // Corpo constante: o 401 do gate nunca ecoa o que veio no pedido.
        assert_eq!(
            resp.headers().get(header::WWW_AUTHENTICATE).unwrap(),
            r#"Bearer realm="garraia""#
        );
        let bytes = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .expect("corpo");
        assert_eq!(&bytes[..], b"gateway: invalid or missing api key");
    }
}

#[tokio::test]
async fn com_chave_errada_o_plano_de_conversa_segue_fechado() {
    for (m, rota) in PLANO_DE_CONVERSA {
        assert_eq!(
            status_de(
                Some(CHAVE),
                metodo(m),
                rota,
                Some("Bearer nao-e-a-chave"),
                None
            )
            .await,
            StatusCode::UNAUTHORIZED,
            "{m} {rota} aceitou uma chave errada"
        );
    }
}

/// Com o bearer certo o gate sai da frente. O que vem depois é problema do
/// handler (415/400/404 com corpo vazio) — o que este teste trava é só que
/// não é mais 401.
#[tokio::test]
async fn com_bearer_valido_o_plano_de_conversa_passa_o_gate() {
    for (m, rota) in PLANO_DE_CONVERSA {
        assert_ne!(
            status_de(
                Some(CHAVE),
                metodo(m),
                rota,
                Some(&format!("Bearer {CHAVE}")),
                None
            )
            .await,
            StatusCode::UNAUTHORIZED,
            "{m} {rota} recusou o bearer certo"
        );
    }
}

/// O Claude Code e o SDK da Anthropic nunca mandam `Authorization: Bearer` —
/// mandam `x-api-key`. Sem isto, gatear `/v1/messages` quebraria a
/// integração documentada.
#[tokio::test]
async fn as_rotas_anthropic_aceitam_x_api_key() {
    for rota in ["/v1/messages", "/v1/messages/count_tokens"] {
        assert_ne!(
            status_de(Some(CHAVE), Method::POST, rota, None, Some(CHAVE)).await,
            StatusCode::UNAUTHORIZED,
            "{rota} recusou a chave em x-api-key"
        );
        assert_eq!(
            status_de(Some(CHAVE), Method::POST, rota, None, Some("errada")).await,
            StatusCode::UNAUTHORIZED,
            "{rota} aceitou x-api-key errada"
        );
    }
}

/// O `x-api-key` é uma concessão às rotas Anthropic, não um segundo header
/// de autenticação do gateway inteiro. Em `/api/*`, no OpenAI-compat e no
/// A2A ele não vale — lá a chave é `Authorization: Bearer`.
#[tokio::test]
async fn x_api_key_nao_vale_fora_das_rotas_anthropic() {
    for (m, rota) in [
        ("GET", "/api/sessions"),
        ("POST", "/v1/chat/completions"),
        ("POST", "/a2a/tasks"),
    ] {
        assert_eq!(
            status_de(Some(CHAVE), metodo(m), rota, None, Some(CHAVE)).await,
            StatusCode::UNAUTHORIZED,
            "{m} {rota} aceitou x-api-key"
        );
    }
}

/// A chave nunca entra por query string — o invariante do módulo, agora
/// também para as rotas novas.
#[tokio::test]
async fn a_chave_na_query_nao_abre_o_plano_de_conversa() {
    for uri in [
        "/v1/chat/completions?api_key=chave-de-teste-do-gateway",
        "/v1/messages?token=chave-de-teste-do-gateway",
        "/a2a/tasks?api_key=chave-de-teste-do-gateway",
    ] {
        assert_eq!(
            status_de(Some(CHAVE), Method::POST, uri, None, None).await,
            StatusCode::UNAUTHORIZED,
            "{uri} aceitou a chave pela query"
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

/// #1240 não fecha nada por default. Sem `gateway.api_key`, cada rota que
/// este PR trouxe para dentro do gate responde exatamente como antes — e
/// isso vale também para o `/v1/models` e o card do A2A.
#[tokio::test]
async fn sem_chave_configurada_o_plano_de_conversa_nao_muda() {
    for (m, rota) in PLANO_DE_CONVERSA {
        assert_ne!(
            status_de(None, metodo(m), rota, None, None).await,
            StatusCode::UNAUTHORIZED,
            "{m} {rota} passou a exigir chave sem haver chave configurada"
        );
    }
    for aberta in ["/v1/models", "/.well-known/agent.json"] {
        assert_ne!(
            status(None, aberta, None).await,
            StatusCode::UNAUTHORIZED,
            "{aberta} passou a exigir chave sem haver chave configurada"
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
