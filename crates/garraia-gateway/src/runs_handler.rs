//! `GET /api/runs` — leitura do ledger de runs de agente (#1227, slice 6).
//!
//! Somente leitura, e mais estrita que o resto de `/api/*` de proposito: e
//! rota nova, entao nada existente quebra por ela nascer fechada.
//!
//! # Quem pode ler
//!
//! 1. **Com `gateway.api_key`**: o gate global de `/api/*`
//!    (`crate::gateway_auth::api_key_layer`) ja exigiu o bearer por fora; o
//!    handler confere de novo com o mesmo [`ApiKeyGate::admits`] — a mesma
//!    funcao, nao uma segunda comparacao —, para que montar a rota fora do
//!    gate por engano nao a deixe aberta.
//! 2. **Sem chave** (o default), so o dono na propria maquina:
//!    - sem `ConnectInfo` -> `503 runs: peer address unavailable` (nunca se
//!      finge que e loopback);
//!    - peer de LAN/internet -> `503 runs: auth not configured`, o mesmo
//!      precedente do `/metrics` e das mutacoes de learning;
//!    - peer loopback com `Host` que nao e nome de loopback -> `403`. Vale
//!      **mesmo sem `Origin`**: uma pagina de DNS rebinding faz um GET
//!      same-origin, e GET same-origin nao manda `Origin`. So o `Host`
//!      `127.0.0.1`/`localhost`/`[::1]` distingue o console do alias;
//!    - peer loopback com `Origin` de outra origem -> `403`;
//!    - peer loopback com cabecalho de proxy (`X-Forwarded-For`,
//!      `X-Real-IP`, `Forwarded`, `X-Forwarded-Host`) -> `503 runs: auth not
//!      configured`. Atras de um proxy reverso local o peer e o proxy, nao o
//!      cliente do dono — e um proxy que reescreve o `Host` para o upstream
//!      (o default do nginx sem `proxy_set_header Host`) passaria nos dois
//!      testes acima com um cliente remoto.
//!
//!    Para ler de um celular na LAN, configure `gateway.api_key`.
//!
//! # O que sai
//!
//! `{"runs":[{id, session_id, mode, status, started_at, finished_at,
//! goal_preview, result_preview, error_preview}]}` — instantes em ISO 8601
//! UTC com `Z`, previas de no maximo [`PREVIEW_MAX`] caracteres, com
//! segredo de formato conhecido redigido (`garraia_security::redact_secrets`)
//! **antes** do corte (cortar primeiro deixaria meia chave curta demais para o
//! regex) e controle de terminal trocado por `U+FFFD`. Os trechos completos
//! de 500 caracteres do ledger **nao** saem por HTTP.
//!
//! `status` fora do conjunto -> `400` de corpo constante, que nao ecoa o
//! valor. `limit` default [`LIMITE_PADRAO`], preso em `[1, LIMITE_MAX]`. O
//! filtro de status e do SQL (`list_recent_agent_runs_by_status`).
//!
//! # Log
//!
//! Este modulo nunca loga `goal` nem trecho de resultado (CLAUDE.md §6) — ha
//! teste varrendo o fonte.

use std::net::{IpAddr, SocketAddr};

