//! Guarda das rotas **mutantes** de `/api/learning/*` (#1093).
//!
//! As rotas de learning executam `git` no repositório do dono: `POST
//! /api/learning/skills/{name}/rollback` roda `git revert` com `repo_root =
//! std::env::current_dir()`, e approve/reject/lock/delete escrevem os arquivos
//! das skills. Até este módulo, o único gate em `/api/*` era o de
//! `gateway.api_key` — que, **desligado por default** (`api_key` ausente), era
//! um passa-direto. Aliado ao CORS `allow_origin(Any)` do dev mode, uma página
//! web qualquer visitada pelo dono disparava o `POST` de dentro do navegador
//! contra `127.0.0.1:3888`: o preflight passava (`Any` + `Any`), e "o peer é
//! loopback" não resolve — quem está no loopback é o navegador da vítima.
//!
//! ## O que este middleware faz
//!
//! Aplicado **só** às rotas de learning (sub-router em `build_router`), para
//! métodos `POST`/`DELETE`/`PATCH`/`PUT`, nesta ordem:
//!
//! 1. **Anti-CSRF de navegador.** `Origin` de esquema diferente do do
//!    transporte (`https` contra um gateway http, ou um esquema exótico),
//!    `Origin` com authority diferente do header `Host`, `Origin: null`,
//!    `Sec-Fetch-Site: cross-site` sem `Origin`, ou qualquer valor
//!    malformado — todos viram `403` de corpo fixo. `Origin` do mesmo
//!    esquema e mesma authority do `Host` (o console web servido pelo
//!    próprio gateway, porta default normalizada) passa.
//! 2. **Fail-closed sem credencial.** Com o gate de `gateway.api_key`
//!    **desligado**, só peer loopback passa; peer de LAN/internet recebe
//!    `503 learning: auth not configured` — o mesmo precedente do
//!    `/metrics` (`crate::metrics_auth`). Com a chave configurada, o gate
//!    global de `/api/*` já exigiu o bearer **por fora** deste layer (layers
//!    do router pai rodam antes dos de rota), então aqui não se revalida
//!    token — duplicar a comparação seria duas implementações para divergir
//!    em silêncio.
//! 3. **Sem `ConnectInfo`** (request sintético, socket sem peer): não se
//!    finge que é loopback — `503 learning: peer address unavailable`,
//!    mesmo raciocínio do comentário do `metrics_auth_layer` sobre allowlist
//!    sem peer.
//!
//! `GET`/`HEAD`/`OPTIONS` não são afetados: leitura segue sob o gate global
//! de `/api/*` como antes. Corpos de resposta são `&'static str` fixos — nada
//! do pedido (token, Origin, sha) é ecoado; o log leva no máximo caminho e
//! método.
//!
//! ## O que este guarda **não** é
//!
//! O passo 1 compara o `Origin` contra o esquema do transporte e a authority
//! do header `Host` (com porta default normalizada). `Host` é controlado por
//! quem faz o pedido, então um cliente que já alcança a porta e
//! não é um navegador (`curl -H "Origin: http://x" -H "Host: x"`) consegue
//! fazer os dois casarem. Isso não é o cenário que este módulo fecha — o
//! ataque é a página visitada pelo dono, onde o navegador fixa `Origin` e
//! `Host` e nenhum dos dois é do atacante. **Contra um cliente que já executa
//! código na máquina, a única proteção é `gateway.api_key`**, que continua
//! sendo o gate de verdade para todo `/api/*`. Este guarda é a camada que
//! falta quando essa chave não existe.

use std::net::{IpAddr, SocketAddr};

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tracing::warn;

use crate::gateway_auth::ApiKeyGate;

/// Corpo do 403 do anti-CSRF. Constante: nada do que veio no pedido é ecoado.
const CORPO_CSRF: &str = "learning: cross-origin mutating request refused";

/// Corpo do 503 fail-closed: peer não-loopback e nenhuma credencial
/// configurada.
const CORPO_SEM_AUTH: &str = "learning: auth not configured";

