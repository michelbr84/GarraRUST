//! Guarda anti-CSRF **genérica** das rotas mutantes do gateway (#1182).
//!
//! O #1093 fechou `/api/learning/*` — as rotas que rodam `git` no repositório
//! do dono. O resto da superfície mutante ficou aberta: `PATCH /api/settings`
//! escreve a config, `POST /api/mode/select` troca o modo do agente,
//! `POST /api/mcp/marketplace/install` instala servidor MCP,
//! `POST /v1/chat/completions` gasta a chave de LLM do dono, `DELETE
//! /api/memory` apaga memória. Todas elas são `/api/*` auth-free por desenho
//! ("quem alcança a porta é o dono") — só que **o navegador do dono alcança a
//! porta rodando código de terceiros**: basta o dono visitar uma página, e ela
//! dispara o `POST` contra `127.0.0.1:3888` de dentro do navegador dele.
//! Pior, o CORS default era `allow_origin(Any)`, então até a *resposta* voltava
//! legível para a página do atacante.
//!
//! Este módulo é a versão genérica daquele guarda, mais as primitivas de
//! gramática de authority que o `learning_auth` já tinha e que agora são
//! compartilhadas pelos dois (e por `/ws`).
//!
//! ## O que `mutations_guard` faz
//!
//! Para `POST`/`PUT`/`PATCH`/`DELETE` (métodos de leitura passam intocados,
//! e o preflight `OPTIONS` é respondido pelo `CorsLayer`, que fica **por
//! fora** deste middleware), nesta ordem:
//!
//! 1. Caminho na skip-list [`SEM_GUARDA`] → passa. São os caminhos que já têm
//!    guarda própria (learning, plugins, admin) ou que não têm navegador no
//!    caminho (webhooks assinados server-to-server).
//! 2. `Origin` **exatamente** igual a uma entrada de
//!    `gateway.allowed_origins` → passa (`origem_declarada`). É a escotilha
//!    do perfil reverse proxy, onde o esquema do `Origin` (https, terminado
//!    no proxy) não é o do transporte que este processo enxerga (http).
//! 3. [`cross_origin`] → `403`. `Origin` de outro esquema, de outra authority
//!    que o `Host`, `Origin: null`, fora da gramática de authority, ou
//!    `Sec-Fetch-Site: cross-site` sem `Origin`.
//! 4. Âncora anti-DNS-rebinding ([`ancora_ok`]), só quando há `Origin` → `403`
//!    se o `Host` for um nome DNS que não é `localhost` nem está em
//!    `gateway.allowed_origins`. Sem isso, o domínio do atacante re-resolvido
//!    para `127.0.0.1` manda `Origin` e `Host` iguais entre si e passa o
//!    passo 3.
//!
//! O corpo do `403` é um `&'static str` fixo: nada do que veio no pedido
//! (`Origin`, `Host`, token) é ecoado. O log leva caminho e método, nunca o
//! valor dos headers.
//!
//! ## Recorte deliberado: nada de `ConnectInfo` aqui
//!
//! O `learning_mutations_guard` tem um passo a mais — sem `gateway.api_key`
//! configurada, peer não-loopback leva `503 auth not configured`. **Este
//! guarda genérico NÃO replica isso**, de propósito: a superfície ampla
//! inclui o caminho que o **app mobile na LAN** usa hoje contra um Garra sem
//! `api_key` (cenário suportado, ver `gateway_auth.rs` e o `GarraConnection`
//! do app). Herdar o `503` do learning aqui quebraria esse app. O que este
//! módulo fecha é o que o **navegador** pode ser forçado a fazer; quem já
//! executa código na máquina (ou já alcança a porta e fabrica headers com
//! `curl`) continua sendo assunto de `gateway.api_key`, o gate de verdade.
//!
//! ## Âncora: por que não é `host_de_loopback`
//!
//! O learning ancora no `Host` ser **nome de loopback**, porque aquele passo
//! só roda no caso "peer loopback, sem credencial". Aqui a superfície é o
//! Web Console inteiro, que é legitimamente acessado por IP de LAN
//! (`http://192.168.1.10:3888`, o app mobile e o navegador do tablet). Um
//! `Host` desses não é nome de loopback e morreria. A âncora certa é outra:
//! **DNS rebinding exige um nome DNS** — um IP literal não é rebindável, e
//! `localhost` não existe no DNS público. Daí [`ancora_ok`]: IP literal,
//! `localhost`, ou nome que o dono listou em `gateway.allowed_origins`.

