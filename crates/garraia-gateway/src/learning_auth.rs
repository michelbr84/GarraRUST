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
//!    `Origin` ou `Host` fora da gramática de authority (o parse estrito
//!    de `parse_authority` rejeita path, query, fragmento, userinfo,
//!    IPv6 sem colchete fechado, `:` dobrado e porta fora do intervalo),
//!    `Sec-Fetch-Site: cross-site` sem `Origin`, ou qualquer valor
//!    malformado — todos viram `403` de corpo fixo. `Origin` do mesmo
//!    esquema e mesma authority do `Host` (o
//!    console web servido pelo próprio gateway, porta default normalizada)
//!    passa.
//! 2. **Fail-closed sem credencial.** Com o gate de `gateway.api_key`
//!    **desligado**, só peer loopback passa; peer de LAN/internet recebe
//!    `503 learning: auth not configured` — o mesmo precedente do
//!    `/metrics` (`crate::metrics_auth`). No caso loopback, pedido de
//!    navegador (com `Origin`) só entra com `Host` de **nome de loopback**
//!    (`127.0.0.1`/`localhost`/`[::1]`) — a âncora anti-DNS-rebinding:
//!    um domínio do atacante re-resolvido para `127.0.0.1` faz o
//!    navegador mandar `Origin` e `Host` iguais ao alias, e só o nome de
//!    loopback, que DNS público nenhum aponta para fora da máquina,
//!    distingue o console do alias. Com a chave configurada, o gate
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
//! do header `Host` (com porta default normalizada), e o passo 3 ancora o
//! caso sem credencial num `Host` de nome de loopback. O que sobra como
//! residual é o cliente **não-navegador**: código que já alcança a porta e
//! fabrica os dois headers (`curl -H "Origin: http://x" -H "Host: x"` —
//! sem `Origin` ele nem precisa fabricar) ou já roda na própria máquina.
//! **Contra um cliente que já executa código local, a única proteção é
//! `gateway.api_key`**, que continua sendo o gate de verdade para todo
//! `/api/*`. Este guarda fecha o que o navegador pode ser forçado a fazer —
//! a página visitada pelo dono e o DNS rebinding —, e é a camada que falta
//! quando essa chave não existe.

use std::net::{IpAddr, Ipv6Addr, SocketAddr};

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
            // Gramática do header `Origin` (RFC 6454): `scheme "://" host
            // [":" port]` — sem path, query, fragmento ou userinfo. O parse
            // estrito de `parse_authority` rejeita qualquer sobra; nenhum
            // navegador manda, quem manda é cliente de mentira: fail-closed.
            let Some((host_origem, porta_origem)) = parse_authority(resto) else {
                return true;
            };
            match headers.get(header::HOST).and_then(|h| h.to_str().ok()) {
                // Sem Host não há como confirmar mesma origem — fail-closed.
                None => true,
                Some(host) => {
                    // Host fora da gramática de authority também não
                    // compara: string alguma se casa por cima de lixo.
                    let Some((host_header, porta_header)) = parse_authority(host) else {
                        return true;
                    };
                    !mesma_authority(host_origem, porta_origem, host_header, porta_header, scheme)
                }
            }
        }
        None => headers
            .get("sec-fetch-site")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.eq_ignore_ascii_case("cross-site")),
    }
}