/// Corpo do 503 quando o IP do peer não está disponível. Nunca se finge que
/// é loopback.
const CORPO_SEM_PEER: &str = "learning: peer address unavailable";

/// `true` quando o pedido mutante chega de uma origem de navegador que não
/// é a do próprio gateway.
///
/// Um POST legítimo do console web (servido em `/learning`) traz `Origin`
/// com o mesmo esquema do transporte e a mesma authority do header `Host`
/// — porta default (`:80` em http, `:443` em https) normalizada dos dois
/// lados. Um POST do app (Dio) ou do `curl` não traz `Origin` nenhum. O que
/// **não** é legítimo: `Origin` de outro esquema (`https` contra um gateway
/// http, ou um esquema exótico), `Origin` de outra authority, `Origin: null`
/// (sandbox/redirect), ou `Sec-Fetch-Site: cross-site` sem `Origin`. Origem
/// malformada ou `Host` ausente são tratados como cross-origin — fail-closed,
/// e nunca se ecoa o valor rejeitado.
fn cross_origin(headers: &HeaderMap, scheme: &str) -> bool {
    match headers.get(header::ORIGIN) {
        Some(origem) => {
            let Ok(origem) = origem.to_str() else {
                return true; // não-ASCII no Origin: fail-closed
            };
            if origem.eq_ignore_ascii_case("null") {
                return true;
            }
            let Some((esquema, resto)) = origem.split_once("://") else {
                return true; // sem esquema, o valor não é uma origem válida
            };
            // O esquema do Origin tem de ser o do transporte: `https` contra
            // um gateway http não é a origem do console que passa aqui.
            if !esquema.eq_ignore_ascii_case(scheme) {
                return true;
            }
            let authority = resto.split('/').next().unwrap_or("");
            if authority.is_empty() {
                return true;
            }
            match headers.get(header::HOST).and_then(|h| h.to_str().ok()) {
                // Sem Host não há como confirmar mesma origem — fail-closed.
                None => true,
                Some(host) => !mesma_authority(authority, host, scheme),
            }
        }
        None => headers
            .get("sec-fetch-site")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.eq_ignore_ascii_case("cross-site")),
    }
}

/// Compara a authority do `Origin` com a do header `Host`, normalizando a
/// porta default dos dois lados: `:80` em http e `:443` em https não mudam
/// a origem. A porta é o sufixo após o **último** `:` e só conta como porta
/// se for toda dígito — IPv6 serializa com colchetes (`[::1]:3888`) e os
/// `:` internos não são porta.
fn mesma_authority(origin: &str, host: &str, scheme: &str) -> bool {
    let porta_default: &str = if scheme.eq_ignore_ascii_case("https") {
        ":443"
    } else {
        ":80"
    };
    let normaliza = |authority: &str| {
        if let Some((antes, porta)) = authority.rsplit_once(':')
            && !porta.is_empty()
            && porta.bytes().all(|b| b.is_ascii_digit())
            && authority.ends_with(porta_default)
        {
            return antes.to_string();
        }
        authority.to_string()
    };
    normaliza(origin).eq_ignore_ascii_case(&normaliza(host))
}

/// Estado da guarda: o gate global (para saber se a porta é autenticada) e o
/// esquema efetivo do transporte — `https` só quando o TLS nativo está
/// configurado, o mesmo critério de `session_cookie_secure`/`use_tls`.
/// Montado no `build_router`.
#[derive(Clone)]
pub struct LearningGuardState {
    pub gate: ApiKeyGate,
    /// Esquema efetivo do transporte (`"http"` | `"https"`).
    pub scheme: &'static str,
}