use std::net::{IpAddr, Ipv6Addr};
use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tracing::warn;

/// Corpo do 403. Constante: nada do que veio no pedido é ecoado.
const CORPO_CSRF: &str =
    "gateway: cross-origin mutating request refused (see gateway.allowed_origins)";

/// Caminhos que **não** passam por [`mutations_guard`].
///
/// Não é "exceção para facilitar": cada um já é guardado por algo mais
/// estrito, e dupla aplicação só trocaria o corpo do 403 (quebrando o
/// contrato de resposta que os testes daquelas rotas já travam).
///
/// - `/api/learning/` — guarda própria do #1093 (`learning_mutations_guard`),
///   que além do anti-CSRF tem o fail-closed de peer/credencial.
/// - `/api/plugins/` — `require_admin_auth` + `require_csrf` no
///   `plugins_handler`.
/// - `/admin/` — `require_admin_auth` + `require_csrf` + `security_headers`.
/// - `/webhooks/` — server-to-server assinado (WhatsApp/Google Chat/Teams/
///   LINE). Não há navegador no caminho e o remetente nunca manda `Origin`;
///   a autenticidade vem da assinatura do provedor.
const SEM_GUARDA: &[&str] = &["/api/learning/", "/api/plugins/", "/admin/", "/webhooks/"];

