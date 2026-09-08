//! Gate de `gateway.api_key` sobre `/api/*` (#1045).
//!
//! Antes deste módulo a chave era exigida em **um** lugar só: o handshake do
//! `/ws` (`crate::ws::ws_handler`). Todo o REST de `/api/*` — sessões,
//! memória, providers, logs, diagnósticos — respondia a qualquer um que
//! alcançasse a porta. Num gateway em `0.0.0.0`, que é o caso de quem usa o
//! app no celular contra o Garra da LAN, isso é a rede inteira.
//!
//! ## Invariantes
//!
//! - **Sem chave configurada, nada muda.** O layer é montado sempre, mas com
//!   `api_key: None` ele devolve o pedido a `next` na primeira linha.
//! - **Só header.** A chave não é aceita por query string. O `/ws` aceita
//!   `?token=` porque o handshake WebSocket de um navegador não leva header;
//!   `fetch` leva, o app leva, e chave em query acaba em log de acesso, no
//!   histórico do navegador e — aqui em particular — no `uri` que o
//!   `DefaultMakeSpan` do tower-http grava no span de todo request.
//! - **Allowlist por igualdade exata**, nunca por prefixo. `/api/healthz` e
//!   `/api/health/` não são `/api/health`.
//! - **`layer`, não `route_layer`**: uma rota inexistente sob `/api/` também
//!   responde 401, para o gate não virar um mapa de quais rotas existem.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::Method;
use axum::middleware::Next;
use axum::response::Response;
use tracing::warn;

use crate::auth_common::{constant_time_token_eq, extract_bearer};

/// O prefixo que o gate cobre. `/v1/*` e `/admin/*` têm autenticação própria
/// (JWT e cookie de admin) e ficam de fora.
const PREFIXO: &str = "/api/";

/// As três rotas de `/api/` que continuam abertas com a chave configurada.
///
/// - `/api/health` e `/api/capabilities`: o onboarding do app bate nas duas
///   **antes** de o usuário digitar a chave, para descobrir se há um Garra
///   do outro lado. Ambas são declaradamente secret-free.
/// - `/api/auth-check`: é como o console web descobre que precisa pedir a
///   chave. Exigir a chave para perguntar se a chave é necessária seria um
///   ciclo.
pub const ROTAS_ABERTAS: &[&str] = &["/api/health", "/api/capabilities", "/api/auth-check"];

/// O corpo do 401. Constante: nada do que veio no pedido é ecoado.
const CORPO_401: &str = "gateway: invalid or missing api key";

/// Estado do layer. `None` em `key` significa "gate desligado".
#[derive(Clone, Default)]
pub struct ApiKeyGate {
    key: Option<Arc<str>>,
}

impl ApiKeyGate {
    /// Constrói o gate a partir da config.
    ///
    /// Chave ausente, vazia ou só com espaço são a mesma coisa: gate
    /// desligado. É a mesma normalização de `MetricsAuthConfig`, e é o que
    /// impede que um `api_key = "  "` num TOML vire um gate que ninguém
    /// consegue satisfazer.
    pub fn from_config(gateway: &garraia_config::GatewayConfig) -> Self {
        let key = gateway
            .api_key
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .map(Arc::from);
        Self { key }
    }

    /// `true` quando há chave — é o que `/api/auth-check` reporta ao console.
    pub fn is_enabled(&self) -> bool {
        self.key.is_some()
    }

    ///  quando o token apresentado serve. Publico porque o handshake
    /// do  usa a mesma decisao — com a diferenca de que la o token pode
    /// vir pela query.
    pub fn admits(&self, apresentada: Option<&str>) -> bool {
        match (&self.key, apresentada) {
            (None, _) => true,
            (Some(esperada), Some(vinda)) => {
                constant_time_token_eq(vinda.as_bytes(), esperada.as_bytes())
            }
            (Some(_), None) => false,
        }
    }
}

/// `true` quando o caminho está atrás do gate.
pub fn is_gated_path(path: &str) -> bool {
    path.starts_with(PREFIXO) && !ROTAS_ABERTAS.contains(&path)
}