use axum::Json;
use axum::extract::{ConnectInfo, Query, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use garraia_db::AgentRunRow;
use garraia_db::agent_runs::{iso8601_utc, parse_run_status, preview, sanitize_control_chars};
use serde::Deserialize;
use tracing::warn;

use crate::auth_common::extract_bearer;
use crate::gateway_auth::ApiKeyGate;
use crate::origin_guard::{Pedido, cross_origin, esquema_efetivo, host_de_loopback};
use crate::state::SharedState;

/// Quantos runs quando ninguem pede outra coisa.
pub const LIMITE_PADRAO: u32 = 20;
/// Teto de `limit`.
pub const LIMITE_MAX: u32 = 200;
/// Tamanho maximo de cada previa, contando o `…` do corte.
pub const PREVIEW_MAX: usize = 120;

const CORPO_401: &str = "runs: invalid or missing api key";
const CORPO_SEM_PEER: &str = "runs: peer address unavailable";
const CORPO_SEM_AUTH: &str = "runs: auth not configured";
const CORPO_HOST: &str = "runs: loopback Host required without gateway.api_key";
const CORPO_ORIGEM: &str = "runs: cross-origin request refused";
const CORPO_QUERY: &str = "runs: invalid query (status must be one of running, done, error, cancelled, interrupted; limit must be a non-negative integer)";

/// Cabecalhos que um proxy reverso acrescenta. Qualquer um deles num pedido
/// de peer loopback sem `gateway.api_key` significa "o peer e um proxy", e o
/// cliente real pode estar em qualquer lugar.
const CABECALHOS_DE_PROXY: [&str; 4] = [
    "x-forwarded-for",
    "x-real-ip",
    "forwarded",
    "x-forwarded-host",
];
const CORPO_SEM_LEDGER: &str = "runs: ledger unavailable";
const CORPO_FALHA: &str = "runs: ledger read failed";

#[derive(Debug, Deserialize)]
struct RunsQuery {
    status: Option<String>,
    limit: Option<u64>,
}

fn negar(status: StatusCode, corpo: &'static str) -> Response {
    (status, Json(serde_json::json!({ "error": corpo }))).into_response()
}

/// A decisao de acesso, pura sobre o pedido. `None` = pode ler.
fn recusa(state: &SharedState, req: &Request) -> Option<Response> {
    let gate = ApiKeyGate::from_config(&state.config.gateway);
    if gate.is_enabled() {
        if gate.admits(extract_bearer(req.headers())) {
            return None;
        }
        return Some(negar(StatusCode::UNAUTHORIZED, CORPO_401));
    }

    let peer: Option<IpAddr> = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(sa)| sa.ip());
    match peer {
        None => {
            warn!("runs: peer address unavailable (no ConnectInfo)");
            Some(negar(StatusCode::SERVICE_UNAVAILABLE, CORPO_SEM_PEER))
        }
        Some(ip) if ip.is_loopback() => {
            if CABECALHOS_DE_PROXY
                .iter()
                .any(|h| req.headers().contains_key(*h))
            {
                warn!("runs: loopback peer carrying proxy headers with no auth configured");
                return Some(negar(StatusCode::SERVICE_UNAVAILABLE, CORPO_SEM_AUTH));
            }
            let pedido = Pedido::de(req.headers(), req.uri());
            if !host_de_loopback(&pedido) {
                warn!("runs: loopback peer behind a non-loopback Host (DNS rebinding?)");
                return Some(negar(StatusCode::FORBIDDEN, CORPO_HOST));
            }
            if pedido.tem_origin() && cross_origin(&pedido, esquema_efetivo(&state.config.gateway))
            {
                warn!("runs: cross-origin read refused");
                return Some(negar(StatusCode::FORBIDDEN, CORPO_ORIGEM));
            }
            None
        }
        Some(_) => {
            warn!("runs: non-loopback peer with no auth configured");
            Some(negar(StatusCode::SERVICE_UNAVAILABLE, CORPO_SEM_AUTH))
        }
    }
}

/// Previa de um campo do ledger: redige, corta, higieniza.
fn previa(texto: &str) -> String {
    let redigido = garraia_security::redact_secrets(texto);
    // `preview` acrescenta `…` quando corta: `PREVIEW_MAX - 1` deixa o total
    // dentro do teto.
    preview(&redigido, PREVIEW_MAX - 1)
}

fn run_json(run: &AgentRunRow) -> serde_json::Value {
    serde_json::json!({
        "id": sanitize_control_chars(&run.id),
        "session_id": run.session_id.as_deref().map(sanitize_control_chars),
        "mode": run.mode.as_deref().map(sanitize_control_chars),
        "status": run.status.as_str(),
        "started_at": iso8601_utc(&run.started_at),
        "finished_at": run.finished_at.as_deref().map(iso8601_utc),
        "goal_preview": previa(&run.goal),
        "result_preview": run.result_snippet.as_deref().map(previa),
        "error_preview": run.error_snippet.as_deref().map(previa),
    })
}