/// O middleware. Montado como layer do sub-router das rotas de learning em
/// `build_router` — por dentro do gate global de `/api/*` e por dentro do
/// CORS, que responde o preflight `OPTIONS` antes de qualquer guarda.
pub async fn learning_mutations_guard(
    State(estado): State<LearningGuardState>,
    req: Request,
    next: Next,
) -> Response {
    // Leitura não é afetada: GET/HEAD/OPTIONS seguem sob o gate global de
    // `/api/*` como antes deste módulo.
    let method = req.method().clone();
    if matches!(method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return next.run(req).await;
    }

    // 1. Anti-CSRF: um POST/DELETE só entra da origem do próprio gateway.
    if cross_origin(req.headers(), estado.scheme) {
        warn!(
            path = %req.uri().path(),
            method = %method,
            "learning: cross-origin mutating request refused"
        );
        return deny(StatusCode::FORBIDDEN, CORPO_CSRF);
    }

    // 2. Com a chave configurada, o gate global já autenticou o bearer —
    //    layers do router pai rodam antes deste, que é layer de rota. Não
    //    revalida: reimplementar a comparação seria uma segunda cópia para
    //    divergir.
    if estado.gate.is_enabled() {
        return next.run(req).await;
    }

    // 3. Gate desligado: só o loopback do próprio dono passa.
    let peer_ip: Option<IpAddr> = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(sa)| sa.ip());
    match peer_ip {
        // Mesmo raciocínio do `metrics_auth`: sem `ConnectInfo` não se pode
        // dizer quem é o peer — e responder como loopback seria fingir.
        None => {
            warn!(
                path = %req.uri().path(),
                method = %method,
                "learning: peer address unavailable (no ConnectInfo)"
            );
            deny(StatusCode::SERVICE_UNAVAILABLE, CORPO_SEM_PEER)
        }
        Some(ip) if ip.is_loopback() => next.run(req).await,
        Some(_) => {
            warn!(
                path = %req.uri().path(),
                method = %method,
                "learning: non-loopback peer with no auth configured"
            );
            deny(StatusCode::SERVICE_UNAVAILABLE, CORPO_SEM_AUTH)
        }
    }
}