/// O middleware. Montado em `build_router` depois de todos os `merge`/`nest`,
/// para cobrir também o que é montado por `build_skill_skin_routes` e por
/// `build_plugin_routes`, e antes do CORS e do rate limit.
pub async fn api_key_layer(State(gate): State<ApiKeyGate>, req: Request, next: Next) -> Response {
    if !gate.is_enabled() {
        return next.run(req).await;
    }

    let path = req.uri().path();
    // O preflight do CORS não leva header de autorização, por definição. Ele
    // já é respondido pelo `CorsLayer`, que está por fora; a checagem aqui é
    // cinto e suspensório para o caso de a ordem dos layers mudar.
    if req.method() == Method::OPTIONS || !is_gated_path(path) {
        return next.run(req).await;
    }

    if gate.admits(extract_bearer(req.headers())) {
        return next.run(req).await;
    }

    // Nunca o valor do token, nem o que veio no header: só o caminho.
    warn!(path = %path, method = %req.method(), "gateway: /api request without a valid api key");
    crate::auth_common::deny_unauthorized("garraia", CORPO_401)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request as HttpRequest, StatusCode, header};
    use axum::middleware::from_fn_with_state;
    use axum::routing::get;
    use axum::{Router, response::IntoResponse};
    use tower::ServiceExt;

    fn config_com(chave: Option<&str>) -> garraia_config::GatewayConfig {
        garraia_config::GatewayConfig {
            api_key: chave.map(str::to_string),
            ..Default::default()
        }
    }

    fn router(chave: Option<&str>) -> Router {
        let gate = ApiKeyGate::from_config(&config_com(chave));
        Router::new()
            .route("/api/sessions", get(|| async { "sessoes" }))
            .route("/api/health", get(|| async { "ok" }))
            .route("/api/capabilities", get(|| async { "caps" }))
            .route("/api/auth-check", get(|| async { "check" }))
            .route("/api/healthz", get(|| async { "pegadinha" }))
            .route("/v1/models", get(|| async { "modelos" }))
            .route("/ws", get(|| async { "socket" }))
            .route("/health", get(|| async { "raiz" }))
            .fallback(|| async { StatusCode::NOT_FOUND.into_response() })
            .layer(from_fn_with_state(gate, api_key_layer))
    }

    async fn status(chave: Option<&str>, req: HttpRequest<Body>) -> StatusCode {
        router(chave).oneshot(req).await.unwrap().status()
    }

    fn get_em(caminho: &str) -> HttpRequest<Body> {
        HttpRequest::builder()
            .uri(caminho)
            .body(Body::empty())
            .unwrap()
    }

    fn get_com_auth(caminho: &str, valor: &str) -> HttpRequest<Body> {
        HttpRequest::builder()
            .uri(caminho)
            .header(header::AUTHORIZATION, valor)
            .body(Body::empty())
            .unwrap()
    }

    // ── o gate desligado ──────────────────────────────────────────────────

    /// A garantia de "zero mudança": sem chave configurada, o gate não existe.
    #[tokio::test]
    async fn sem_chave_configurada_tudo_passa() {
        assert_eq!(status(None, get_em("/api/sessions")).await, StatusCode::OK);
        assert_eq!(status(None, get_em("/api/health")).await, StatusCode::OK);
    }

    /// Chave só com espaço em branco é o mesmo que chave nenhuma — senão o
    /// gate ficaria ligado com uma senha que ninguém consegue digitar.
    #[tokio::test]
    async fn chave_em_branco_nao_liga_o_gate() {
        for branco in ["", "   ", "\t"] {
            assert!(!ApiKeyGate::from_config(&config_com(Some(branco))).is_enabled());
            assert_eq!(
                status(Some(branco), get_em("/api/sessions")).await,
                StatusCode::OK,
                "gate ligou com a chave {branco:?}"
            );
        }
        assert!(ApiKeyGate::from_config(&config_com(Some("k"))).is_enabled());
    }

    // ── o gate ligado ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn sem_header_da_401() {
        let resp = router(Some("k1"))
            .oneshot(get_em("/api/sessions"))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            resp.headers().get(header::WWW_AUTHENTICATE).unwrap(),
            r#"Bearer realm="garraia""#
        );
    }

    #[tokio::test]
    async fn com_a_chave_certa_passa() {
        assert_eq!(
            status(Some("k1"), get_com_auth("/api/sessions", "Bearer k1")).await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn chave_errada_ou_mal_formada_da_401() {
        for valor in [
            "Bearer k2",      // chave errada
            "Bearer k1extra", // prefixo da certa nao basta
            "Bearer k",       // sufixo tambem nao
            "bearer k1",      // esquema e case-sensitive
            "Basic k1",       // esquema errado
            "k1",             // sem esquema
            "Bearer ",        // vazio depois do esquema
        ] {
            assert_eq!(
                status(Some("k1"), get_com_auth("/api/sessions", valor)).await,
                StatusCode::UNAUTHORIZED,
                "aceitou {valor:?}"
            );
        }
    }

    /// Espaco em volta do token e tolerado — e o comportamento que o
    /// `extract_bearer` do `/metrics` ja tinha, e que este gate herda por
    /// usar o mesmo helper. O que **nao** e tolerado e um token diferente:
    /// o `trim` limpa a borda, nunca o meio.
    #[tokio::test]
    async fn espaco_em_volta_do_token_e_tolerado_mas_o_token_e_exato() {
        assert_eq!(
            status(Some("k1"), get_com_auth("/api/sessions", "Bearer  k1 ")).await,
            StatusCode::OK
        );
        assert_eq!(
            status(Some("k1"), get_com_auth("/api/sessions", "Bearer  k 1 ")).await,
            StatusCode::UNAUTHORIZED
        );
    }

    /// A chave não é aceita por query string. O `/ws` aceita porque o
    /// handshake de navegador não leva header; o REST não tem essa limitação.
    #[tokio::test]
    async fn a_chave_na_query_nao_vale() {
        for uri in ["/api/sessions?api_key=k1", "/api/sessions?token=k1"] {
            assert_eq!(
                status(Some("k1"), get_em(uri)).await,
                StatusCode::UNAUTHORIZED,
                "aceitou a chave pela query em {uri}"
            );
        }
    }

    // ── a allowlist ───────────────────────────────────────────────────────

    #[tokio::test]
    async fn as_tres_rotas_abertas_passam_sem_chave() {
        for aberta in ROTAS_ABERTAS {
            assert_eq!(
                status(Some("k1"), get_em(aberta)).await,
                StatusCode::OK,
                "{aberta} deveria estar aberta"
            );
        }
    }

    /// A allowlist é por igualdade exata. Um `starts_with` aqui abriria
    /// `/api/healthz` e qualquer coisa sob `/api/health/`.
    #[tokio::test]
    async fn a_allowlist_nao_pega_prefixo_nem_sufixo() {
        assert_eq!(
            status(Some("k1"), get_em("/api/healthz")).await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            status(Some("k1"), get_em("/api/health/")).await,
            StatusCode::UNAUTHORIZED
        );
        assert!(!is_gated_path("/api/health"));
        assert!(is_gated_path("/api/healthz"));
        assert!(is_gated_path("/api/health/"));
    }

    // ── o que fica de fora ────────────────────────────────────────────────

    #[tokio::test]
    async fn fora_do_prefixo_o_gate_nao_age() {
        for aberto in ["/v1/models", "/ws", "/health"] {
            assert_eq!(
                status(Some("k1"), get_em(aberto)).await,
                StatusCode::OK,
                "{aberto} nao deveria ser gateado"
            );
        }
        assert!(!is_gated_path("/v1/models"));
        assert!(!is_gated_path("/ws"));
        assert!(!is_gated_path("/health"));
        // Nem por semelhanca: o prefixo e literal.
        assert!(!is_gated_path("/apixyz"));
        assert!(!is_gated_path("/API/sessions"));
    }

    /// Rota inexistente sob `/api/` responde 401, e não 404: o gate não conta
    /// a quem não tem a chave quais rotas existem.
    #[tokio::test]
    async fn rota_inexistente_sob_api_da_401_e_nao_404() {
        assert_eq!(
            status(Some("k1"), get_em("/api/nao-existe")).await,
            StatusCode::UNAUTHORIZED
        );
        // E com a chave, o 404 honesto.
        assert_eq!(
            status(Some("k1"), get_com_auth("/api/nao-existe", "Bearer k1")).await,
            StatusCode::NOT_FOUND
        );
    }

    /// Preflight de CORS não leva credencial. Ele já é respondido pelo
    /// `CorsLayer`, que fica por fora deste; o guarda aqui é para o caso de a
    /// ordem mudar.
    #[tokio::test]
    async fn preflight_nao_e_barrado() {
        let req = HttpRequest::builder()
            .method(Method::OPTIONS)
            .uri("/api/sessions")
            .body(Body::empty())
            .unwrap();
        assert_ne!(
            router(Some("k1")).oneshot(req).await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
    }
}