/// `true` quando o pedido mutante chega de uma origem de navegador que não
/// é a do próprio gateway.
///
/// Um POST legítimo do console web (servido pelo próprio gateway) traz
/// `Origin` com o mesmo esquema do transporte e a mesma authority do header
/// `Host` — porta default (`:80` em http, `:443` em https) normalizada dos
/// dois lados. Um POST do app (Dio) ou do `curl` não traz `Origin` nenhum. O
/// que **não** é legítimo: `Origin` de outro esquema (`https` contra um
/// gateway http, ou um esquema exótico), `Origin` de outra authority,
/// `Origin: null` (sandbox/redirect), ou `Sec-Fetch-Site: cross-site` sem
/// `Origin`. Origem malformada ou `Host` ausente são tratados como
/// cross-origin — fail-closed, e nunca se ecoa o valor rejeitado.
pub fn cross_origin(headers: &HeaderMap, scheme: &str) -> bool {
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
pub fn parse_authority(authority: &str) -> Option<(&str, Option<u16>)> {
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
/// É a âncora anti-DNS-rebinding do `learning_mutations_guard`: nomes de
/// loopback não existem no DNS público, então um `Host` que não é loopback
/// não pode ser o endereço pelo qual um gateway ligado em loopback de fato
/// foi alcançado — é o alias de um domínio do atacante que o navegador da
/// vítima resolveu para `127.0.0.1`. O `Host` passa pelo mesmo parse estrito
/// de [`parse_authority`]: `[::1]lixo` não é nome de loopback, é lixo —
/// fail-closed em tudo que não parseia.
///
/// A superfície ampla usa [`ancora_ok`], não esta função: ver o doc do
/// módulo.
pub fn host_de_loopback(headers: &HeaderMap) -> bool {
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

/// Âncora anti-DNS-rebinding da superfície ampla: `true` quando o `Host` do
/// pedido **não pode** ser um alias re-resolvido de um domínio do atacante.
///
/// Consultada só quando há `Origin` (cliente não-navegador não é vítima de
/// rebinding). Três formas de passar:
///
/// 1. **IP literal** (v4, ou v6 entre colchetes). Rebinding é um truque de
///    DNS: quem digita o IP não passa por resolvedor nenhum, e o navegador
///    da vítima não tem como ser levado a mandar `Host: 192.168.1.10` para
///    um servidor que não seja aquele. Cobre o Web Console acessado por IP
///    de LAN, que `host_de_loopback` mataria.
/// 2. **`localhost`**, que DNS público nenhum aponta para fora da máquina.
/// 3. **Nome listado em `gateway.allowed_origins`** — o dono declarou o
///    domínio do reverse proxy dele; é o mesmo ato de confiança do CORS.
///
/// A comparação da forma 3 é só do **host**, sem porta: rebinding troca o IP
/// por trás de um nome, e a porta não participa do truque.
///
/// `Host` ausente ou fora da gramática → `false` (fail-closed).
pub fn ancora_ok(headers: &HeaderMap, allowed_origins: &[String]) -> bool {
    let Some(host_header) = headers.get(header::HOST).and_then(|h| h.to_str().ok()) else {
        return false;
    };
    let Some((host, _porta)) = parse_authority(host_header) else {
        return false;
    };
    // `parse_authority` devolve o IPv6 já sem colchetes, então o `parse`
    // cobre as duas famílias.
    if host.parse::<IpAddr>().is_ok() || host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    allowed_origins.iter().any(|origem| {
        let authority = origem
            .split_once("://")
            .map(|(_, resto)| resto)
            .unwrap_or(origem.as_str());
        parse_authority(authority)
            .is_some_and(|(permitido, _)| permitido.eq_ignore_ascii_case(host))
    })
}

/// `true` quando o header `Origin` é **exatamente** uma das origens que o
/// dono declarou em `gateway.allowed_origins`.
///
/// É a escotilha do perfil "reverse proxy com domínio próprio": o proxy
/// termina TLS e fala `http` com o gateway por baixo, então o navegador manda
/// `Origin: https://meu.dominio` contra um transporte que [`cross_origin`]
/// conhece como `http` — a comparação de esquema recusa, e nem a âncora
/// chega a ser consultada. Sem esta função, aquele perfil não teria escotilha
/// nenhuma (nem `gateway.api_key` resolveria: este guarda roda igual com a
/// chave configurada, de propósito — ver `learning_auth_layering.rs`).
///
/// A confiança aqui é a mesma que a lista já carrega para o CORS: o dono
/// escreveu a origem no arquivo de config dele. A comparação é da string
/// inteira (`scheme://host[:port]`), case-insensitive, com uma barra final
/// opcional tolerada dos dois lados — não é match por prefixo, e uma entrada
/// vazia nunca casa.
fn origem_declarada(headers: &HeaderMap, allowed_origins: &[String]) -> bool {
    let Some(origem) = headers
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.trim_end_matches('/'))
    else {
        return false;
    };
    if origem.is_empty() {
        return false;
    }
    allowed_origins
        .iter()
        .any(|declarada| declarada.trim_end_matches('/').eq_ignore_ascii_case(origem))
}

/// `true` quando o handshake de WebSocket pode subir.
///
/// Sem `Origin` o cliente não é navegador (o app, o `garra` CLI e o `curl`
/// não mandam) e o guarda não tem o que decidir — quem alcança a porta sem
/// navegador é assunto de `gateway.api_key`. Com `Origin`, valem as mesmas
/// duas regras do [`mutations_guard`]: mesma origem do transporte **e**
/// âncora anti-rebinding.
///
/// Usada em `/ws` (webchat). **Não** em `/ws/parrot`: o overlay do Garra
/// Desktop é uma webview Tauri, cujo `Origin` real de handshake
/// (`tauri://localhost` e variantes por plataforma) não pôde ser verificado
/// no ambiente desta entrega — adivinhar a allowlist arriscaria derrubar o
/// desktop em produção sem forma de testar. Fica como follow-up.
pub fn ws_upgrade_permitido(headers: &HeaderMap, scheme: &str, allowed_origins: &[String]) -> bool {
    if !headers.contains_key(header::ORIGIN) {
        return true;
    }
    if origem_declarada(headers, allowed_origins) {
        return true;
    }
    !cross_origin(headers, scheme) && ancora_ok(headers, allowed_origins)
}

/// Estado da guarda genérica. Montado no `build_router`.
#[derive(Clone)]
pub struct OriginGuardState {
    /// Esquema efetivo do transporte (`"http"` | `"https"`) — `https` só
    /// quando o TLS nativo está configurado, o mesmo critério de
    /// `use_tls`/`session_cookie_secure`.
    pub scheme: &'static str,
    /// `gateway.allowed_origins`, as origens que o dono declarou confiar.
    pub allowed_origins: Arc<[String]>,
}

/// O middleware. Montado **por dentro** do gate de `gateway.api_key` (ver o
/// comentário da montagem em `router.rs`): quando os dois se aplicam, o 401
/// do gate vem primeiro.
pub async fn mutations_guard(
    State(estado): State<OriginGuardState>,
    req: Request,
    next: Next,
) -> Response {
    // Leitura não é afetada. `OPTIONS` idem: o preflight é respondido pelo
    // `CorsLayer`, que é layer mais externo, antes de chegar aqui.
    let method = req.method().clone();
    if matches!(method, Method::GET | Method::HEAD | Method::OPTIONS) {
        return next.run(req).await;
    }

    if SEM_GUARDA
        .iter()
        .any(|prefixo| req.uri().path().starts_with(prefixo))
    {
        return next.run(req).await;
    }

    // A origem que o dono declarou passa direto — é a escotilha do reverse
    // proxy, onde o esquema do `Origin` (https, terminado no proxy) não é o
    // do transporte que este processo enxerga (http).
    if origem_declarada(req.headers(), &estado.allowed_origins) {
        return next.run(req).await;
    }

    let recusa = cross_origin(req.headers(), estado.scheme)
        // Âncora anti-DNS-rebinding: `Origin` e `Host` casarem entre si não
        // basta, porque o alias do atacante casa consigo mesmo.
        || (req.headers().contains_key(header::ORIGIN)
            && !ancora_ok(req.headers(), &estado.allowed_origins));

    if recusa {
        // Só caminho e método vão para o log; o valor de `Origin`/`Host` não
        // sai, e o corpo é constante.
        warn!(
            path = %req.uri().path(),
            method = %method,
            "gateway: cross-origin mutating request refused"
        );
        return (StatusCode::FORBIDDEN, CORPO_CSRF).into_response();
    }

    next.run(req).await
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::http::HeaderValue;
    use axum::http::Request as HttpRequest;
    use axum::middleware::from_fn_with_state;
    use axum::routing::{any, get};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn handler() -> &'static str {
        "ok"
    }

    /// As rotas mutantes reais que este guarda cobre — uma amostra
    /// representativa de cada família da superfície `/api/*` + `/v1/*` +
    /// `/chat` + `/a2a/*`. `any(...)` porque o que se testa aqui é o
    /// middleware, não o roteamento por método.
    const MUTANTES: &[(&str, &str)] = &[
        ("PATCH", "/api/settings"),
        ("POST", "/api/mode/select"),
        ("POST", "/api/mcp/marketplace/install"),
        ("POST", "/api/skills"),
        ("PUT", "/api/skills/x"),
        ("DELETE", "/api/skins/x"),
        ("POST", "/api/sessions"),
        ("DELETE", "/api/sessions/x"),
        ("DELETE", "/api/memory"),
        ("POST", "/v1/chat/completions"),
        ("POST", "/v1/messages"),
        ("POST", "/chat"),
        ("POST", "/a2a/tasks"),
    ];

    /// Caminhos da skip-list, com o corpo que a rota de verdade responderia.
    const PULADOS: &[&str] = &[
        "/api/learning/skills/x/rollback",
        "/api/plugins/install",
        "/admin/settings",
        "/webhooks/whatsapp",
    ];

    fn estado(allowed: &[&str]) -> OriginGuardState {
        OriginGuardState {
            scheme: "http",
            allowed_origins: allowed.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    fn router(estado: OriginGuardState) -> Router {
        let mut router = Router::new().route("/api/health", get(handler));
        for (_, path) in MUTANTES {
            router = router.route(path, any(handler));
        }
        for path in PULADOS {
            router = router.route(path, any(handler));
        }
        router.layer(from_fn_with_state(estado, mutations_guard))
    }

    fn request(method: &str, uri: &str, headers: &[(&str, &str)]) -> HttpRequest<Body> {
        let mut builder = HttpRequest::builder().uri(uri).method(method);
        for (nome, valor) in headers {
            builder = builder.header(*nome, *valor);
        }
        builder.body(Body::empty()).expect("request sintetico")
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

    fn headers_com(pares: &[(&str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (nome, valor) in pares {
            headers.insert(
                axum::http::HeaderName::from_bytes(nome.as_bytes()).expect("nome de header"),
                HeaderValue::from_str(valor).expect("valor de header"),
            );
        }
        headers
    }

    // ── 1. a tabela das rotas mutantes reais ──────────────────────────────

    #[tokio::test]
    async fn origin_estranho_da_403_em_toda_rota_mutante() {
        let router = router(estado(&[]));
        for (metodo, path) in MUTANTES {
            let req = request(
                metodo,
                path,
                &[
                    ("host", "127.0.0.1:3888"),
                    ("origin", "http://evil.example"),
                ],
            );
            let (status, corpo) = status_corpo(router.clone(), req).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{metodo} {path}");
            assert_eq!(corpo, CORPO_CSRF, "{metodo} {path}");
        }
    }

    #[tokio::test]
    async fn sem_origin_passa_em_toda_rota_mutante() {
        // O app (Dio), o `curl` e o `garra` CLI não mandam `Origin`.
        let router = router(estado(&[]));
        for (metodo, path) in MUTANTES {
            let req = request(metodo, path, &[("host", "127.0.0.1:3888")]);
            let (status, corpo) = status_corpo(router.clone(), req).await;
            assert_eq!(status, StatusCode::OK, "{metodo} {path}");
            assert_eq!(corpo, "ok");
        }
    }

    #[tokio::test]
    async fn origin_igual_ao_host_passa_em_toda_rota_mutante() {
        // O Web Console é servido pelo próprio gateway: same-origin.
        let router = router(estado(&[]));
        for (metodo, path) in MUTANTES {
            let req = request(
                metodo,
                path,
                &[
                    ("host", "127.0.0.1:3888"),
                    ("origin", "http://127.0.0.1:3888"),
                ],
            );
            let (status, corpo) = status_corpo(router.clone(), req).await;
            assert_eq!(status, StatusCode::OK, "{metodo} {path}");
            assert_eq!(corpo, "ok");
        }
    }

    #[tokio::test]
    async fn sec_fetch_site_cross_site_sem_origin_da_403() {
        let router = router(estado(&[]));
        for (metodo, path) in MUTANTES {
            let req = request(
                metodo,
                path,
                &[("host", "127.0.0.1:3888"), ("sec-fetch-site", "cross-site")],
            );
            let (status, corpo) = status_corpo(router.clone(), req).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{metodo} {path}");
            assert_eq!(corpo, CORPO_CSRF);
        }
    }

    #[tokio::test]
    async fn leitura_nao_e_afetada() {
        let req = request(
            "GET",
            "/api/health",
            &[
                ("host", "127.0.0.1:3888"),
                ("origin", "http://evil.example"),
            ],
        );
        let (status, corpo) = status_corpo(router(estado(&[])), req).await;
        assert_eq!(status, StatusCode::OK, "GET não é deste guarda");
        assert_eq!(corpo, "ok");
    }

    // ── 2. a âncora anti-rebinding ────────────────────────────────────────

    #[tokio::test]
    async fn ancora_por_host() {
        // Cada caso é `Origin == Host` (casam entre si, passam o passo 2) —
        // só a âncora separa o console legítimo do alias do atacante.
        for (host, allowed, esperado) in [
            // IP de LAN: o Web Console no tablet. `host_de_loopback` mataria.
            ("192.168.1.10:3888", &[][..], StatusCode::OK),
            ("127.0.0.1:3888", &[][..], StatusCode::OK),
            ("[::1]:3888", &[][..], StatusCode::OK),
            ("localhost:3888", &[][..], StatusCode::OK),
            ("LOCALHOST:3888", &[][..], StatusCode::OK),
            // Nome DNS não listado: rebinding.
            ("evil.example:3888", &[][..], StatusCode::FORBIDDEN),
            // O mesmo nome, declarado pelo dono: passa.
            (
                "meu.dominio:3888",
                &["http://meu.dominio:3888"][..],
                StatusCode::OK,
            ),
            // A entrada listada sem porta continua cobrindo: a âncora
            // compara só o host.
            (
                "meu.dominio:3888",
                &["http://meu.dominio"][..],
                StatusCode::OK,
            ),
            // Nome listado ≠ nome do pedido.
            (
                "evil.example:3888",
                &["http://meu.dominio:3888"][..],
                StatusCode::FORBIDDEN,
            ),
        ] {
            let origin = format!("http://{host}");
            let req = request(
                "PATCH",
                "/api/settings",
                &[("host", host), ("origin", &origin)],
            );
            let (status, _) = status_corpo(router(estado(allowed)), req).await;
            assert_eq!(status, esperado, "Host {host} com allowed {allowed:?}");
        }
    }

    #[tokio::test]
    async fn ancora_fail_closed_sem_host_ou_com_host_invalido() {
        for host in ["[::1]lixo", "127.0.0.1:0"] {
            assert!(
                !ancora_ok(&headers_com(&[("host", host)]), &[]),
                "Host fora da gramática aceito: {host}"
            );
        }
        assert!(!ancora_ok(&HeaderMap::new(), &[]), "sem Host tem de falhar");
    }

    // ── 2b. a escotilha do reverse proxy ──────────────────────────────────

    #[tokio::test]
    async fn origem_declarada_passa_mesmo_com_esquema_diferente() {
        // O perfil "reverse proxy com dominio proprio": o proxy termina TLS,
        // o navegador manda `Origin: https://…`, e o gateway por baixo fala
        // `http`. Sem a escotilha, nao haveria config que salvasse — nem
        // `gateway.api_key`, que nao dispensa a origem certa.
        let req = request(
            "PATCH",
            "/api/settings",
            &[("host", "meu.dominio"), ("origin", "https://meu.dominio")],
        );
        let (status, _) = status_corpo(router(estado(&["https://meu.dominio"])), req).await;
        assert_eq!(status, StatusCode::OK, "origem declarada tem de passar");

        // Barra final tolerada dos dois lados.
        let req = request(
            "PATCH",
            "/api/settings",
            &[("host", "meu.dominio"), ("origin", "https://meu.dominio")],
        );
        let (status, _) = status_corpo(router(estado(&["https://meu.dominio/"])), req).await;
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn a_escotilha_nao_e_match_por_prefixo() {
        // O vetor obvio contra uma comparacao frouxa: o atacante registra um
        // dominio que TEM o declarado como prefixo.
        for origin in [
            "https://meu.dominio.evil.example",
            "https://evil.example/https://meu.dominio",
            "https://meu.dominio:8443",
        ] {
            let req = request(
                "PATCH",
                "/api/settings",
                &[("host", "evil.example"), ("origin", origin)],
            );
            let (status, corpo) = status_corpo(router(estado(&["https://meu.dominio"])), req).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "escotilha frouxa: {origin}");
            assert_eq!(corpo, CORPO_CSRF);
        }
    }

    #[tokio::test]
    async fn lista_vazia_nao_declara_nada() {
        // Guarda contra o `any()` de lista vazia virar `true` num refactor, e
        // contra uma entrada vazia casar com `Origin` vazio.
        assert!(!origem_declarada(
            &headers_com(&[("origin", "https://meu.dominio")]),
            &[]
        ));
        assert!(!origem_declarada(
            &headers_com(&[("host", "127.0.0.1:3888")]),
            &["".to_string()]
        ));
    }

    // ── 3. a skip-list ────────────────────────────────────────────────────

    #[tokio::test]
    async fn skip_list_nao_e_dupla_guardada() {
        // Cross-origin nos caminhos da skip-list: o guarda genérico não
        // responde — quem responde é a guarda própria daquele caminho (aqui,
        // o handler de mentira). O corpo prova que o 403 não veio daqui.
        let router = router(estado(&[]));
        for path in PULADOS {
            let req = request(
                "POST",
                path,
                &[
                    ("host", "127.0.0.1:3888"),
                    ("origin", "http://evil.example"),
                ],
            );
            let (status, corpo) = status_corpo(router.clone(), req).await;
            assert_eq!(status, StatusCode::OK, "{path} foi duplo-guardado");
            assert_eq!(corpo, "ok");
        }
    }

    #[test]
    fn a_skip_list_nao_cresce_sem_alguem_notar() {
        // Anti-drift. A guarda é **opt-out**: rota mutante nova nasce coberta
        // sem ninguém lembrar de registrá-la — a única forma de nascer
        // desprotegida é entrar nesta lista. Então é a lista que precisa de
        // trava, e não a cobertura. Mexeu aqui, justifique no PR: cada
        // entrada tem de ter guarda própria mais estrita (senão o buraco do
        // #1182 volta por ela).
        assert_eq!(
            SEM_GUARDA,
            ["/api/learning/", "/api/plugins/", "/admin/", "/webhooks/"],
            "a skip-list mudou — cada entrada precisa de guarda propria"
        );
    }

    // ── 4. o handshake de WebSocket ───────────────────────────────────────

    #[test]
    fn ws_upgrade_permitido_decide_por_origin() {
        // Sem Origin: cliente não-navegador (app, CLI, curl).
        assert!(ws_upgrade_permitido(
            &headers_com(&[("host", "127.0.0.1:3888")]),
            "http",
            &[]
        ));
        // Origin de outra origem: o `new WebSocket` da página do atacante.
        assert!(!ws_upgrade_permitido(
            &headers_com(&[
                ("host", "127.0.0.1:3888"),
                ("origin", "http://evil.example")
            ]),
            "http",
            &[]
        ));
        // Same-origin: o webchat servido pelo próprio gateway.
        assert!(ws_upgrade_permitido(
            &headers_com(&[
                ("host", "127.0.0.1:3888"),
                ("origin", "http://127.0.0.1:3888")
            ]),
            "http",
            &[]
        ));
        // Rebinding: `Origin == Host`, nome DNS não listado.
        assert!(!ws_upgrade_permitido(
            &headers_com(&[
                ("host", "evil.example:3888"),
                ("origin", "http://evil.example:3888")
            ]),
            "http",
            &[]
        ));
        // O mesmo nome, declarado pelo dono.
        assert!(ws_upgrade_permitido(
            &headers_com(&[
                ("host", "meu.dominio:3888"),
                ("origin", "http://meu.dominio:3888")
            ]),
            "http",
            &["http://meu.dominio:3888".to_string()]
        ));
    }

    // ── 5. gramática de authority (migrada do `learning_auth`, que agora
    //    importa estas primitivas daqui) ─────────────────────────────────

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
        let headers = |valor: &str| headers_com(&[("host", valor)]);
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