/// Faz o parse **estrito** de uma authority `host[:porta]` — a única forma
/// que `Origin` (RFC 6454) e `Host` (RFC 3986 §3.2) têm. Retorna o host sem
/// porta e a porta explícita, e rejeita **qualquer sobra**:
///
/// - IPv6 só entre colchetes, com fechamento exato: `[::1]`, `[::1]:3888`;
///   `[::1]lixo` e `[::1]:3888:80` não parseiam;
/// - fora de colchetes o `:` só separa porta — `host:porta:porta` morre na
///   porta que não é dígito;
/// - porta toda dígito, não vazia, no intervalo `1..=65535`;
/// - nome de host só com `[A-Za-z0-9.\-_~]`: `@` (userinfo), `/`, `?` e `#`
///   (path, query, fragmento) não têm vez numa authority.
///
/// Fail-closed: `None` para tudo que não é exatamente uma authority.
fn parse_authority(authority: &str) -> Option<(&str, Option<u16>)> {
    if let Some(resto) = authority.strip_prefix('[') {
        // IP-literal: `[…]` ou `[…]:porta`, e nada depois.
        let (dentro, depois) = resto.split_once(']')?;
        dentro.parse::<Ipv6Addr>().ok()?;
        let porta = if depois.is_empty() {
            None
        } else {
            Some(valida_porta(depois.strip_prefix(':')?)?)
        };
        return Some((dentro, porta));
    }
    // Fora de colchetes um `:` separa a porta — e é o único permitido:
    // o segundo `:` de `host:porta:porta` morre na porta não-dígito.
    let (host, porta) = match authority.split_once(':') {
        None => (authority, None),
        Some((antes, depois)) => (antes, Some(valida_porta(depois)?)),
    };
    if host.is_empty()
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'~'))
    {
        return None;
    }
    Some((host, porta))
}

/// Porta de authority válida: toda dígito, não vazia, no intervalo
/// `1..=65535` (`u16` sem a porta zero).
fn valida_porta(p: &str) -> Option<u16> {
    if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let porta: u16 = p.parse().ok()?;
    (porta > 0).then_some(porta)
}

/// Compara as duas authorities já parseadas, normalizando a porta default
/// dos dois lados: `:80` em http e `:443` em https não mudam a origem.
fn mesma_authority(
    host_origem: &str,
    porta_origem: Option<u16>,
    host_header: &str,
    porta_header: Option<u16>,
    scheme: &str,
) -> bool {
    let porta_default: u16 = if scheme.eq_ignore_ascii_case("https") {
        443
    } else {
        80
    };
    host_origem.eq_ignore_ascii_case(host_header)
        && porta_origem.unwrap_or(porta_default) == porta_header.unwrap_or(porta_default)
}