fn deny(status: StatusCode, body: &'static str) -> Response {
    (status, body).into_response()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use axum::middleware::from_fn_with_state;
    use axum::routing::{delete, get, post};
    use http_body_util::BodyExt;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use tower::ServiceExt;

    async fn mutante() -> &'static str {
        "ok-mutante"
    }

    async fn leitura() -> &'static str {
        "ok-leitura"
    }

    fn router(estado: LearningGuardState) -> Router {
        Router::new()
            .route("/api/learning/skills", get(leitura))
            // PATCH e PUT entram aqui para o guarda ser coberto nos quatro
            // metodos que ele trava, nao so nos dois que existem hoje: uma
            // rota mutante nova em qualquer um deles ja nasce protegida.
            .route(
                "/api/learning/skills/x",
                delete(mutante).patch(mutante).put(mutante),
            )
            .route("/api/learning/skills/x/rollback", post(mutante))
            .layer(from_fn_with_state(estado, learning_mutations_guard))
    }

    fn gate_com(chave: Option<&str>) -> ApiKeyGate {
        ApiKeyGate::from_config(&garraia_config::GatewayConfig {
            api_key: chave.map(str::to_string),
            ..Default::default()
        })
    }

    /// Estado padrão dos testes: gateway http sem TLS nativo (o default da
    /// instalação) e a chave opcional.
    fn estado_com(chave: Option<&str>) -> LearningGuardState {
        LearningGuardState {
            gate: gate_com(chave),
            scheme: "http",
        }
    }

    /// Request sintético com peer, headers e corpo opcionais.
    fn request(
        method: Method,
        uri: &str,
        peer: Option<IpAddr>,
        headers: &[(&str, &str)],
    ) -> HttpRequest<Body> {
        let mut builder = HttpRequest::builder().uri(uri).method(method);
        for (nome, valor) in headers {
            builder = builder.header(*nome, *valor);
        }
        let mut req = builder.body(Body::empty()).unwrap();
        if let Some(ip) = peer {
            req.extensions_mut()
                .insert(ConnectInfo(SocketAddr::new(ip, 55555)));
        }
        req
    }

    async fn status_corpo(router: Router, req: HttpRequest<Body>) -> (StatusCode, String) {
        let resp = router.oneshot(req).await.expect("oneshot");
        let status = resp.status();
        let body = resp
            .into_body()
            .collect()
            .await
            .expect("collect body")
            .to_bytes();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
    /// O `is_loopback()` do std cobre `::1`; o teste explicita isso para um
    /// futuro refactor para `== Ipv4Addr::LOCALHOST` nao passar batido.
    const LOOPBACK6: IpAddr = IpAddr::V6(Ipv6Addr::LOCALHOST);
    const LAN: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));

    /// Os quatro metodos que o guarda trava, com a URI que os serve.
    const MUTANTES: [(Method, &str); 4] = [
        (Method::POST, "/api/learning/skills/x/rollback"),
        (Method::DELETE, "/api/learning/skills/x"),
        (Method::PATCH, "/api/learning/skills/x"),
        (Method::PUT, "/api/learning/skills/x"),
    ];

    // ── 1. mutante loopback, sem Origin, sem chave → passa ────────────────

    #[tokio::test]
    async fn mutante_loopback_sem_origin_passa() {
        let router = router(estado_com(None));
        for (metodo, uri) in MUTANTES {
            for peer in [LOOPBACK, LOOPBACK6] {
                let req = request(metodo.clone(), uri, Some(peer), &[]);
                let (status, corpo) = status_corpo(router.clone(), req).await;
                assert_eq!(status, StatusCode::OK, "{metodo} de {peer} deveria passar");
                assert_eq!(corpo, "ok-mutante");
            }
        }
    }

    // ── 2. mutante não-loopback, sem chave → 503 fail-closed ──────────────

    #[tokio::test]
    async fn mutante_nao_loopback_sem_chave_da_503() {
        let req = request(
            Method::POST,
            "/api/learning/skills/x/rollback",
            Some(LAN),
            &[],
        );
        let (status, corpo) = status_corpo(router(estado_com(None)), req).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(corpo, "learning: auth not configured");
    }

    // ── 3. Origin de outro host → 403, mesmo com peer loopback ────────────

    #[tokio::test]
    async fn origin_de_outro_host_da_403() {
        let req = request(
            Method::POST,
            "/api/learning/skills/x/rollback",
            Some(LOOPBACK),
            &[
                (header::HOST.as_str(), "127.0.0.1:3888"),
                (header::ORIGIN.as_str(), "http://evil.example"),
            ],
        );
        let (status, corpo) = status_corpo(router(estado_com(None)), req).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(corpo, CORPO_CSRF);
    }

    // ── 3b. os quatro metodos mutantes, nao so os dois de hoje ─────────────

    #[tokio::test]
    async fn cross_origin_da_403_nos_quatro_metodos_mutantes() {
        for (metodo, uri) in MUTANTES {
            let req = request(
                metodo.clone(),
                uri,
                Some(LOOPBACK),
                &[
                    (header::HOST.as_str(), "127.0.0.1:3888"),
                    (header::ORIGIN.as_str(), "http://evil.example"),
                ],
            );
            let (status, corpo) = status_corpo(router(estado_com(None)), req).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{metodo} cross-origin");
            assert_eq!(corpo, CORPO_CSRF);
        }
    }

    #[tokio::test]
    async fn nao_loopback_da_503_nos_quatro_metodos_mutantes() {
        for (metodo, uri) in MUTANTES {
            let req = request(metodo.clone(), uri, Some(LAN), &[]);
            let (status, corpo) = status_corpo(router(estado_com(None)), req).await;
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{metodo} de LAN");
            assert_eq!(corpo, CORPO_SEM_AUTH);
        }
    }

    // ── 4. Origin: null → 403 ─────────────────────────────────────────────

    #[tokio::test]
    async fn origin_null_da_403() {
        let req = request(
            Method::POST,
            "/api/learning/skills/x/rollback",
            Some(LOOPBACK),
            &[
                (header::HOST.as_str(), "127.0.0.1:3888"),
                (header::ORIGIN.as_str(), "null"),
            ],
        );
        let (status, corpo) = status_corpo(router(estado_com(None)), req).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(corpo, CORPO_CSRF);
    }

    // ── 5. Sec-Fetch-Site: cross-site sem Origin → 403 ────────────────────

    #[tokio::test]
    async fn sec_fetch_site_cross_site_da_403() {
        let req = request(
            Method::POST,
            "/api/learning/skills/x/rollback",
            Some(LOOPBACK),
            &[
                (header::HOST.as_str(), "127.0.0.1:3888"),
                ("sec-fetch-site", "cross-site"),
            ],
        );
        let (status, corpo) = status_corpo(router(estado_com(None)), req).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(corpo, CORPO_CSRF);
    }

    // ── 6. GET não é afetado ──────────────────────────────────────────────

    #[tokio::test]
    async fn get_passa_com_peer_nao_loopback_e_sem_chave() {
        let req = request(Method::GET, "/api/learning/skills", Some(LAN), &[]);
        let (status, corpo) = status_corpo(router(estado_com(None)), req).await;
        assert_eq!(status, StatusCode::OK, "leitura não é deste guarda");
        assert_eq!(corpo, "ok-leitura");
    }

    // ── 7. Sem ConnectInfo → 503, nunca finge loopback ────────────────────

    #[tokio::test]
    async fn sem_connect_info_da_503() {
        let req = request(Method::POST, "/api/learning/skills/x/rollback", None, &[]);
        let (status, corpo) = status_corpo(router(estado_com(None)), req).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(corpo, CORPO_SEM_PEER);
    }

    // ── 8. Chave configurada: o middleware não bloqueia o que o gate liberou ──

    #[tokio::test]
    async fn com_api_key_o_guarda_nao_bloqueia_nao_loopback() {
        // O gate global de /api/* já exigiu o bearer por fora deste layer
        // (testado em `gateway_auth`); aqui basta confirmar que o guarda
        // não recusa o request autenticado — inclusive sem peer avaliado.
        let router = router(estado_com(Some("k1")));
        for peer in [Some(LAN), None] {
            let req = request(
                Method::POST,
                "/api/learning/skills/x/rollback",
                peer,
                &[(header::AUTHORIZATION.as_str(), "Bearer k1")],
            );
            let (status, corpo) = status_corpo(router.clone(), req).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(corpo, "ok-mutante");
        }
    }

    // ── os caminhos legítimos do console web ──────────────────────────────

    #[tokio::test]
    async fn origin_igual_ao_host_passa() {
        let req = request(
            Method::POST,
            "/api/learning/skills/x/rollback",
            Some(LOOPBACK),
            &[
                (header::HOST.as_str(), "127.0.0.1:3888"),
                (header::ORIGIN.as_str(), "http://127.0.0.1:3888"),
            ],
        );
        let (status, _) = status_corpo(router(estado_com(None)), req).await;
        assert_eq!(status, StatusCode::OK, "console web local tem de passar");
    }

    #[tokio::test]
    async fn sec_fetch_site_same_origin_passa() {
        let req = request(
            Method::POST,
            "/api/learning/skills/x/rollback",
            Some(LOOPBACK),
            &[
                (header::HOST.as_str(), "127.0.0.1:3888"),
                ("sec-fetch-site", "same-origin"),
            ],
        );
        let (status, _) = status_corpo(router(estado_com(None)), req).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn origin_malformada_ou_host_ausente_da_403() {
        // Fail-closed: origem sem scheme, authority vazio ou sem Host não
        // confirmam "mesma origem".
        for headers in [
            vec![(header::ORIGIN.as_str(), "evil.example.com")],
            vec![
                (header::HOST.as_str(), "127.0.0.1:3888"),
                (header::ORIGIN.as_str(), "http:///"),
            ],
        ] {
            let req = request(
                Method::POST,
                "/api/learning/skills/x/rollback",
                Some(LOOPBACK),
                &headers,
            );
            let (status, _) = status_corpo(router(estado_com(None)), req).await;
            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "origem não confirmável tem de falhar fechado"
            );
        }
    }

    #[tokio::test]
    async fn origin_maiuscula_do_host_passa() {
        // Autoridades são case-insensitive.
        let req = request(
            Method::POST,
            "/api/learning/skills/x/rollback",
            Some(LOOPBACK),
            &[
                (header::HOST.as_str(), "127.0.0.1:3888"),
                (header::ORIGIN.as_str(), "HTTP://127.0.0.1:3888"),
            ],
        );
        let (status, _) = status_corpo(router(estado_com(None)), req).await;
        assert_eq!(status, StatusCode::OK);
    }

    // ── 9. Esquema e porta: a comparação é da origem COMPLETA (revisão
    //    do #1093: antes só a authority era comparada, e o doc do módulo
    //    mentia dizendo que o esquema entrava) ────────────────────────────

    #[tokio::test]
    async fn origin_https_contra_gateway_http_da_403() {
        // O cenário do achado da revisão: `https://127.0.0.1` contra
        // `Host: 127.0.0.1` casava a authority e passava — origens
        // diferentes, esquemas diferentes.
        let req = request(
            Method::POST,
            "/api/learning/skills/x/rollback",
            Some(LOOPBACK),
            &[
                (header::HOST.as_str(), "127.0.0.1"),
                (header::ORIGIN.as_str(), "https://127.0.0.1"),
            ],
        );
        let (status, corpo) = status_corpo(router(estado_com(None)), req).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(corpo, CORPO_CSRF);
    }

    #[tokio::test]
    async fn porta_default_80_normalizada_dos_dois_lados() {
        // `:80` em http não muda a origem, venha no Origin ou no Host.
        for (origin, host) in [
            ("http://example.com", "example.com:80"),
            ("http://example.com:80", "example.com"),
            ("http://example.com:80", "example.com:80"),
            ("http://example.com", "example.com"),
        ] {
            let req = request(
                Method::POST,
                "/api/learning/skills/x/rollback",
                Some(LOOPBACK),
                &[
                    (header::HOST.as_str(), host),
                    (header::ORIGIN.as_str(), origin),
                ],
            );
            let (status, _) = status_corpo(router(estado_com(None)), req).await;
            assert_eq!(
                status,
                StatusCode::OK,
                "{origin} vs {host} é a mesma origem"
            );
        }
    }

    #[tokio::test]
    async fn ipv6_authority_casa_e_diferente() {
        // IPv6 serializa com colchetes; os `:` internos não são porta —
        // o sufixo após o ÚLTIMO `:` é que conta, e só se for dígito.
        for (origin, host, esperado) in [
            ("http://[::1]:3888", "[::1]:3888", StatusCode::OK),
            ("http://[::1]", "[::1]", StatusCode::OK),
            ("http://[::1]:3888", "outro-host", StatusCode::FORBIDDEN),
        ] {
            let req = request(
                Method::POST,
                "/api/learning/skills/x/rollback",
                Some(LOOPBACK),
                &[
                    (header::HOST.as_str(), host),
                    (header::ORIGIN.as_str(), origin),
                ],
            );
            let (status, _) = status_corpo(router(estado_com(None)), req).await;
            assert_eq!(status, esperado, "{origin} vs {host}");
        }
    }

    #[tokio::test]
    async fn esquema_exotico_da_403() {
        // `ftp://` não é a origem do console, mesmo com a mesma authority.
        let req = request(
            Method::POST,
            "/api/learning/skills/x/rollback",
            Some(LOOPBACK),
            &[
                (header::HOST.as_str(), "127.0.0.1"),
                (header::ORIGIN.as_str(), "ftp://127.0.0.1"),
            ],
        );
        let (status, corpo) = status_corpo(router(estado_com(None)), req).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(corpo, CORPO_CSRF);
    }

    #[tokio::test]
    async fn tls_configurado_aceita_origin_https() {
        // O esquema não é hardcoded: com TLS nativo o console legítimo é
        // https, e o Origin https da mesma authority tem de passar.
        let mut estado = estado_com(None);
        estado.scheme = "https";
        let req = request(
            Method::POST,
            "/api/learning/skills/x/rollback",
            Some(LOOPBACK),
            &[
                (header::HOST.as_str(), "127.0.0.1:3888"),
                (header::ORIGIN.as_str(), "https://127.0.0.1:3888"),
            ],
        );
        let (status, _) = status_corpo(router(estado), req).await;
        assert_eq!(status, StatusCode::OK);
    }
}
