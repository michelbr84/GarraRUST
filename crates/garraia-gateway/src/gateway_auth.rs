//! Gate de `gateway.api_key` sobre `/api/*` (#1045) e sobre o plano de
//! conversa e o A2A (#1240).
//!
//! Antes deste módulo a chave era exigida em **um** lugar só: o handshake do
//! `/ws` (`crate::ws::ws_handler`). Todo o REST de `/api/*` — sessões,
//! memória, providers, logs, diagnósticos — respondia a qualquer um que
//! alcançasse a porta. Num gateway em `0.0.0.0`, que é o caso de quem usa o
//! app no celular contra o Garra da LAN, isso é a rede inteira.
//!
//! ## Por que o gate deixou de ser um prefixo só (#1240)
//!
//! A versão original cobria `path.starts_with("/api/")` e justificava a
//! exclusão dizendo que "`/v1/*` e `/admin/*` têm autenticação própria".
//! Isso é verdade para o `rest_v1` (workspace multi-tenant, que monta os
//! próprios layers de JWT) e para o `/admin` (cookie de sessão). **Não** era
//! verdade para as rotas compat OpenAI/Anthropic nem para o A2A, que só
//! compartilham o prefixo `/v1/` com o `rest_v1` e estão montadas no mesmo
//! router cru que `/api/*`:
//!
//! - `POST /v1/chat/completions` ignora qualquer bearer e atribui o pedido ao
//!   dono da allowlist;
//! - `POST /v1/messages` e `POST /v1/messages/count_tokens` não têm resolução
//!   de identidade nenhuma;
//! - `POST /a2a/tasks` chama o runtime sem extractor de auth.
//!
//! Todas elas rodam as tools do GarraIA **na máquina do dono** (`bash` e
//! `file_write` nos modos coder/debug, sandbox `Off` por default), e o modo
//! vem por header controlado pelo chamador. Ou seja: o operador configurava
//! `gateway.api_key` acreditando ter fechado a porta, e a superfície mais
//! poderosa do processo continuava respondendo a qualquer `curl`. Um
//! fail-open de um controle que o operador ligou é pior que não ter o
//! controle — por isso o recorte virou um conjunto explícito.
//!
//! ## Invariantes
//!
//! - **Sem chave configurada, nada muda.** O layer é montado sempre, mas com
//!   `api_key: None` ele devolve o pedido a `next` na primeira linha. A
//!   #1240 **não fecha nada por default**: ela só faz a chave cobrir o que o
//!   operador já pensava que ela cobria.
//! - **Só header.** A chave não é aceita por query string. O `/ws` aceita
//!   `?token=` porque o handshake WebSocket de um navegador não leva header;
//!   `fetch` leva, o app leva, e chave em query acaba em log de acesso, no
//!   histórico do navegador e — aqui em particular — no `uri` que o
//!   `DefaultMakeSpan` do tower-http grava no span de todo request.
//! - **Allowlist por igualdade exata**, nunca por prefixo. `/api/healthz` e
//!   `/api/health/` não são `/api/health`. O mesmo vale para
//!   [`ROTAS_DE_CONVERSA`]: é uma lista nominal, e não `/v1/` inteiro, senão
//!   o gate engoliria o `rest_v1` e o `/v1/auth/*`, que têm auth própria.
//! - **`layer`, não `route_layer`**: uma rota inexistente sob `/api/` também
//!   responde 401, para o gate não virar um mapa de quais rotas existem.
//! - **A descoberta continua aberta**: `/v1/models` e
//!   `/.well-known/agent.json` são como um cliente descobre o que há do outro
//!   lado antes de ter a chave, o mesmo papel de `/api/capabilities`. Nenhuma
//!   das duas executa nada.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method};
use axum::middleware::Next;
use axum::response::Response;
use tracing::warn;

use crate::auth_common::{constant_time_token_eq, extract_bearer, extract_raw_header_token};

/// O prefixo do Web Console. `/admin/*` tem cookie de sessão próprio e fica
/// de fora; `/v1/*` **não** é coberto em bloco — ver [`ROTAS_DE_CONVERSA`].
const PREFIXO: &str = "/api/";