/// `true` quando a authority do header `Host` é um **nome de loopback**:
/// `127.0.0.1` (ou qualquer IP de loopback), `localhost` ou `[::1]`.
///
/// É a âncora anti-DNS-rebinding do passo 3: nomes de loopback não existem
/// no DNS público, então um `Host` que não é loopback não pode ser o
/// endereço pelo qual um gateway ligado em loopback foi alcançado — é o
/// alias de um domínio do atacante que o navegador da vítima resolveu para
/// `127.0.0.1`. O `Host` passa pelo mesmo parse estrito do passo 1
/// (`parse_authority`): `[::1]lixo` não é nome de loopback, é lixo —
/// fail-closed em tudo que não parseia.
fn host_de_loopback(headers: &HeaderMap) -> bool {
    let Some(host_header) = headers.get(header::HOST).and_then(|h| h.to_str().ok()) else {
        return false;
    };
    let Some((sem_porta, _)) = parse_authority(host_header) else {
        return false;
    };
    sem_porta.eq_ignore_ascii_case("localhost")
        || sem_porta
            .parse::<IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
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
        Some(ip) if ip.is_loopback() => {
            // Anti-DNS-rebinding (#1093): o navegador da vítima pode
            // conectar ao domínio do atacante re-resolvido para 127.0.0.1 —
            // aí `Origin` e `Host` são ambos o alias do atacante e casam
            // entre si no passo 1. A âncora é o `Host`: pedido de navegador
            // (com `Origin`) só entra com `Host` de loopback, o endereço
            // pelo qual um gateway ligado em loopback de fato é alcançado.
            if req.headers().contains_key(header::ORIGIN) && !host_de_loopback(req.headers()) {
                warn!(
                    path = %req.uri().path(),
                    method = %method,
                    "learning: loopback peer behind a non-loopback Host (DNS rebinding?)"
                );
                return deny(StatusCode::FORBIDDEN, CORPO_CSRF);
            }
            next.run(req).await
        }
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
        // `:80` em http não muda a origem, venha no Origin ou no Host. Os
        // combos usam nome de loopback porque, com o gate desligado, a
        // âncora anti-DNS-rebinding do passo 3 nega `Host` que não é
        // loopback por design — normalização e âncora no mesmo caminho que
        // o console real percorre.
        for (origin, host) in [
            ("http://localhost", "localhost:80"),
            ("http://localhost:80", "localhost"),
            ("http://localhost:80", "localhost:80"),
            ("http://localhost", "localhost"),
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

    // ── rodada 3 (code review): gramática do Origin ────────────────────────

    #[tokio::test]
    async fn origin_fora_da_gramatica_da_403() {
        // RFC 6454: Origin é `scheme "://" host [":" port]` — sem path,
        // query, fragmento ou userinfo. O valor abaixo casava a authority e
        // passava; agora a sobra é rejeitada, valendo também autenticado
        // (o passo 1 roda antes do gate).
        for origin_malformado in [
            "http://127.0.0.1:3888/qualquer-caminho",
            "http://127.0.0.1:3888?q=1",
            "http://127.0.0.1:3888#frag",
            "http://usuario@127.0.0.1:3888",
        ] {
            let req = request(
                Method::POST,
                "/api/learning/skills/x/rollback",
                Some(LOOPBACK),
                &[
                    (header::HOST.as_str(), "127.0.0.1:3888"),
                    (header::ORIGIN.as_str(), origin_malformado),
                ],
            );
            let (status, corpo) = status_corpo(router(estado_com(Some("chave"))), req).await;
            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "Origin malformado aceito: {origin_malformado}"
            );
            assert_eq!(corpo, CORPO_CSRF);
        }

        // Vetor que isola a rejeição: o `Host` carregando a MESMA sobra.
        // Nos quatro vetores acima a sobra já denuncia o Origin na
        // comparação com um `Host` limpo; aqui as duas authorities casam
        // entre si e só o parse estrito (`parse_authority` rejeita sobra
        // depois da porta) separa "authority com sobra" de "authority".
        // É a prova de não-vacuidade: tornar o parse leniente deve
        // tornar exatamente este vetor verde.
        let req = request(
            Method::POST,
            "/api/learning/skills/x/rollback",
            Some(LOOPBACK),
            &[
                (header::HOST.as_str(), "127.0.0.1:3888/qualquer-caminho"),
                (
                    header::ORIGIN.as_str(),
                    "http://127.0.0.1:3888/qualquer-caminho",
                ),
            ],
        );
        let (status, corpo) = status_corpo(router(estado_com(Some("chave"))), req).await;
        assert_eq!(
            status,
            StatusCode::FORBIDDEN,
            "sobra espelhada no Host aceita como mesma origem"
        );
        assert_eq!(corpo, CORPO_CSRF);
    }

    // ── rodada 3 (security audit): DNS rebinding ────────────────────────────

    #[tokio::test]
    async fn rebinding_de_dominio_externo_da_403() {
        // O cenário do achado HIGH da auditoria: domínio do atacante
        // re-resolvido para 127.0.0.1. O navegador da vítima manda `Origin`
        // e `Host` iguais ao alias — casam entre si e o passo 1 passava.
        // A âncora é o Host de loopback: alias de DNS público não entra.
        let req = request(
            Method::POST,
            "/api/learning/skills/x/rollback",
            Some(LOOPBACK),
            &[
                (header::HOST.as_str(), "evil.example:3888"),
                (header::ORIGIN.as_str(), "http://evil.example:3888"),
            ],
        );
        let (status, corpo) = status_corpo(router(estado_com(None)), req).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(corpo, CORPO_CSRF);
    }

    #[tokio::test]
    async fn console_de_loopback_continua_passando_com_origin() {
        // Controle negativo da âncora: o console legítimo serve em nomes de
        // loopback (com ou sem porta default, IPv6 incluso) e continua
        // entrando com o Origin do navegador.
        for (host, origin) in [
            ("127.0.0.1:3888", "http://127.0.0.1:3888"),
            ("localhost:3888", "http://localhost:3888"),
            ("[::1]:3888", "http://[::1]:3888"),
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
            let (status, corpo) = status_corpo(router(estado_com(None)), req).await;
            assert_eq!(status, StatusCode::OK, "console em {host} deveria passar");
            assert_eq!(corpo, "ok-mutante");
        }
    }

    // ── rodada 4 (code review): parse estrito da authority ──────────────────

    #[tokio::test]
    async fn authority_malformada_espelhada_da_403() {
        // Vetores do code review rodada 3: authorities que casam consigo
        // mesmas no `Host` espelhado — nada de acidente de comparação com
        // `Host` limpo, só o parse estrito pode separar. `[::1]lixo` entrava
        // como loopback na âncora (corte no primeiro `]`), `[::1]:3888:80` e
        // `host:porta:porta` têm `:` dobrado, `99999` e `0` são portas fora
        // do intervalo.
        for (origin, host) in [
            ("http://[::1]lixo", "[::1]lixo"),
            ("http://[::1]:3888:80", "[::1]:3888:80"),
            ("http://host:porta:porta", "host:porta:porta"),
            ("http://127.0.0.1:99999", "127.0.0.1:99999"),
            ("http://127.0.0.1:0", "127.0.0.1:0"),
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
            let (status, corpo) = status_corpo(router(estado_com(Some("chave"))), req).await;
            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "authority espelhada aceita: {origin} / {host}"
            );
            assert_eq!(corpo, CORPO_CSRF);
        }
    }

    #[test]
    fn parse_authority_rejeita_fora_da_gramatica() {
        // A gramática inteira num só lugar: o `None` de cada lixo é o que o
        // passo 1, a âncora e a comparação de authority têm em comum.
        for lixo in [
            "[::1]lixo",
            "[::1]:3888:80",
            "host:porta:porta",
            "127.0.0.1:99999",
            "127.0.0.1:0",
            "usuario@127.0.0.1",
            "127.0.0.1:3888/qualquer-caminho",
            "127.0.0.1:3888?q=1",
            "127.0.0.1:3888#frag",
            ":3888",
            "localhost:",
            "",
            "::1",
        ] {
            assert!(
                parse_authority(lixo).is_none(),
                "authority aceita fora da gramática: {lixo}"
            );
        }
        assert_eq!(parse_authority("localhost"), Some(("localhost", None)));
        assert_eq!(
            parse_authority("localhost:3888"),
            Some(("localhost", Some(3888)))
        );
        assert_eq!(parse_authority("[::1]"), Some(("::1", None)));
        assert_eq!(parse_authority("[::1]:3888"), Some(("::1", Some(3888))));
    }

    #[test]
    fn host_de_loopback_rejeita_sobra_depois_do_colchete() {
        // O buraco antigo: o corte no primeiro `]` fazia `[::1]lixo` contar
        // como loopback na âncora. Agora o `Host` passa pelo mesmo parse
        // estrito do passo 1 e não parseia — fail-closed.
        let headers = |valor: &str| {
            let mut headers = HeaderMap::new();
            headers.insert(
                header::HOST,
                axum::http::HeaderValue::from_str(valor).unwrap(),
            );
            headers
        };
        assert!(!host_de_loopback(&headers("[::1]lixo")));
        for bom in [
            "127.0.0.1:3888",
            "localhost:3888",
            "LOCALHOST",
            "[::1]:3888",
            "[::1]",
        ] {
            assert!(host_de_loopback(&headers(bom)), "loopback negado: {bom}");
        }
        for ruim in ["evil.example:3888", "localhost.evil.com", "192.168.0.1"] {
            assert!(
                !host_de_loopback(&headers(ruim)),
                "não-loopback aceito: {ruim}"
            );
        }
    }
}