/// GET /api/runs
pub async fn list_runs(State(state): State<SharedState>, req: Request) -> Response {
    if let Some(resp) = recusa(&state, &req) {
        return resp;
    }

    let Ok(Query(q)) = Query::<RunsQuery>::try_from_uri(req.uri()) else {
        return negar(StatusCode::BAD_REQUEST, CORPO_QUERY);
    };
    let filtro = match q.status.as_deref() {
        None => None,
        Some(bruto) => match parse_run_status(bruto) {
            Some(s) => Some(s),
            None => return negar(StatusCode::BAD_REQUEST, CORPO_QUERY),
        },
    };
    let limite = q
        .limit
        .map(|l| l.clamp(1, u64::from(LIMITE_MAX)))
        .and_then(|l| u32::try_from(l).ok())
        .unwrap_or(LIMITE_PADRAO);

    let Some(store) = state.session_store.as_ref() else {
        return negar(StatusCode::SERVICE_UNAVAILABLE, CORPO_SEM_LEDGER);
    };
    let lidos = {
        let store = store.lock().await;
        match &filtro {
            Some(s) => store.list_recent_agent_runs_by_status(s, limite),
            None => store.list_recent_agent_runs(limite),
        }
    };
    match lidos {
        Ok(runs) => {
            let runs: Vec<_> = runs.iter().map(run_json).collect();
            (StatusCode::OK, Json(serde_json::json!({ "runs": runs }))).into_response()
        }
        Err(e) => {
            warn!(erro = %e, "runs: leitura do ledger falhou");
            negar(StatusCode::INTERNAL_SERVER_ERROR, CORPO_FALHA)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::{Body, to_bytes};
    use axum::routing::get;
    use garraia_agents::AgentRuntime;
    use garraia_channels::ChannelRegistry;
    use garraia_config::AppConfig;
    use garraia_db::{RunStatus, SessionStore};
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use tower::ServiceExt;

    const CHAVE: &str = "chave-de-teste-do-ledger";

    fn estado(chave: Option<&str>, store: Option<SessionStore>) -> SharedState {
        let mut config = AppConfig::default();
        config.gateway.api_key = chave.map(str::to_string);
        let mut st = crate::state::AppState::new(
            config,
            Arc::new(AgentRuntime::new()),
            ChannelRegistry::new(),
        );
        if let Some(s) = store {
            st.set_session_store(Arc::new(Mutex::new(s)));
        }
        Arc::new(st)
    }

    fn store_semeado() -> SessionStore {
        let st = SessionStore::in_memory().expect("store");
        st.start_agent_run("r-feito", Some("s-1"), "rodar o build", Some("heartbeat"))
            .expect("abre");
        st.finish_agent_run("r-feito", RunStatus::Done, Some("verde"), None)
            .expect("fecha");
        st
    }

    struct Pedir<'a> {
        uri: &'a str,
        peer: Option<[u8; 4]>,
        headers: Vec<(&'a str, &'a str)>,
    }

    impl<'a> Pedir<'a> {
        fn local(uri: &'a str) -> Self {
            Self {
                uri,
                peer: Some([127, 0, 0, 1]),
                headers: vec![("host", "127.0.0.1:3888")],
            }
        }
    }

    async fn chamar(state: SharedState, p: Pedir<'_>) -> (StatusCode, serde_json::Value) {
        let router = Router::new()
            .route("/api/runs", get(list_runs))
            .with_state(state);
        let mut b = axum::http::Request::builder().method("GET").uri(p.uri);
        for (k, v) in &p.headers {
            b = b.header(*k, *v);
        }
        let mut req = b.body(Body::empty()).expect("req");
        if let Some(ip) = p.peer {
            req.extensions_mut()
                .insert(ConnectInfo(SocketAddr::from((ip, 40404))));
        }
        let resp = router.oneshot(req).await.expect("resp");
        let status = resp.status();
        let corpo = to_bytes(resp.into_body(), usize::MAX).await.expect("corpo");
        let v = serde_json::from_slice(&corpo).unwrap_or(serde_json::Value::Null);
        (status, v)
    }

    #[tokio::test]
    async fn sem_chave_console_local_le() {
        for host in ["127.0.0.1:3888", "localhost:3888", "localhost"] {
            let mut p = Pedir::local("/api/runs");
            p.headers = vec![("host", host)];
            let (st, v) = chamar(estado(None, Some(store_semeado())), p).await;
            assert_eq!(st, StatusCode::OK, "{host}: {v}");
            assert_eq!(v["runs"].as_array().expect("array").len(), 1);
        }
        // Console servido pelo proprio gateway: Origin igual ao Host passa.
        let mut p = Pedir::local("/api/runs");
        p.headers.push(("origin", "http://127.0.0.1:3888"));
        let (st, _) = chamar(estado(None, Some(store_semeado())), p).await;
        assert_eq!(st, StatusCode::OK);
    }

    #[tokio::test]
    async fn sem_chave_sem_connect_info_e_503() {
        let mut p = Pedir::local("/api/runs");
        p.peer = None;
        let (st, v) = chamar(estado(None, Some(store_semeado())), p).await;
        assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(v["error"], CORPO_SEM_PEER);
    }

    #[tokio::test]
    async fn sem_chave_peer_de_lan_e_503() {
        let mut p = Pedir::local("/api/runs");
        p.peer = Some([192, 168, 0, 10]);
        p.headers = vec![("host", "192.168.0.2:3888")];
        let (st, v) = chamar(estado(None, Some(store_semeado())), p).await;
        assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(v["error"], CORPO_SEM_AUTH);
        assert!(v.get("runs").is_none());
    }

    /// DNS rebinding: o navegador conecta em 127.0.0.1, mas o `Host` e o
    /// dominio do atacante — com e sem `Origin`.
    #[tokio::test]
    async fn sem_chave_host_de_rebinding_e_403_mesmo_sem_origin() {
        for com_origin in [false, true] {
            let mut p = Pedir::local("/api/runs");
            p.headers = vec![("host", "evil.example:3888")];
            if com_origin {
                p.headers.push(("origin", "http://evil.example:3888"));
            }
            let (st, v) = chamar(estado(None, Some(store_semeado())), p).await;
            assert_eq!(st, StatusCode::FORBIDDEN, "origin={com_origin}");
            assert_eq!(v["error"], CORPO_HOST);
        }
    }

    /// Proxy reverso local que reescreve o `Host` para o upstream (nginx sem
    /// `proxy_set_header Host`): peer 127.0.0.1, `Host` de loopback, sem
    /// `Origin` — so o cabecalho de proxy denuncia o cliente remoto.
    #[tokio::test]
    async fn sem_chave_atras_de_proxy_e_503() {
        for (cabecalho, valor) in [
            ("x-forwarded-for", "203.0.113.9"),
            ("x-real-ip", "203.0.113.9"),
            ("forwarded", "for=203.0.113.9"),
            ("x-forwarded-host", "garraia.example"),
        ] {
            let mut p = Pedir::local("/api/runs");
            p.headers.push((cabecalho, valor));
            let (st, v) = chamar(estado(None, Some(store_semeado())), p).await;
            assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE, "{cabecalho}: {v}");
            assert_eq!(v["error"], CORPO_SEM_AUTH);
            assert!(v.get("runs").is_none());
        }
    }

    /// Com a chave, o proxy e o caminho suportado: o bearer decide.
    #[tokio::test]
    async fn com_chave_atras_de_proxy_le_com_bearer() {
        let bearer = format!("Bearer {CHAVE}");
        let mut p = Pedir::local("/api/runs");
        p.headers.push(("x-forwarded-for", "203.0.113.9"));
        p.headers.push(("authorization", &bearer));
        let (st, v) = chamar(estado(Some(CHAVE), Some(store_semeado())), p).await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }

    #[tokio::test]
    async fn sem_chave_origin_de_outra_pagina_e_403() {
        let mut p = Pedir::local("/api/runs");
        p.headers.push(("origin", "https://evil.example"));
        let (st, v) = chamar(estado(None, Some(store_semeado())), p).await;
        assert_eq!(st, StatusCode::FORBIDDEN);
        assert_eq!(v["error"], CORPO_ORIGEM);
    }

    #[tokio::test]
    async fn com_chave_exige_bearer_ate_do_loopback() {
        let (st, v) = chamar(
            estado(Some(CHAVE), Some(store_semeado())),
            Pedir::local("/api/runs"),
        )
        .await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);
        assert!(v.get("runs").is_none());

        let bearer = format!("Bearer {CHAVE}");
        let mut p = Pedir::local("/api/runs");
        // Com a chave, a LAN le — e o motivo de configura-la.
        p.peer = Some([192, 168, 0, 10]);
        p.headers = vec![("host", "192.168.0.2:3888"), ("authorization", &bearer)];
        let (st, v) = chamar(estado(Some(CHAVE), Some(store_semeado())), p).await;
        assert_eq!(st, StatusCode::OK, "{v}");

        let mut p = Pedir::local("/api/runs");
        p.headers.push(("authorization", "Bearer errada"));
        let (st, _) = chamar(estado(Some(CHAVE), Some(store_semeado())), p).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn status_desconhecido_e_400_sem_eco() {
        for uri in [
            "/api/runs?status=bogus%3Cscript%3E",
            "/api/runs?limit=abc",
            "/api/runs?limit=-1",
        ] {
            let (st, v) = chamar(estado(None, Some(store_semeado())), Pedir::local(uri)).await;
            assert_eq!(st, StatusCode::BAD_REQUEST, "{uri}");
            assert_eq!(v["error"], CORPO_QUERY);
            let texto = v.to_string();
            assert!(
                !texto.contains("bogus") && !texto.contains("abc"),
                "{texto}"
            );
        }
    }

    #[tokio::test]
    async fn limit_e_preso_e_status_filtra_no_sql() {
        let st = SessionStore::in_memory().expect("store");
        for i in 0..205 {
            let id = format!("done-{i:03}");
            st.start_agent_run(&id, None, "g", None).expect("abre");
            st.finish_agent_run(&id, RunStatus::Done, None, None)
                .expect("fecha");
        }
        for i in 0..5 {
            st.start_agent_run(&format!("vivo-{i}"), None, "g", None)
                .expect("abre");
        }
        garraia_db::agent_runs::log_interrupted_runs(&st);
        let state = estado(None, Some(st));

        let n = |v: &serde_json::Value| v["runs"].as_array().expect("array").len();
        let (_, v) = chamar(state.clone(), Pedir::local("/api/runs?limit=0")).await;
        assert_eq!(n(&v), 1);
        let (_, v) = chamar(state.clone(), Pedir::local("/api/runs?limit=10000")).await;
        assert_eq!(n(&v), LIMITE_MAX as usize);
        let (_, v) = chamar(state.clone(), Pedir::local("/api/runs")).await;
        assert_eq!(n(&v), LIMITE_PADRAO as usize);
        let (_, v) = chamar(
            state.clone(),
            Pedir::local("/api/runs?status=interrupted&limit=50"),
        )
        .await;
        assert_eq!(n(&v), 5);
        assert!(
            v["runs"]
                .as_array()
                .expect("array")
                .iter()
                .all(|r| r["status"] == "interrupted")
        );
    }

    #[tokio::test]
    async fn previas_curtas_higienizadas_redigidas_e_instantes_com_z() {
        let st = SessionStore::in_memory().expect("store");
        let chave = "sk-ant-api03-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let goal = format!("\x1b[2Klinha\r\num {chave} {}", "x".repeat(400));
        st.start_agent_run("r", None, &goal, None).expect("abre");
        st.finish_agent_run("r", RunStatus::Error, None, Some(&format!("401 {chave}")))
            .expect("fecha");
        let (status, v) = chamar(estado(None, Some(st)), Pedir::local("/api/runs")).await;
        assert_eq!(status, StatusCode::OK);
        let run = &v["runs"][0];
        let previa = run["goal_preview"].as_str().expect("texto");
        assert!(
            previa.chars().count() <= PREVIEW_MAX,
            "{}",
            previa.chars().count()
        );
        for c in ['\x1b', '\r', '\n'] {
            assert!(!previa.contains(c), "{previa:?}");
        }
        let texto = v.to_string();
        assert!(!texto.contains("AAAAAAAAAAAAAAAA"), "chave vazou: {texto}");
        assert!(
            run["error_preview"]
                .as_str()
                .expect("texto")
                .contains("[REDACTED]")
        );
        assert!(run["result_preview"].is_null());
        assert!(run["started_at"].as_str().expect("t").ends_with('Z'));
        assert!(run["finished_at"].as_str().expect("t").ends_with('Z'));
        // O trecho completo do ledger nao sai por HTTP.
        for chave in ["goal", "result_snippet", "error_snippet"] {
            assert!(run.get(chave).is_none(), "{chave} exposto");
        }
    }

    #[tokio::test]
    async fn sem_store_e_503() {
        let (st, v) = chamar(estado(None, None), Pedir::local("/api/runs")).await;
        assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(v["error"], CORPO_SEM_LEDGER);
    }

    /// A rota esta montada no router de verdade, atras do gate global: com a
    /// chave e sem bearer, 401 do gate (corpo do gate, nao o deste modulo).
    #[tokio::test]
    async fn montada_no_router_real_atras_do_gate() {
        let state = estado(Some(CHAVE), Some(store_semeado()));
        let admin = Arc::new(Mutex::new(
            crate::admin::store::AdminStore::in_memory().expect("admin"),
        ));
        let router = crate::router::build_router(
            state,
            crate::push_channels::PushChannelStates::empty(),
            admin,
            Arc::new(vec![0u8; 32]),
        );
        let pedir = |auth: Option<String>| {
            let mut b = axum::http::Request::builder()
                .method("GET")
                .uri("/api/runs")
                .header("host", "127.0.0.1:3888");
            if let Some(a) = auth {
                b = b.header("authorization", a);
            }
            let mut req = b.body(Body::empty()).expect("req");
            req.extensions_mut()
                .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40404))));
            req
        };
        let resp = router.clone().oneshot(pedir(None)).await.expect("resp");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let corpo = to_bytes(resp.into_body(), usize::MAX).await.expect("corpo");
        assert!(
            !String::from_utf8_lossy(&corpo).contains("runs:"),
            "o 401 tem de ser o do gate global, que roda antes do handler"
        );

        let resp = router
            .oneshot(pedir(Some(format!("Bearer {CHAVE}"))))
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// O handler nunca loga conteudo do ledger. `concat!` impede o teste de
    /// casar consigo mesmo.
    #[test]
    fn log_nunca_carrega_conteudo_do_ledger() {
        let src = include_str!("runs_handler.rs");
        let fim = src.find(concat!("#[cfg(", "test)]")).expect("testes");
        let producao = &src[..fim];
        for linha in producao
            .lines()
            .filter(|l| l.contains(concat!("warn", "!(")))
        {
            for agulha in ["goal", "snippet", "preview", "run."] {
                assert!(!linha.contains(agulha), "log com conteudo: {linha}");
            }
        }
        for agulha in [
            concat!("info", "!("),
            concat!("debug", "!("),
            concat!("lo", "g::"),
        ] {
            assert!(!producao.contains(agulha), "{agulha}");
        }
    }
}