/// O espaço de nomes do A2A, coberto **por prefixo**.
///
/// Diferente de `/v1/`, `/a2a/` é inteiro deste gateway: não há nada de
/// terceiros ali com autenticação própria, e toda rota nova nesse espaço
/// dirige o agente. Por prefixo, rota nova nasce coberta.
///
/// `/.well-known/agent.json` — o card de descoberta — não começa com
/// `/a2a/`, então fica de fora sem precisar de exceção.
const PREFIXO_A2A: &str = "/a2a/";

/// As rotas do plano de conversa que o gate cobre (#1240), por igualdade
/// exata.
///
/// Nominal de propósito: `/v1/` também hospeda o `rest_v1` (`/v1/me`,
/// `/v1/groups`, `/v1/openapi.json`) e o `/v1/auth/*`, que têm JWT próprio e
/// respondem 503 fail-closed sem secret. Cobri-los aqui trocaria o contrato
/// de resposta deles por um 401 de outro realm.
pub const ROTAS_DE_CONVERSA: &[&str] = &[
    "/v1/chat/completions",
    "/v1/messages",
    "/v1/messages/count_tokens",
];

/// As rotas compat Anthropic, que aceitam a chave **também** por `x-api-key`.
///
/// O Claude Code e o SDK oficial da Anthropic nunca mandam
/// `Authorization: Bearer` — mandam `x-api-key`. Sem isto, gatear
/// `/v1/messages` fecharia a integração documentada em vez de autenticá-la.
/// A concessão é só destas duas rotas: um segundo header valendo no gateway
/// inteiro seria uma superfície a mais para manter em dia.
const ROTAS_ANTHROPIC: &[&str] = &["/v1/messages", "/v1/messages/count_tokens"];

/// O header que as rotas Anthropic aceitam além do bearer. **Nunca** query
/// string: o invariante do módulo vale igual aqui.
const HEADER_ANTHROPIC: &str = "x-api-key";

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
    ///
    /// **A chave é lida uma vez, no `build_router`.** Trocar
    /// `gateway.api_key` em disco não muda o gate de um processo em
    /// execução: é preciso reiniciar. O hot-reload de hoje só alcança
    /// `agent.system_prompt`, `max_tokens` e `log_level`, então não há
    /// divergência atual — mas quem estender o hot-reload precisa mexer
    /// aqui **e** em `auth_check`, que lê o mesmo campo.
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

    /// `true` quando o token apresentado serve.
    ///
    /// Publico porque o handshake do `/ws` toma a mesma decisao — com a
    /// diferenca de que la o token tambem pode vir pela query.
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
///
/// Três regras, nesta ordem de leitura: o Web Console menos a allowlist de
/// descoberta, o conjunto nominal do plano de conversa, e o espaço de nomes
/// do A2A por prefixo.
pub fn is_gated_path(path: &str) -> bool {
    (path.starts_with(PREFIXO) && !ROTAS_ABERTAS.contains(&path))
        || ROTAS_DE_CONVERSA.contains(&path)
        || path.starts_with(PREFIXO_A2A)
}

/// `true` quando algum dos headers que aquele caminho aceita traz a chave.
///
/// `Authorization: Bearer` em toda parte; `x-api-key` **só** nas rotas
/// Anthropic, e como **alternativa**, não como substituto: um cliente pode
/// mandar os dois (o SDK da Anthropic põe o seu, um proxy no caminho pode ter
/// posto o outro), e basta um deles servir.
fn credencial_admitida(gate: &ApiKeyGate, path: &str, headers: &HeaderMap) -> bool {
    if gate.admits(extract_bearer(headers)) {
        return true;
    }
    ROTAS_ANTHROPIC.contains(&path)
        && extract_raw_header_token(headers, HEADER_ANTHROPIC).is_some_and(|k| gate.admits(Some(k)))
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

    if credencial_admitida(&gate, path, req.headers()) {
        return next.run(req).await;
    }

    // Nunca o valor do token, nem o que veio no header: só o caminho.
    warn!(path = %path, method = %req.method(), "gateway: request without a valid api key");
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
            .route("/v1/chat/completions", get(|| async { "openai" }))
            .route("/v1/messages", get(|| async { "anthropic" }))
            .route("/v1/messages/count_tokens", get(|| async { "tokens" }))
            .route("/a2a/tasks", get(|| async { "a2a" }))
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

    fn get_com_x_api_key(caminho: &str, valor: &str) -> HttpRequest<Body> {
        HttpRequest::builder()
            .uri(caminho)
            .header("x-api-key", valor)
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
    async fn fora_do_conjunto_gateado_o_gate_nao_age() {
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
        assert!(!is_gated_path("/ping"));
        // A descoberta do A2A nao comeca com `/a2a/`, entao fica de fora sem
        // precisar de excecao.
        assert!(!is_gated_path("/.well-known/agent.json"));
        // O `rest_v1` e o `/v1/auth/*` tem autenticacao propria: o conjunto
        // da #1240 e nominal, nao `/v1/` inteiro.
        assert!(!is_gated_path("/v1/me"));
        assert!(!is_gated_path("/v1/auth/login"));
        assert!(!is_gated_path("/v1/openapi.json"));
        // Nem por semelhanca: o prefixo e literal.
        assert!(!is_gated_path("/apixyz"));
        assert!(!is_gated_path("/API/sessions"));
    }

    /// Os dois WebSockets ficam fora deste middleware **de proposito**, e a
    /// razao nao e "eles nao precisam de chave" — e que o middleware so
    /// aceita a chave por header, e `new WebSocket(...)` nao consegue mandar
    /// header nenhum, nem no navegador nem na webview Tauri. Por isso os
    /// clientes legitimos mandam o token pela **query string**, e os dois
    /// handlers o leem de la (`ws.rs` e `parrot_ws.rs`, via `?token=` /
    /// `?api_key=`, com bearer ainda aceito para cliente de CLI):
    ///
    ///   - `/ws`: `webchat.html` monta `?token=${encodeURIComponent(...)}`;
    ///   - `/ws/parrot`: `garraia-desktop/ui/ws.js` faz o mesmo, com a chave
    ///     vinda do comando Tauri `gateway_api_key`, que a le do mesmo
    ///     `config.yml` deste gateway.
    ///
    /// `origin_guard_layering.rs` e `auth_test.rs` travam o comportamento.
    ///
    /// Quem "consertar" a ausencia de `/ws/parrot` desta lista adicionando a
    /// rota aqui recusa os dois clientes, porque aqui a query nao e olhada;
    /// quem apagar o gate de dentro do handler reabre o buraco da auditoria
    /// R4 do PR #1251.
    #[test]
    fn os_websockets_ficam_fora_do_middleware_e_gateiam_por_dentro() {
        assert!(!is_gated_path("/ws"));
        assert!(!is_gated_path("/ws/parrot"));
    }

    // ── #1240: o plano de conversa e o A2A ────────────────────────────────

    /// As rotas compat OpenAI/Anthropic e o A2A estao no mesmo router cru
    /// que `/api/*` e executam as tools do GarraIA na maquina do dono. Antes
    /// da #1240 o gate nao as via.
    #[test]
    fn o_plano_de_conversa_e_o_a2a_sao_gateados() {
        for gateada in [
            "/v1/chat/completions",
            "/v1/messages",
            "/v1/messages/count_tokens",
            "/a2a/tasks",
            "/a2a/tasks/abc",
            "/a2a/tasks/abc/cancel",
        ] {
            assert!(is_gated_path(gateada), "{gateada} ficou de fora do gate");
        }
    }

    /// O conjunto de `/v1/` e por igualdade exata, como a allowlist de
    /// `/api/`: um `starts_with` aqui arrastaria `/v1/messages/algo-novo`
    /// (nao existe) e, pior, daria a impressao de cobrir `/v1/me`.
    #[test]
    fn o_conjunto_de_v1_e_por_igualdade_exata() {
        assert!(!is_gated_path("/v1/chat/completions/stream"));
        assert!(!is_gated_path("/v1/messagesx"));
        assert!(!is_gated_path("/v1/messages/"));
        // Ja o A2A e por prefixo, porque `/a2a/` e um espaco de nomes
        // inteiro deste gateway — rota nova ali nasce coberta.
        assert!(is_gated_path("/a2a/qualquer-coisa-nova"));
        assert!(!is_gated_path("/a2a"));
        assert!(!is_gated_path("/a2ax/tasks"));
    }

    #[tokio::test]
    async fn sem_chave_o_plano_de_conversa_nao_muda() {
        for rota in ["/v1/chat/completions", "/v1/messages", "/a2a/tasks"] {
            assert_eq!(
                status(None, get_em(rota)).await,
                StatusCode::OK,
                "{rota} passou a exigir chave sem haver chave configurada"
            );
        }
    }

    #[tokio::test]
    async fn com_chave_o_plano_de_conversa_exige_bearer() {
        for rota in ["/v1/chat/completions", "/v1/messages", "/a2a/tasks"] {
            assert_eq!(
                status(Some("k1"), get_em(rota)).await,
                StatusCode::UNAUTHORIZED,
                "{rota} respondeu sem a chave"
            );
            assert_eq!(
                status(Some("k1"), get_com_auth(rota, "Bearer k1")).await,
                StatusCode::OK,
                "{rota} recusou o bearer certo"
            );
        }
    }

    /// O `x-api-key` e aceito **so** nas rotas Anthropic, porque o Claude
    /// Code e o SDK da Anthropic nunca mandam bearer. Nas demais ele nao
    /// vale: um segundo header de autenticacao valendo em toda parte seria
    /// uma superficie a mais para manter em dia.
    #[tokio::test]
    async fn x_api_key_vale_so_nas_rotas_anthropic() {
        for anthropic in ["/v1/messages", "/v1/messages/count_tokens"] {
            assert_eq!(
                status(Some("k1"), get_com_x_api_key(anthropic, "k1")).await,
                StatusCode::OK,
                "{anthropic} recusou a chave em x-api-key"
            );
            assert_eq!(
                status(Some("k1"), get_com_x_api_key(anthropic, "k2")).await,
                StatusCode::UNAUTHORIZED,
                "{anthropic} aceitou x-api-key errada"
            );
        }
        for outra in ["/api/sessions", "/v1/chat/completions", "/a2a/tasks"] {
            assert_eq!(
                status(Some("k1"), get_com_x_api_key(outra, "k1")).await,
                StatusCode::UNAUTHORIZED,
                "{outra} aceitou x-api-key"
            );
        }
    }

    /// Bearer errado nao e "consertado" pelo `x-api-key` certo em rota que
    /// nao e Anthropic, e nas Anthropic os dois caminhos sao alternativos.
    #[tokio::test]
    async fn bearer_e_x_api_key_sao_alternativas_nas_anthropic() {
        let req = HttpRequest::builder()
            .uri("/v1/messages")
            .header(header::AUTHORIZATION, "Bearer k2")
            .header("x-api-key", "k1")
            .body(Body::empty())
            .unwrap();
        // O bearer errado nao invalida a chave certa no header que o SDK usa.
        assert_eq!(status(Some("k1"), req).await, StatusCode::OK);
    }

    /// Rota inexistente sob `/api/` responde 401, e não 404: o gate não conta
    /// a quem não tem a chave quais rotas existem. É o invariante
    /// "`layer`, não `route_layer`".
    ///
    /// **Aqui, e só aqui, o 401 prova o gate** (auditoria R4 do PR #1251).
    /// Este teste roda num router de mentira com `fallback` próprio, então o
    /// único 401 possível é o deste middleware. No `build_router` de verdade
    /// não é assim: o `require_admin_auth` que `build_plugin_routes` monta
    /// como `.layer()` (GAR-459, e portanto anterior à #1045) faz **toda**
    /// rota inexistente do gateway responder 401 — de corpo vazio. Quem for
    /// medir a cobertura do gate no router real tem de distinguir pelo
    /// **corpo** (`CORPO_401`), nunca pelo status: pelo status, mede-se o
    /// fallback e conclui-se que está tudo gateado.
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
