//! #1513 — o contrato da ponte MCP Streamable HTTP, no `build_router` de verdade.
//!
//! Os testes de unidade em `src/mcp_http/` provam as decisoes (a politica de
//! envio, a superficie anunciada, a precondicao de credencial). Só isto aqui
//! prova a **montagem**: que `/mcp` nasce por dentro do gate de
//! `gateway.api_key`, que o `initialize → tools/list → tools/call` responde
//! JSON-RPC valido, e que um `garra_send_message` sem aprovacao do operador nao
//! envia.
//!
//! Sao os criterios 1, 2 e 3 da issue, na ordem:
//!
//! 1. `POST /mcp` responde JSON-RPC com a chave e **401** sem ela.
//! 2. Um host MCP enxerga as tools em `tools/list` — o que o Paperclip faz ao
//!    apontar para `http://127.0.0.1:3888/mcp`.
//! 3. `garra_send_message` sem aprovacao nao envia (e nem aparece na lista).
//!
//! Zero Docker, zero rede, zero Postgres: `AppState::new` + `build_router`, o
//! mesmo molde de `origin_guard_layering.rs`. O que este arquivo NAO cobre e o
//! caminho de entrega de verdade (um bot do Telegram do outro lado) — o teste
//! mais fundo que da para escrever aqui prova que o portao **abriu**, e a
//! recusa passou a ser "canal fora do ar" em vez de "nao autorizado".

use std::sync::Arc;
use std::time::Duration;

use axum::body::{Body, to_bytes};
use axum::http::{Request, Response, StatusCode};
use garraia_agents::AgentRuntime;
use garraia_ask::provider_binding::mock_endpoint::{MockEndpoint, SENTINEL};
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_config::LlmProviderConfig;
use garraia_config::defaults::DEFAULT_CLOUD_MODEL;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::push_channels::PushChannelStates;
use garraia_gateway::router::build_router;
use garraia_gateway::state::AppState;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tower::ServiceExt;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, Respond, ResponseTemplate};

const CHAVE: &str = "chave-de-teste-da-ponte-mcp";
const BEARER: &str = "Bearer chave-de-teste-da-ponte-mcp";

/// O `Accept` que o transporte Streamable HTTP exige (rmcp responde 406 sem os
/// dois tipos). Um cliente MCP manda isto; e parte do contrato, nao detalhe.
const ACCEPT: &str = "application/json, text/event-stream";

/// Um chat de destino qualquer. Vira allowlist quando o teste quer o portao
/// aberto.
const DESTINO: i64 = -100_123_456;

/// Como o operador liga a ponte.
#[derive(Clone, Copy, Default)]
struct Ligacao {
    enabled: bool,
    com_credencial: bool,
    allow_send: bool,
    /// Se um destino entra em `proactive_chat_ids` de um canal telegram.
    com_allowlist: bool,
}

impl Ligacao {
    /// O default da instalacao: nada ligado.
    fn desligada() -> Self {
        Self::default()
    }

    /// A ponte no ar, somente leitura — o caso de uso comum.
    fn leitura() -> Self {
        Self {
            enabled: true,
            com_credencial: true,
            ..Self::default()
        }
    }

    fn com(mut self, allow_send: bool, com_allowlist: bool) -> Self {
        self.allow_send = allow_send;
        self.com_allowlist = com_allowlist;
        self
    }
}

fn config_de(l: Ligacao) -> AppConfig {
    let mut config = AppConfig::default();
    config.gateway.mcp_http.enabled = l.enabled;
    config.gateway.mcp_http.allow_send = l.allow_send;
    if l.com_credencial {
        config.gateway.api_key = Some(CHAVE.to_string());
    }
    if l.com_allowlist {
        config.channels.insert(
            "tg".to_string(),
            garraia_config::ChannelConfig {
                channel_type: "telegram".to_string(),
                enabled: Some(true),
                settings: [("proactive_chat_ids".to_string(), json!([DESTINO]))]
                    .into_iter()
                    .collect(),
            },
        );
    }
    config
}

/// Um `POST /mcp` no router de verdade.
///
/// O `ConnectInfo` e inserido porque o rate limiter (por fora do gate, de
/// proposito) le o IP do par: sem ele o pedido morre em 500 no governor antes
/// de qualquer camada de auth — o mesmo comentario de `api_key_gate.rs` e
/// `origin_guard_layering.rs`.
async fn post_mcp(l: Ligacao, bearer: Option<&str>, corpo: Value) -> Response<Body> {
    let (router, _state) = router_e_state(l);
    pedir(router, bearer, corpo).await
}

/// O router e o `AppState` que ele carrega, para os testes que precisam olhar o
/// estado **depois** de mais de um pedido. O `post_mcp` acima descarta o state
/// porque a maioria dos casos so olha a resposta.
fn router_e_state(l: Ligacao) -> (axum::Router, Arc<AppState>) {
    let state = Arc::new(AppState::new(
        config_de(l),
        Arc::new(AgentRuntime::new()),
        ChannelRegistry::new(),
    ));
    let admin_store = Arc::new(Mutex::new(
        AdminStore::in_memory().expect("in-memory admin store"),
    ));
    let router = build_router(
        Arc::clone(&state),
        PushChannelStates::empty(),
        admin_store,
        Arc::new(vec![0u8; 32]),
    );
    (router, state)
}

async fn pedir(router: axum::Router, bearer: Option<&str>, corpo: Value) -> Response<Body> {
    pedir_com_cabecalhos(router, bearer, &[], corpo).await
}

/// `pedir` com cabecalhos a mais — o `MCP-Protocol-Version` que um host manda
/// fora do `initialize`, por exemplo.
async fn pedir_com_cabecalhos(
    router: axum::Router,
    bearer: Option<&str>,
    cabecalhos: &[(&str, &str)],
    corpo: Value,
) -> Response<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/mcp")
        .header("accept", ACCEPT)
        .header("content-type", "application/json")
        // Loopback: o `allowed_hosts` default do rmcp so aceita loopback, e essa
        // e a propriedade que o teste tambem esta exercitando.
        .header("host", "127.0.0.1:3888");
    if let Some(b) = bearer {
        builder = builder.header("authorization", b);
    }
    for (nome, valor) in cabecalhos {
        builder = builder.header(*nome, *valor);
    }
    let mut req = builder
        .body(Body::from(corpo.to_string()))
        .expect("request");
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            [127, 0, 0, 1],
            40404,
        ))));
    router.oneshot(req).await.expect("resposta")
}

async fn corpo_json(resp: Response<Body>) -> Value {
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.expect("corpo");
    let texto = String::from_utf8_lossy(&bytes);
    // `json_response: true` da `application/json` no caminho simples; o fallback
    // para SSE existe no rmcp e este `strip_prefix` o cobre em vez de fingir que
    // nao existe.
    let cru = texto
        .lines()
        .find_map(|l| l.strip_prefix("data: "))
        .unwrap_or(&texto);
    serde_json::from_str(cru).unwrap_or_else(|e| panic!("resposta nao e JSON ({e}): {texto}"))
}

fn rpc(id: u32, metodo: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": metodo, "params": params })
}

/// O `initialize` que um host MCP manda primeiro.
fn initialize() -> Value {
    rpc(
        1,
        "initialize",
        json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": { "name": "teste-de-contrato", "version": "0" }
        }),
    )
}

/// `initialize` + `tools/list` num router recem-montado.
///
/// Stateless de proposito (ver `mcp_http::configuracao_do_transporte`): sem
/// sessao para carregar entre pedidos, `tools/list` vale sozinho. O `initialize`
/// ainda roda antes porque e o que um host de verdade faz, e porque e metade do
/// criterio 1.
async fn tools_list(l: Ligacao) -> Value {
    let init = corpo_json(post_mcp(l, Some(BEARER), initialize()).await).await;
    assert_eq!(init["jsonrpc"], "2.0", "initialize: {init}");
    assert!(
        init["result"]["capabilities"]["tools"].is_object(),
        "initialize sem a capability tools — o host nunca chamaria tools/list: {init}"
    );
    corpo_json(post_mcp(l, Some(BEARER), rpc(2, "tools/list", json!({}))).await).await
}

fn nomes_das_tools(lista: &Value) -> Vec<String> {
    lista["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("tools/list sem array de tools: {lista}"))
        .iter()
        .map(|t| t["name"].as_str().unwrap_or_default().to_string())
        .collect()
}

/// O envelope `garra.mcp.v1` de dentro do `CallToolResult`.
fn envelope(resposta: &Value) -> Value {
    let texto = resposta["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("tools/call sem conteudo de texto: {resposta}"));
    serde_json::from_str(texto).unwrap_or_else(|e| panic!("envelope nao e JSON ({e}): {texto}"))
}

async fn chamar(l: Ligacao, nome: &str, args: Value) -> Value {
    corpo_json(
        post_mcp(
            l,
            Some(BEARER),
            rpc(3, "tools/call", json!({ "name": nome, "arguments": args })),
        )
        .await,
    )
    .await
}

// ── criterio 1: a rota existe, e exige a chave ───────────────────────────────

/// No default da instalacao a ponte nao existe. **404, nao 401**: um 401 diria
/// "existe algo aqui, traga credencial", e nao existe.
#[tokio::test]
async fn desligada_por_default_a_rota_nao_existe() {
    let resp = post_mcp(Ligacao::desligada(), None, initialize()).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// A trava do `decidir_montagem`, agora no router: pedir a ponte sem configurar
/// `gateway.api_key` **nao** a abre. Sem isto, um operador que ligasse a ponte
/// num gateway sem chave publicaria a lista de conversas e o historico delas
/// para qualquer um que alcance a porta.
#[tokio::test]
async fn ligada_sem_credencial_a_rota_nao_sobe() {
    let l = Ligacao {
        enabled: true,
        com_credencial: false,
        ..Ligacao::default()
    };
    let resp = post_mcp(l, None, initialize()).await;
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

/// Criterio 1, metade negativa: com a ponte no ar e sem bearer, 401 — e o 401 e
/// o do gate do gateway, nao um erro do rmcp.
#[tokio::test]
async fn sem_bearer_a_ponte_responde_401() {
    let resp = post_mcp(Ligacao::leitura(), None, initialize()).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    let bytes = to_bytes(resp.into_body(), usize::MAX).await.expect("corpo");
    assert_eq!(
        String::from_utf8_lossy(&bytes),
        "gateway: invalid or missing api key"
    );
}

/// Chave errada tambem e 401 — o gate compara por igualdade, nao por presenca.
#[tokio::test]
async fn bearer_errado_responde_401() {
    let resp = post_mcp(Ligacao::leitura(), Some("Bearer outra-chave"), initialize()).await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

/// Criterio 1, metade positiva: com a chave, o `initialize` responde JSON-RPC
/// valido e anuncia a capability `tools`.
#[tokio::test]
async fn com_bearer_o_initialize_responde_json_rpc() {
    let resp = post_mcp(Ligacao::leitura(), Some(BEARER), initialize()).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = corpo_json(resp).await;
    assert_eq!(body["jsonrpc"], "2.0", "{body}");
    assert_eq!(body["id"], 1, "{body}");
    assert!(body["error"].is_null(), "{body}");
    assert!(
        body["result"]["protocolVersion"].is_string(),
        "initialize sem protocolVersion: {body}"
    );
    assert!(
        body["result"]["capabilities"]["tools"].is_object(),
        "{body}"
    );
}

// ── criterio 2: o host enxerga as tools ──────────────────────────────────────

/// O que o Paperclip ve ao apontar para `http://127.0.0.1:3888/mcp` numa
/// instalacao de leitura: as quatro tools de leitura da spec, e nenhuma de
/// escrita.
#[tokio::test]
async fn tools_list_mostra_as_quatro_de_leitura() {
    let nomes = nomes_das_tools(&tools_list(Ligacao::leitura()).await);
    for esperada in [
        "garra_status",
        "garra_list_chats",
        "garra_read_history",
        "garra_pair_status",
    ] {
        assert!(
            nomes.iter().any(|n| n == esperada),
            "falta {esperada}: {nomes:?}"
        );
    }
    assert_eq!(nomes.len(), 4, "{nomes:?}");
}

/// Com o operador tendo ligado o envio E liberado um destino, sao as cinco da
/// spec.
#[tokio::test]
async fn tools_list_mostra_as_cinco_quando_o_envio_esta_liberado() {
    let nomes = nomes_das_tools(&tools_list(Ligacao::leitura().com(true, true)).await);
    assert_eq!(nomes.len(), 5, "{nomes:?}");
    assert!(nomes.iter().any(|n| n == "garra_send_message"), "{nomes:?}");
}

/// Os metadados que um host na spec 2026-07-28 (o Claude Code) manda em todo
/// pedido: no HTTP stateless eles vem junto do `MCP-Protocol-Version` e do
/// `Mcp-Method` (SEP-2243), sem `initialize`.
fn meta_2026_07_28() -> Value {
    json!({
        "io.modelcontextprotocol/protocolVersion": "2026-07-28",
        "io.modelcontextprotocol/clientInfo": { "name": "teste-de-contrato", "version": "0" },
        "io.modelcontextprotocol/clientCapabilities": {}
    })
}

/// #1518 — um host na spec 2026-07-28 valida o `tools/list` contra o schema
/// novo, que torna `ttlMs` e `cacheScope` obrigatorios (SEP-2549). Sem os dois
/// ele descarta a lista inteira: a ponte fica "conectada" e sem nenhuma tool.
#[tokio::test]
async fn tools_list_na_spec_2026_07_28_leva_as_dicas_de_cache() {
    let (router, _state) = router_e_state(Ligacao::leitura());
    let lista = corpo_json(
        pedir_com_cabecalhos(
            router,
            Some(BEARER),
            &[
                ("mcp-protocol-version", "2026-07-28"),
                ("mcp-method", "tools/list"),
            ],
            rpc(2, "tools/list", json!({ "_meta": meta_2026_07_28() })),
        )
        .await,
    )
    .await;
    assert!(!nomes_das_tools(&lista).is_empty(), "{lista}");
    assert!(
        lista["result"]["ttlMs"].is_u64(),
        "ttlMs precisa ser inteiro >= 0: {lista}"
    );
    assert!(
        matches!(
            lista["result"]["cacheScope"].as_str(),
            Some("public" | "private")
        ),
        "cacheScope precisa ser public|private: {lista}"
    );
}

/// #1518 — host no protocolo legado segue recebendo o `tools/list` de sempre:
/// as dicas de cache nasceram na 2026-07-28.
#[tokio::test]
async fn tools_list_legado_segue_sem_dicas_de_cache() {
    let (router, _state) = router_e_state(Ligacao::leitura());
    let lista = corpo_json(
        pedir_com_cabecalhos(
            router,
            Some(BEARER),
            &[("mcp-protocol-version", "2025-06-18")],
            rpc(2, "tools/list", json!({})),
        )
        .await,
    )
    .await;
    assert!(!nomes_das_tools(&lista).is_empty(), "{lista}");
    assert!(
        lista["result"].get("ttlMs").is_none() && lista["result"].get("cacheScope").is_none(),
        "host legado recebeu dicas de cache: {lista}"
    );
}

/// `tools/call garra_status` de ponta a ponta: JSON-RPC valido por fora,
/// envelope `garra.mcp.v1` por dentro.
#[tokio::test]
async fn tools_call_garra_status_responde_o_envelope() {
    let resp = chamar(Ligacao::leitura(), "garra_status", json!({})).await;
    assert_eq!(resp["jsonrpc"], "2.0", "{resp}");
    assert!(resp["error"].is_null(), "{resp}");
    let env = envelope(&resp);
    assert_eq!(env["schema"], "garra.mcp.v1", "{env}");
    assert_eq!(env["ok"], true, "{env}");
    assert!(env["version"].is_string(), "{env}");
    assert!(env["channels"].is_array(), "{env}");
    // A propria ponte diz que nao pode enviar — e assim que o orquestrador
    // descobre isso sem tentar um envio.
    assert_eq!(env["mcp_http"]["send_enabled"], false, "{env}");
}

/// `garra_read_history` devolve o envelope mesmo para uma conversa que nao
/// existia: ela e criada vazia pela hidratacao, e "vazia" e uma resposta
/// honesta. O que importa aqui e o contrato — `redacted: true` sempre.
#[tokio::test]
async fn tools_call_read_history_declara_a_redacao() {
    let resp = chamar(
        Ligacao::leitura(),
        "garra_read_history",
        json!({ "chat": "conversa-que-nao-existe" }),
    )
    .await;
    let env = envelope(&resp);
    assert_eq!(env["ok"], true, "{env}");
    assert_eq!(env["redacted"], true, "{env}");
    assert_eq!(env["messages"], json!([]), "{env}");
}

/// O `initialize` diz que o servidor e o GarraIA, nao o SDK.
///
/// `ServerInfo::new` do rmcp preenche `server_info` com
/// `Implementation::from_build_env()`, que resolve para o `CARGO_PKG_*` do
/// **rmcp** — sondando uma ponte real antes desta correcao, o `initialize`
/// respondia `{"name":"rmcp","version":"3.3.0"}`. Esse e o nome que o host
/// mostra ao usuario na lista de servidores conectados: todo servidor escrito
/// em rmcp apareceria como o mesmo "rmcp", e quem tivesse dois na maquina nao
/// saberia qual e qual. O teste existe porque a regressao e silenciosa —
/// nenhuma tool para de funcionar quando o nome volta a ser do SDK.
#[tokio::test]
async fn o_initialize_identifica_o_garraia_e_nao_o_sdk() {
    let body = corpo_json(post_mcp(Ligacao::leitura(), Some(BEARER), initialize()).await).await;
    let info = &body["result"]["serverInfo"];
    assert_eq!(info["name"], "garraia-gateway", "{body}");
    assert_eq!(info["version"], env!("CARGO_PKG_VERSION"), "{body}");
    // E as instrucoes contam ao host que o envio e restrito, antes de ele
    // tentar.
    let instrucoes = body["result"]["instructions"]
        .as_str()
        .unwrap_or_else(|| panic!("initialize sem instructions: {body}"));
    assert!(instrucoes.contains("garra_send_message"), "{instrucoes}");
}

/// Ler o historico de um `chat` que nao existe **nao cria** conversa.
///
/// `hydrate_session_history` cria a sessao em memoria quando ela falta, o que e
/// certo para os caminhos que abrem um turno e errado numa leitura vinda de
/// fora: cada id inventado passaria a ocupar espaco no `DashMap` de sessoes e a
/// aparecer no `garra_list_chats` como se fosse conversa. O rate limit por IP
/// atrasa isso; nao poe teto.
///
/// Os dois pedidos correm contra o MESMO `AppState`, senao o teste nao veria o
/// crescimento que ele existe para negar.
#[tokio::test]
async fn ler_historico_de_chat_inexistente_nao_cria_sessao() {
    let (router, state) = router_e_state(Ligacao::leitura());
    for inventado in ["nao-existe-1", "nao-existe-2"] {
        let resp = pedir(
            router.clone(),
            Some(BEARER),
            rpc(
                3,
                "tools/call",
                json!({
                    "name": "garra_read_history",
                    "arguments": { "chat": inventado }
                }),
            ),
        )
        .await;
        let env = envelope(&corpo_json(resp).await);
        assert_eq!(env["ok"], true, "{env}");
        assert_eq!(env["messages"], json!([]), "{env}");
    }
    assert_eq!(
        state.sessions.len(),
        0,
        "leitura de chat inexistente deixou sessao para tras: {:?}",
        state
            .sessions
            .iter()
            .map(|e| e.id.clone())
            .collect::<Vec<_>>()
    );

    // E o `garra_list_chats` nao passa a listar os ids inventados.
    let lista = envelope(
        &corpo_json(
            pedir(
                router,
                Some(BEARER),
                rpc(
                    4,
                    "tools/call",
                    json!({ "name": "garra_list_chats", "arguments": {} }),
                ),
            )
            .await,
        )
        .await,
    );
    assert_eq!(lista["chats"], json!([]), "{lista}");
}

/// A contraparte: uma conversa que existe de verdade **e** lida, e a limpeza
/// acima nao a remove. Sem este par, "nao cria sessao" poderia ser satisfeito
/// por um `read_history` que nunca le nada.
#[tokio::test]
async fn ler_historico_de_chat_que_existe_devolve_a_conversa_e_a_preserva() {
    let (router, state) = router_e_state(Ligacao::leitura());
    // Como o web chat grava antes de um turno.
    state
        .hydrate_session_history("conversa-de-verdade", Some("web"), None)
        .await;
    assert_eq!(state.sessions.len(), 1);

    let env = envelope(
        &corpo_json(
            pedir(
                router,
                Some(BEARER),
                rpc(
                    3,
                    "tools/call",
                    json!({
                        "name": "garra_read_history",
                        "arguments": { "chat": "conversa-de-verdade" }
                    }),
                ),
            )
            .await,
        )
        .await,
    );
    assert_eq!(env["ok"], true, "{env}");
    assert_eq!(env["chat"], "conversa-de-verdade", "{env}");
    assert_eq!(
        state.sessions.len(),
        1,
        "a limpeza removeu uma sessao que existia antes do pedido"
    );
}

/// Argumento invalido e erro de **protocolo** (bug do chamador), nao resultado
/// de tool: o `deny_unknown_fields` chega ao JSON-RPC como `error`.
#[tokio::test]
async fn argumento_desconhecido_vira_erro_de_protocolo() {
    let resp = chamar(
        Ligacao::leitura(),
        "garra_read_history",
        json!({ "chat": "x", "campo_inventado": 1 }),
    )
    .await;
    assert!(
        !resp["error"].is_null(),
        "campo desconhecido passou: {resp}"
    );
}

#[tokio::test]
async fn tool_inexistente_vira_erro_de_protocolo() {
    let resp = chamar(Ligacao::leitura(), "garra_formata_o_disco", json!({})).await;
    assert!(!resp["error"].is_null(), "{resp}");
}

// ── criterio 3: sem aprovacao, nao envia ─────────────────────────────────────

/// O coracao do criterio 3. Na instalacao de leitura, `garra_send_message`:
///
/// - **nao aparece** em `tools/list` (o modelo do outro lado nao planeja em
///   cima de uma capacidade que nao existe); e
/// - **recusa** quando chamada direto, com o motivo estavel `send_disabled`.
///
/// As duas metades importam: "nao anunciada" sozinho seria seguranca por
/// obscuridade — um chamador que leu a doc chamaria pelo nome.
#[tokio::test]
async fn send_message_sem_aprovacao_nao_envia() {
    let l = Ligacao::leitura();
    let nomes = nomes_das_tools(&tools_list(l).await);
    assert!(
        !nomes.iter().any(|n| n == "garra_send_message"),
        "tool de envio anunciada num Garra que recusa todo envio: {nomes:?}"
    );

    let resp = chamar(
        l,
        "garra_send_message",
        json!({ "channel": "telegram", "chat_id": DESTINO, "text": "nao devia sair" }),
    )
    .await;
    let env = envelope(&resp);
    assert_eq!(env["ok"], false, "{env}");
    assert_eq!(env["error"]["kind"], "send_disabled", "{env}");
    assert_eq!(
        resp["result"]["isError"],
        json!(true),
        "a recusa tem de chegar como isError para o host: {resp}"
    );
}

/// O erro de configuracao mais provavel: o interruptor ligado e a allowlist
/// esquecida. **Nao envia**, e o motivo nomeia a allowlist — `allow_send`
/// sozinho nunca foi suficiente.
#[tokio::test]
async fn interruptor_ligado_sem_allowlist_nao_envia() {
    let l = Ligacao::leitura().com(true, false);
    let nomes = nomes_das_tools(&tools_list(l).await);
    assert!(
        !nomes.iter().any(|n| n == "garra_send_message"),
        "{nomes:?}"
    );

    let env = envelope(
        &chamar(
            l,
            "garra_send_message",
            json!({ "channel": "telegram", "chat_id": DESTINO, "text": "nao devia sair" }),
        )
        .await,
    );
    assert_eq!(env["ok"], false, "{env}");
    assert_eq!(env["error"]["kind"], "no_allowlist", "{env}");
}

/// Com tudo liberado, um destino **fora** da allowlist continua recusado. E o
/// que separa "o operador ligou o envio" de "o operador aprovou este destino".
#[tokio::test]
async fn destino_fora_da_allowlist_nao_envia_mesmo_com_tudo_ligado() {
    let env = envelope(
        &chamar(
            Ligacao::leitura().com(true, true),
            "garra_send_message",
            json!({ "channel": "telegram", "chat_id": 999_999, "text": "nao devia sair" }),
        )
        .await,
    );
    assert_eq!(env["ok"], false, "{env}");
    assert_eq!(env["error"]["kind"], "target_not_allowed", "{env}");
}

/// O GREEN do RED→GREEN: com o interruptor ligado e o destino aprovado, a
/// autorizacao **deixa** de ser o motivo da recusa — o que sobra e o canal nao
/// estar registrado neste gateway de teste (`channel_offline`).
///
/// E o mais fundo que se prova sem um bot do Telegram do outro lado, e e o que
/// distingue um gate implementado de um `return Err` universal: se o portao
/// nunca abrisse, este teste veria `send_disabled` aqui.
#[tokio::test]
async fn com_destino_aprovado_o_portao_abre_e_a_recusa_passa_a_ser_entrega() {
    let env = envelope(
        &chamar(
            Ligacao::leitura().com(true, true),
            "garra_send_message",
            json!({ "channel": "telegram", "chat_id": DESTINO, "text": "oi" }),
        )
        .await,
    );
    assert_eq!(env["ok"], false, "{env}");
    assert_eq!(
        env["error"]["kind"], "channel_offline",
        "a autorizacao ainda esta barrando um destino aprovado: {env}"
    );
}

/// Canal sem allowlist de destino nao herda a do Telegram.
#[tokio::test]
async fn canal_sem_allowlist_nao_envia() {
    let env = envelope(
        &chamar(
            Ligacao::leitura().com(true, true),
            "garra_send_message",
            json!({ "channel": "discord", "chat_id": DESTINO, "text": "oi" }),
        )
        .await,
    );
    assert_eq!(env["ok"], false, "{env}");
    // O schema anuncia `enum: ["telegram"]`, e o schema e consultivo em MCP —
    // entao o canal e recusado no servidor, nao no anuncio.
    assert_eq!(env["error"]["kind"], "channel_unsupported", "{env}");
}

/// Nenhuma recusa ecoa o destino. Um `chat_id` recusado ainda e a conversa de
/// uma pessoa real, e a resposta vai para um chamador de fora.
#[tokio::test]
async fn nenhuma_recusa_ecoa_o_destino() {
    for l in [
        Ligacao::leitura(),
        Ligacao::leitura().com(true, false),
        Ligacao::leitura().com(true, true),
    ] {
        let env = envelope(
            &chamar(
                l,
                "garra_send_message",
                json!({ "channel": "telegram", "chat_id": DESTINO, "text": "oi" }),
            )
            .await,
        );
        let texto = env.to_string();
        assert!(
            !texto.contains(&DESTINO.to_string()),
            "a resposta ecoou o destino: {texto}"
        );
    }
}

/// O marcador que nao pode aparecer na resposta. E propositalmente impossivel de
/// confundir com credencial de verdade (pontos e maiusculas fora do alfabeto de
/// qualquer token conhecido) — o que importa e que ele venha do erro do canal.
const MARCADOR_DO_ERRO: &str = "MARCADOR.QUE.NAO.PODE.SAIR.do-teste";

/// Um canal que sempre falha a entrega, para alcancar o unico ramo que os testes
/// acima nao alcancam: `delivery_failed`. O erro imita o formato que um canal
/// real produz — uma URL de servico externo, com o destino no query string.
struct CanalQueFalha;

#[async_trait::async_trait]
impl garraia_channels::Channel for CanalQueFalha {
    fn channel_type(&self) -> &str {
        "telegram"
    }
    fn display_name(&self) -> &str {
        "Telegram (falso)"
    }
    async fn connect(&mut self) -> garraia_common::Result<()> {
        Ok(())
    }
    async fn disconnect(&mut self) -> garraia_common::Result<()> {
        Ok(())
    }
    async fn send_message(&self, _message: &garraia_common::Message) -> garraia_common::Result<()> {
        Err(garraia_common::Error::Channel(format!(
            "telegram send failed: A network error: error sending request for url \
             (https://api.telegram.org/bot{MARCADOR_DO_ERRO}/sendMessage?chat_id={DESTINO})"
        )))
    }
    fn status(&self) -> garraia_channels::ChannelStatus {
        garraia_channels::ChannelStatus::Connected
    }
}

/// A falha de **entrega** tambem nao ecoa o erro do canal.
///
/// Este e o ramo que `nenhuma_recusa_ecoa_o_destino` nao alcanca: la o canal nem
/// esta registrado, entao a recusa para em `channel_offline`. Aqui o portao abre
/// de verdade, a tool chama o canal, e o canal falha — e e nesse ponto que a
/// tentacao de devolver `e.to_string()` aparece. O texto do erro e de um servico
/// externo: hoje o teloxide mascara o token do bot, mas isso e garantia de UMA
/// dependencia, e cada canal novo traz o proprio formato. O chamador recebe
/// codigo estavel; o motivo fica no log do dono.
#[tokio::test]
async fn falha_de_entrega_nao_ecoa_o_erro_do_canal() {
    let (router, state) = router_e_state(Ligacao::leitura().com(true, true));
    state
        .channels
        .write()
        .await
        .register(Box::new(CanalQueFalha));

    let env = envelope(
        &corpo_json(
            pedir(
                router,
                Some(BEARER),
                rpc(
                    3,
                    "tools/call",
                    json!({
                        "name": "garra_send_message",
                        "arguments": { "channel": "telegram", "chat_id": DESTINO, "text": "oi" }
                    }),
                ),
            )
            .await,
        )
        .await,
    );

    assert_eq!(env["ok"], false, "{env}");
    assert_eq!(
        env["error"]["kind"], "delivery_failed",
        "o portao nao abriu, entao este teste nao esta provando o que diz: {env}"
    );
    let texto = env.to_string();
    assert!(
        !texto.contains(MARCADOR_DO_ERRO),
        "a resposta ecoou o erro do canal, com credencial dentro: {texto}"
    );
    assert!(
        !texto.contains(&DESTINO.to_string()),
        "a resposta ecoou o destino pela via do erro do canal: {texto}"
    );
    assert!(
        !texto.contains("api.telegram.org"),
        "a resposta ecoou a URL do servico externo: {texto}"
    );
}

// ── garra_ask (#1612): a ponte chama o LLM so com allow_ask ───────────────────

/// Um router com a config dada. Sem o `ConnectInfo` do `post_mcp`: estes testes
/// montam o router uma vez e o reusam entre pedidos (o teto de chamadas mora
/// nele), e quem faz o pedido injeta o par em `pedir_ask`.
fn router_com(config: AppConfig) -> axum::Router {
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
        PushChannelStates::empty(),
        admin_store,
        Arc::new(vec![0u8; 32]),
    )
}

/// A config da ponte com `garra_ask`, e um `llm.openrouter` apontando para o
/// endpoint falso. A chave `k-do-dono` e a que o provider deve receber, e nenhuma
/// outra.
fn config_com_ask(
    endereco_falso: &str,
    allow_ask: bool,
    modelos: &[&str],
    orcamento: u32,
) -> AppConfig {
    let mut config = config_de(Ligacao::leitura());
    config.gateway.mcp_http.allow_ask = allow_ask;
    config.gateway.mcp_http.ask_allowed_models = modelos.iter().map(|m| m.to_string()).collect();
    config.gateway.mcp_http.ask_budget_per_minute = orcamento;
    config.llm.insert(
        "openrouter".to_string(),
        LlmProviderConfig {
            provider: "openrouter".to_string(),
            model: Some(DEFAULT_CLOUD_MODEL.to_string()),
            api_key: Some("k-do-dono".to_string()),
            base_url: Some(format!("{endereco_falso}/api/v1")),
            extra: Default::default(),
        },
    );
    config
}

/// `garra_ask` como um `tools/call` no router dado. O router nao e consumido: o
/// teste pode fazer varios pedidos no mesmo, que e o que o teto de chamadas exige.
async fn pedir_ask(router: &axum::Router, args: Value) -> Value {
    corpo_json(
        pedir(
            router.clone(),
            Some(BEARER),
            rpc(
                3,
                "tools/call",
                json!({ "name": "garra_ask", "arguments": args }),
            ),
        )
        .await,
    )
    .await
}

/// O `tools/list` de um router ja montado, depois do `initialize` que um host manda.
async fn tools_list_de(router: &axum::Router) -> Value {
    let _ = corpo_json(pedir(router.clone(), Some(BEARER), initialize()).await).await;
    corpo_json(
        pedir(
            router.clone(),
            Some(BEARER),
            rpc(2, "tools/list", json!({})),
        )
        .await,
    )
    .await
}

/// O erro de `garra_ask` de dentro do envelope `garra.ask.v1`, ou falha de teste.
fn erro_ask(resposta: &Value) -> Value {
    assert_eq!(
        resposta["result"]["isError"], true,
        "garra_ask deveria ter falhado: {resposta}"
    );
    let env = envelope(resposta);
    assert_eq!(env["schema"], "garra.ask.v1", "{env}");
    assert_eq!(env["ok"], false, "{env}");
    env["error"].clone()
}

/// A tool so aparece com `allow_ask`: o default da instalacao nao anuncia nada.
#[tokio::test]
async fn garra_ask_so_e_anunciada_com_allow_ask() {
    let endereco = MockEndpoint::start().await;
    let desligada = router_com(config_com_ask(&endereco.uri(), false, &[], 10));
    let ligada = router_com(config_com_ask(&endereco.uri(), true, &[], 10));

    let nomes_off = nomes_das_tools(&tools_list_de(&desligada).await);
    assert!(!nomes_off.iter().any(|n| n == "garra_ask"), "{nomes_off:?}");

    let nomes_on = nomes_das_tools(&tools_list_de(&ligada).await);
    assert!(nomes_on.iter().any(|n| n == "garra_ask"), "{nomes_on:?}");
}

/// O caminho feliz: sem `model` o pedido vai para o modelo default do projeto,
/// com a chave do dono, e a resposta sai como o envelope `garra.ask.v1` do stdio.
#[tokio::test]
async fn garra_ask_sem_modelo_usa_o_default_do_projeto_e_a_chave_do_dono() {
    let endereco = MockEndpoint::start().await;
    let router = router_com(config_com_ask(&endereco.uri(), true, &[], 10));

    let resposta = pedir_ask(&router, json!({ "message": "oi" })).await;

    assert_ne!(resposta["result"]["isError"], true, "{resposta}");
    let env = envelope(&resposta);
    assert_eq!(env["schema"], "garra.ask.v1", "{env}");
    assert_eq!(env["ok"], true, "{env}");
    assert_eq!(env["answer"], SENTINEL, "{env}");
    assert_eq!(env["provider"], "openrouter", "{env}");
    assert_eq!(env["model"], DEFAULT_CLOUD_MODEL, "{env}");

    let credenciais = endereco.credentials().await;
    assert!(
        !credenciais.is_empty() && credenciais.iter().all(|c| c == "k-do-dono"),
        "o provider recebeu {credenciais:?}"
    );
}

/// Sem lista, um modelo que nao e o default e recusado **antes** de qualquer
/// pedido sair: o endpoint do provider nao recebe nada.
#[tokio::test]
async fn garra_ask_modelo_fora_da_lista_nao_chega_ao_provider() {
    let endereco = MockEndpoint::start().await;
    let router = router_com(config_com_ask(&endereco.uri(), true, &[], 10));

    let resposta = pedir_ask(
        &router,
        json!({ "message": "oi", "model": "openrouter/auto" }),
    )
    .await;

    let erro = erro_ask(&resposta);
    assert_eq!(erro["kind"], "model_not_allowed", "{erro}");
    assert!(
        endereco.credentials().await.is_empty(),
        "o modelo recusado chegou ao provider"
    );
}

/// Com lista, o modelo listado passa e o que nao esta nela continua recusado.
#[tokio::test]
async fn garra_ask_com_lista_aceita_o_modelo_listado() {
    let endereco = MockEndpoint::start().await;
    let router = router_com(config_com_ask(
        &endereco.uri(),
        true,
        &["openrouter/auto"],
        10,
    ));

    let ok = pedir_ask(
        &router,
        json!({ "message": "oi", "model": "openrouter/auto" }),
    )
    .await;
    let env = envelope(&ok);
    assert_eq!(env["ok"], true, "{env}");
    assert_eq!(env["model"], "openrouter/auto", "{env}");

    // A lista substituiu o default: o modelo do projeto agora e recusado.
    let fora = pedir_ask(&router, json!({ "message": "oi" })).await;
    assert_eq!(erro_ask(&fora)["kind"], "model_not_allowed");
}

/// Com `allow_ask` desligado a tool nao e anunciada, mas uma chamada direta a ela
/// e recusada pelo mesmo motivo — e nada chega ao provider.
#[tokio::test]
async fn garra_ask_desligada_recusa_mesmo_quando_chamada_direto() {
    let endereco = MockEndpoint::start().await;
    let router = router_com(config_com_ask(&endereco.uri(), false, &[], 10));

    let resposta = pedir_ask(&router, json!({ "message": "oi" })).await;

    assert_eq!(erro_ask(&resposta)["kind"], "ask_disabled");
    assert!(endereco.credentials().await.is_empty());
}

/// O teto por minuto: a segunda chamada e recusada como `over_budget`, e a
/// recusa nao gasta inferencia — o provider so viu a primeira.
#[tokio::test]
async fn garra_ask_teto_por_minuto_recusa_a_seguinte_sem_gastar() {
    let endereco = MockEndpoint::start().await;
    let router = router_com(config_com_ask(&endereco.uri(), true, &[], 1));

    let primeira = pedir_ask(&router, json!({ "message": "oi" })).await;
    assert_eq!(envelope(&primeira)["ok"], true, "{primeira}");
    assert_eq!(endereco.credentials().await.len(), 1);

    let segunda = pedir_ask(&router, json!({ "message": "de novo" })).await;
    assert_eq!(erro_ask(&segunda)["kind"], "over_budget");
    assert_eq!(
        endereco.credentials().await.len(),
        1,
        "a recusa por teto chegou ao provider"
    );
}

/// Os argumentos seguem o contrato do stdio: provider fora do enum e campo
/// desconhecido sao erro de protocolo, nao chegam ao provider.
#[tokio::test]
async fn garra_ask_argumento_invalido_vira_erro_de_protocolo() {
    let endereco = MockEndpoint::start().await;
    let router = router_com(config_com_ask(&endereco.uri(), true, &[], 10));

    let provider_estranho = pedir_ask(
        &router,
        json!({ "message": "oi", "provider": "minha-entrada-llm" }),
    )
    .await;
    assert!(
        provider_estranho["error"].is_object(),
        "{provider_estranho}"
    );

    let campo_extra = pedir_ask(&router, json!({ "message": "oi", "modelo": "x" })).await;
    assert!(campo_extra["error"].is_object(), "{campo_extra}");

    assert!(endereco.credentials().await.is_empty());
}

// ── #1613: orquestradores externos, cada um com identidade e politica ─────────

/// Um orquestrador de teste. O token vai para uma variavel de ambiente que so
/// este teste usa, e a config guarda so o nome dela — como em producao.
fn orquestrador(
    nome: &str,
    var: &str,
    token: &str,
    tools: &[&str],
    chats: &[i64],
) -> garraia_config::OrchestratorMcpHttp {
    // SAFETY: nome de variavel exclusivo do teste que o chama.
    unsafe { std::env::set_var(var, token) };
    garraia_config::OrchestratorMcpHttp {
        nome: nome.to_string(),
        key_env: var.to_string(),
        tools: tools.iter().map(|t| t.to_string()).collect(),
        chats: chats.to_vec(),
    }
}

/// A ponte no ar com `orquestradores`, e a allowlist global de destinos do
/// canal telegram dada. O `allow_send` segue a ligacao.
fn config_com_orquestradores(
    l: Ligacao,
    orquestradores: Vec<garraia_config::OrchestratorMcpHttp>,
    destinos_globais: &[i64],
) -> AppConfig {
    let mut config = config_de(l);
    config.gateway.mcp_http.orchestrators = orquestradores;
    config.channels.insert(
        "tg".to_string(),
        garraia_config::ChannelConfig {
            channel_type: "telegram".to_string(),
            enabled: Some(true),
            settings: [("proactive_chat_ids".to_string(), json!(destinos_globais))]
                .into_iter()
                .collect(),
        },
    );
    config
}

/// O router e o `AppState` de uma config pronta (sem passar pela `Ligacao`).
fn router_e_estado(config: AppConfig) -> (axum::Router, Arc<AppState>) {
    let state = Arc::new(AppState::new(
        config,
        Arc::new(AgentRuntime::new()),
        ChannelRegistry::new(),
    ));
    let admin_store = Arc::new(Mutex::new(
        AdminStore::in_memory().expect("in-memory admin store"),
    ));
    let router = build_router(
        Arc::clone(&state),
        PushChannelStates::empty(),
        admin_store,
        Arc::new(vec![0u8; 32]),
    );
    (router, state)
}

/// Um `tools/call` com o token dado como `Authorization`.
async fn chamar_como(router: &axum::Router, token: &str, nome: &str, args: Value) -> Value {
    let bearer = format!("Bearer {token}");
    corpo_json(
        pedir(
            router.clone(),
            Some(bearer.as_str()),
            rpc(3, "tools/call", json!({ "name": nome, "arguments": args })),
        )
        .await,
    )
    .await
}

/// Os nomes que o `tools/list` mostra para o token dado.
async fn listar_como(router: &axum::Router, token: &str) -> Vec<String> {
    let bearer = format!("Bearer {token}");
    let _ = corpo_json(pedir(router.clone(), Some(bearer.as_str()), initialize()).await).await;
    nomes_das_tools(
        &corpo_json(
            pedir(
                router.clone(),
                Some(bearer.as_str()),
                rpc(2, "tools/list", json!({})),
            )
            .await,
        )
        .await,
    )
}

/// Um canal que guarda o texto que sairia: a prova de que o payload chega
/// redigido, e o caminho de envio de verdade (nao a recusa).
struct CanalQueGuarda(Arc<Mutex<Vec<String>>>);

#[async_trait::async_trait]
impl garraia_channels::Channel for CanalQueGuarda {
    fn channel_type(&self) -> &str {
        "telegram"
    }
    fn display_name(&self) -> &str {
        "Telegram (guarda)"
    }
    async fn connect(&mut self) -> garraia_common::Result<()> {
        Ok(())
    }
    async fn disconnect(&mut self) -> garraia_common::Result<()> {
        Ok(())
    }
    async fn send_message(&self, message: &garraia_common::Message) -> garraia_common::Result<()> {
        if let garraia_common::MessageContent::Text(texto) = &message.content {
            self.0.lock().await.push(texto.clone());
        }
        Ok(())
    }
    fn status(&self) -> garraia_channels::ChannelStatus {
        garraia_channels::ChannelStatus::Connected
    }
}

/// O orquestrador ve so as tools da sua lista. O dono continua vendo as de
/// leitura, como antes.
#[tokio::test]
async fn orquestrador_ve_so_as_tools_da_sua_lista() {
    let config = config_com_orquestradores(
        Ligacao::leitura(),
        vec![orquestrador(
            "badgood-lista",
            "GARRA_TESTE_1613_LISTA",
            "tok-lista-badgood",
            &["status", "list_chats"],
            &[],
        )],
        &[],
    );
    let (router, _estado) = router_e_estado(config);

    assert_eq!(
        listar_como(&router, "tok-lista-badgood").await,
        vec!["garra_status", "garra_list_chats"]
    );
    assert_eq!(listar_como(&router, CHAVE).await.len(), 4);
}

/// Tool que existe mas nao esta na lista do orquestrador: recusa como
/// `tool_not_allowed`, com `isError`, e a tool nem roda.
#[tokio::test]
async fn tool_fora_da_lista_do_orquestrador_e_recusada() {
    let config = config_com_orquestradores(
        Ligacao::leitura(),
        vec![orquestrador(
            "badgood-tool",
            "GARRA_TESTE_1613_TOOL",
            "tok-tool-badgood",
            &["status"],
            &[],
        )],
        &[],
    );
    let (router, _estado) = router_e_estado(config);

    let resposta = chamar_como(&router, "tok-tool-badgood", "garra_pair_status", json!({})).await;
    assert_eq!(resposta["result"]["isError"], json!(true), "{resposta}");
    assert_eq!(
        envelope(&resposta)["error"]["kind"],
        "tool_not_allowed",
        "{resposta}"
    );
}

/// Credencial que nao identifica ninguem nao entra na ponte: 401, igual a
/// chave errada. Um orquestrador cuja variavel nao existe tambem nao entra.
#[tokio::test]
async fn token_que_nao_identifica_ninguem_da_401() {
    let config = config_com_orquestradores(
        Ligacao::leitura(),
        vec![
            orquestrador(
                "badgood-401",
                "GARRA_TESTE_1613_401",
                "tok-401-badgood",
                &["status"],
                &[],
            ),
            garraia_config::OrchestratorMcpHttp {
                nome: "sem-variavel".to_string(),
                key_env: "GARRA_TESTE_1613_NUNCA_DEFINIDA".to_string(),
                tools: vec!["status".to_string()],
                chats: vec![],
            },
        ],
        &[],
    );
    let (router, _estado) = router_e_estado(config);

    let desconhecida = pedir(
        router.clone(),
        Some("Bearer tok-que-ninguem-tem"),
        initialize(),
    )
    .await;
    assert_eq!(desconhecida.status(), StatusCode::UNAUTHORIZED);

    let sem_variavel = pedir(
        router,
        Some("Bearer GARRA_TESTE_1613_NUNCA_DEFINIDA"),
        initialize(),
    )
    .await;
    assert_eq!(sem_variavel.status(), StatusCode::UNAUTHORIZED);
}

/// Interseccao de destinos: o chat precisa estar na lista do orquestrador
/// **e** na allowlist global. O que falta em uma das duas nao envia, e a
/// recusa de quem nao esta na lista dele nao revela a global.
#[tokio::test]
async fn destino_precisa_estar_nas_duas_listas() {
    const OUTRO: i64 = -100_987_654;
    let mut config = config_com_orquestradores(
        Ligacao::leitura().com(true, true),
        vec![orquestrador(
            "badgood-dest",
            "GARRA_TESTE_1613_DEST",
            "tok-dest-badgood",
            &["send_message"],
            &[DESTINO],
        )],
        &[DESTINO, OUTRO],
    );
    config.gateway.mcp_http.allow_send = true;
    let (router, _estado) = router_e_estado(config);

    let envio = |chat: i64| json!({ "channel": "telegram", "chat_id": chat, "text": "oi" });
    let nas_duas = envelope(
        &chamar_como(
            &router,
            "tok-dest-badgood",
            "garra_send_message",
            envio(DESTINO),
        )
        .await,
    );
    assert_eq!(
        nas_duas["error"]["kind"], "channel_offline",
        "a interseccao passou e o portao abriu: {nas_duas}"
    );

    let so_na_global = envelope(
        &chamar_como(
            &router,
            "tok-dest-badgood",
            "garra_send_message",
            envio(OUTRO),
        )
        .await,
    );
    assert_eq!(so_na_global["error"]["kind"], "destination_not_allowed");

    let estranho = envelope(
        &chamar_como(
            &router,
            "tok-dest-badgood",
            "garra_send_message",
            envio(999_999),
        )
        .await,
    );
    assert_eq!(estranho["error"]["kind"], "destination_not_allowed");
    assert_eq!(
        so_na_global["error"]["message"], estranho["error"]["message"],
        "a recusa revelou a allowlist global"
    );
}

/// O outro lado da interseccao: o chat esta na lista do orquestrador, mas nao
/// na global. Quem barra e a global, com o motivo de sempre.
#[tokio::test]
async fn destino_so_na_lista_do_orquestrador_e_barrado_pela_global() {
    const SO_DELE: i64 = -100_555_555;
    let mut config = config_com_orquestradores(
        Ligacao::leitura().com(true, true),
        vec![orquestrador(
            "badgood-global",
            "GARRA_TESTE_1613_GLOBAL",
            "tok-global-badgood",
            &["send_message"],
            &[DESTINO, SO_DELE],
        )],
        &[DESTINO],
    );
    config.gateway.mcp_http.allow_send = true;
    let (router, _estado) = router_e_estado(config);

    let env = envelope(
        &chamar_como(
            &router,
            "tok-global-badgood",
            "garra_send_message",
            json!({ "channel": "telegram", "chat_id": SO_DELE, "text": "oi" }),
        )
        .await,
    );
    assert_eq!(env["error"]["kind"], "target_not_allowed", "{env}");
}

/// A tool de envio nao e anunciada para um orquestrador sem destino em comum
/// com a global: anunciar uma capacidade que toda chamada recusa e o erro que
/// a ponte evita desde a #1513.
#[tokio::test]
async fn envio_nao_e_anunciado_sem_destino_em_comum() {
    let mut config = config_com_orquestradores(
        Ligacao::leitura().com(true, true),
        vec![orquestrador(
            "badgood-anuncio",
            "GARRA_TESTE_1613_ANUNCIO",
            "tok-anuncio-badgood",
            &["send_message", "status"],
            &[-100_777_777],
        )],
        &[DESTINO],
    );
    config.gateway.mcp_http.allow_send = true;
    let (router, _estado) = router_e_estado(config);

    let nomes = listar_como(&router, "tok-anuncio-badgood").await;
    assert_eq!(nomes, vec!["garra_status"], "{nomes:?}");
}

/// O texto que sai pelo canal e o redigido; o texto limpo sai identico.
#[tokio::test]
async fn payload_de_envio_sai_redigido_e_o_limpo_sai_identico() {
    let mut config = config_com_orquestradores(
        Ligacao::leitura().com(true, true),
        vec![orquestrador(
            "badgood-redacao",
            "GARRA_TESTE_1613_REDACAO",
            "tok-redacao-badgood",
            &["send_message"],
            &[DESTINO],
        )],
        &[DESTINO],
    );
    config.gateway.mcp_http.allow_send = true;
    let (router, estado) = router_e_estado(config);
    let guardadas = Arc::new(Mutex::new(Vec::new()));
    estado
        .channels
        .write()
        .await
        .register(Box::new(CanalQueGuarda(Arc::clone(&guardadas))));

    let suspeito = "deploy feito com sk-or-v1-abcdefghijklmnopqrstuvwxyz0123456789 \
                    e Bearer abcdefgh12345678";
    let env = envelope(
        &chamar_como(
            &router,
            "tok-redacao-badgood",
            "garra_send_message",
            json!({ "channel": "telegram", "chat_id": DESTINO, "text": suspeito }),
        )
        .await,
    );
    assert_eq!(env["ok"], true, "{env}");

    let limpo = "reunião às 15h, pauta: revisar o roteiro";
    let env_limpo = envelope(
        &chamar_como(
            &router,
            "tok-redacao-badgood",
            "garra_send_message",
            json!({ "channel": "telegram", "chat_id": DESTINO, "text": limpo }),
        )
        .await,
    );
    assert_eq!(env_limpo["ok"], true, "{env_limpo}");

    let saidas = guardadas.lock().await.clone();
    assert_eq!(saidas.len(), 2, "{saidas:?}");
    assert!(saidas[0].contains("[REDACTED]"), "{}", saidas[0]);
    assert!(!saidas[0].contains("sk-or-v1-"), "{}", saidas[0]);
    assert!(!saidas[0].contains("abcdefgh12345678"), "{}", saidas[0]);
    assert_eq!(saidas[1], limpo);
}

/// Cada chamada da ponte deixa uma linha de auditoria com o orquestrador e o
/// resultado, e o token nunca aparece no log. O destino sai pseudonimizado.
#[tracing_test::traced_test]
#[tokio::test]
async fn cada_chamada_e_auditada_com_o_orquestrador_e_sem_o_token() {
    const TOKEN: &str = "tok-auditoria-segredo-1613";
    let mut config = config_com_orquestradores(
        Ligacao::leitura().com(true, true),
        vec![orquestrador(
            "badgood-auditoria",
            "GARRA_TESTE_1613_AUDITORIA",
            TOKEN,
            &["status", "send_message"],
            &[DESTINO],
        )],
        &[DESTINO],
    );
    config.gateway.mcp_http.allow_send = true;
    let (router, _estado) = router_e_estado(config);

    chamar_como(&router, TOKEN, "garra_status", json!({})).await;
    chamar_como(
        &router,
        TOKEN,
        "garra_send_message",
        json!({ "channel": "telegram", "chat_id": DESTINO, "text": "oi" }),
    )
    .await;

    assert!(
        logs_contain("badgood-auditoria"),
        "sem o orquestrador no log"
    );
    assert!(
        logs_contain("mcp_http: chamada"),
        "sem a linha de auditoria"
    );
    assert!(logs_contain("channel_offline"), "sem o resultado");
    assert!(!logs_contain(TOKEN), "o token vazou para o log");
    logs_assert(|linhas: &[&str]| {
        let vazou = linhas
            .iter()
            .filter(|l| l.contains("garraia_gateway::mcp_http"))
            .any(|l| l.contains(&DESTINO.to_string()));
        if vazou {
            Err("o chat_id entrou no log de auditoria".into())
        } else {
            Ok(())
        }
    });
}

/// A config default (sem orquestradores) nao muda a superficie do dono nem o
/// despacho: o `tools/list` e o `tools/call` seguem os de antes.
#[tokio::test]
async fn sem_orquestradores_o_dono_segue_como_antes() {
    let (router, _estado) = router_e_estado(config_de(Ligacao::leitura()));
    assert_eq!(listar_como(&router, CHAVE).await.len(), 4);
    let env = envelope(&chamar_como(&router, CHAVE, "garra_status", json!({})).await);
    assert_eq!(env["ok"], true, "{env}");
}

// ── garra_agent (#1615): delegacao de tarefas com identidade, teto e redacao ──

/// A config da ponte com `garra_agent` e um `llm.openrouter` no endpoint falso.
/// A chave `k-do-dono` e a unica que o provider pode receber.
fn config_com_agent(endereco_falso: &str, allow_agent: bool, orcamento: u32) -> AppConfig {
    let mut config = config_com_ask(endereco_falso, false, &[], 10);
    config.gateway.mcp_http.allow_agent = allow_agent;
    config.gateway.mcp_http.agent_budget_per_minute = orcamento;
    config
}

/// `garra_agent` como um `tools/call` no router dado.
async fn pedir_agent(router: &axum::Router, args: Value) -> Value {
    corpo_json(
        pedir(
            router.clone(),
            Some(BEARER),
            rpc(
                3,
                "tools/call",
                json!({ "name": "garra_agent", "arguments": args }),
            ),
        )
        .await,
    )
    .await
}

/// O erro de `garra_agent` de dentro do envelope `garra.agent.v1`.
fn erro_agent(resposta: &Value) -> Value {
    assert_eq!(
        resposta["result"]["isError"], true,
        "garra_agent deveria ter falhado: {resposta}"
    );
    let env = envelope(resposta);
    assert_eq!(env["schema"], "garra.agent.v1", "{env}");
    assert_eq!(env["ok"], false, "{env}");
    env["error"].clone()
}

/// Aponta o `llm.openrouter` da config para o endpoint falso dado.
fn com_llm_falso(mut config: AppConfig, endereco_falso: &str) -> AppConfig {
    config.llm.insert(
        "openrouter".to_string(),
        LlmProviderConfig {
            provider: "openrouter".to_string(),
            model: Some(DEFAULT_CLOUD_MODEL.to_string()),
            api_key: Some("k-do-dono".to_string()),
            base_url: Some(format!("{endereco_falso}/api/v1")),
            extra: Default::default(),
        },
    );
    config
}

/// Config com `garra_agent` ligada e o provider apontando para `endereco`.
fn config_agent_para(endereco: &str) -> AppConfig {
    let mut config = config_de(Ligacao::leitura());
    config.gateway.mcp_http.allow_agent = true;
    com_llm_falso(config, endereco)
}

/// A tool so aparece com `allow_agent`. O default da instalacao continua com as
/// quatro de leitura: ligar `garra_agent` nao muda nada para quem nao a ligou.
#[tokio::test]
async fn garra_agent_so_e_anunciada_com_allow_agent() {
    let endereco = MockEndpoint::start().await;
    let desligada = router_com(config_com_agent(&endereco.uri(), false, 2));
    let ligada = router_com(config_com_agent(&endereco.uri(), true, 2));

    let nomes_off = nomes_das_tools(&tools_list_de(&desligada).await);
    assert!(
        !nomes_off.iter().any(|n| n == "garra_agent"),
        "{nomes_off:?}"
    );
    assert_eq!(nomes_off.len(), 4, "{nomes_off:?}");

    let nomes_on = nomes_das_tools(&tools_list_de(&ligada).await);
    assert!(nomes_on.iter().any(|n| n == "garra_agent"), "{nomes_on:?}");
}

/// Sem `allow_agent`, uma chamada direta e recusada com `agent_disabled` antes de
/// qualquer provider ser montado: o endpoint nao recebe nada.
#[tokio::test]
async fn garra_agent_desligado_recusa_mesmo_quando_chamado_direto() {
    let endereco = MockEndpoint::start().await;
    let router = router_com(config_com_agent(&endereco.uri(), false, 2));

    let erro = erro_agent(&pedir_agent(&router, json!({ "message": "faz x" })).await);
    assert_eq!(erro["kind"], "agent_disabled", "{erro}");
    assert!(
        endereco.credentials().await.is_empty(),
        "o provider foi chamado"
    );
}

/// O caminho feliz: sem `model` vai o modelo default do projeto, com a chave do
/// dono, e a resposta sai como o envelope `garra.agent.v1`.
#[tokio::test]
async fn garra_agent_caminho_feliz_usa_o_default_e_a_chave_do_dono() {
    let endereco = MockEndpoint::start().await;
    let router = router_com(config_com_agent(&endereco.uri(), true, 2));

    let resposta = pedir_agent(&router, json!({ "message": "faz x" })).await;

    assert_ne!(resposta["result"]["isError"], true, "{resposta}");
    let env = envelope(&resposta);
    assert_eq!(env["schema"], "garra.agent.v1", "{env}");
    assert_eq!(env["ok"], true, "{env}");
    assert_eq!(env["answer"], SENTINEL, "{env}");
    assert_eq!(env["provider"], "openrouter", "{env}");
    assert_eq!(env["model"], DEFAULT_CLOUD_MODEL, "{env}");
    assert!(
        env["session_id"]
            .as_str()
            .is_some_and(|s| s.starts_with("mcp-http-dono-")),
        "{env}"
    );

    let credenciais = endereco.credentials().await;
    assert!(
        !credenciais.is_empty() && credenciais.iter().all(|c| c == "k-do-dono"),
        "o provider recebeu {credenciais:?}"
    );
}

/// O modelo do agente passa pela lista de `garra_ask`: fora dela, recusado antes
/// do provider.
#[tokio::test]
async fn garra_agent_modelo_fora_da_lista_nao_chega_ao_provider() {
    let endereco = MockEndpoint::start().await;
    let router = router_com(config_com_agent(&endereco.uri(), true, 2));

    let erro = erro_agent(
        &pedir_agent(
            &router,
            json!({ "message": "faz x", "model": "openrouter/auto" }),
        )
        .await,
    );
    assert_eq!(erro["kind"], "model_not_allowed", "{erro}");
    assert!(endereco.credentials().await.is_empty());
}

/// O teto por minuto: a segunda execucao e recusada como `over_budget`, e a
/// recusa nao gasta inferencia.
#[tokio::test]
async fn garra_agent_teto_por_minuto_recusa_a_seguinte_sem_gastar() {
    let endereco = MockEndpoint::start().await;
    let router = router_com(config_com_agent(&endereco.uri(), true, 1));

    let primeira = pedir_agent(&router, json!({ "message": "faz x" })).await;
    assert_eq!(envelope(&primeira)["ok"], true, "{primeira}");

    let segunda = pedir_agent(&router, json!({ "message": "faz de novo" })).await;
    assert_eq!(erro_agent(&segunda)["kind"], "over_budget");
    assert_eq!(
        endereco.credentials().await.len(),
        1,
        "a recusa por teto chegou ao provider"
    );
}

/// #1615 — o orcamento de `garra_agent` e PROPRIO: esgotar o de `garra_ask` nao
/// para o agente, e esgotar o agente nao para o ask.
#[tokio::test]
async fn orcamento_de_agent_e_proprio_e_nao_divide_com_ask() {
    let endereco = MockEndpoint::start().await;
    let mut config = config_com_ask(&endereco.uri(), true, &[], 1);
    config.gateway.mcp_http.allow_agent = true;
    config.gateway.mcp_http.agent_budget_per_minute = 1;
    let router = router_com(config);

    assert_eq!(
        envelope(&pedir_ask(&router, json!({ "message": "oi" })).await)["ok"],
        true
    );
    let agente = pedir_agent(&router, json!({ "message": "faz x" })).await;
    assert_eq!(
        envelope(&agente)["ok"],
        true,
        "o ask nao pode ter gasto a cota do agente: {agente}"
    );

    assert_eq!(
        erro_ask(&pedir_ask(&router, json!({ "message": "de novo" })).await)["kind"],
        "over_budget"
    );
    assert_eq!(
        erro_agent(&pedir_agent(&router, json!({ "message": "de novo" })).await)["kind"],
        "over_budget"
    );
}

/// `working_dir` e `additionalProperties` continuam de fora na ponte: o pedido
/// nao escolhe diretorio do disco do operador. E erro de protocolo.
#[tokio::test]
async fn garra_agent_working_dir_vira_erro_de_protocolo() {
    let endereco = MockEndpoint::start().await;
    let router = router_com(config_com_agent(&endereco.uri(), true, 2));

    let resposta = pedir_agent(&router, json!({ "message": "faz x", "working_dir": "/" })).await;
    assert!(resposta["error"].is_object(), "{resposta}");
    assert!(endereco.credentials().await.is_empty());
}

/// `timeout_secs` acima do teto do operador e erro de protocolo, nao corte em
/// silencio: o provider nao e chamado.
#[tokio::test]
async fn garra_agent_timeout_acima_do_teto_do_operador_vira_erro_de_protocolo() {
    let endereco = MockEndpoint::start().await;
    let mut config = config_com_agent(&endereco.uri(), true, 2);
    config.gateway.mcp_http.agent_max_seconds = 60;
    let router = router_com(config);

    let resposta = pedir_agent(&router, json!({ "message": "faz x", "timeout_secs": 61 })).await;
    assert!(resposta["error"].is_object(), "{resposta}");
    assert!(endereco.credentials().await.is_empty());
}

/// Um provider que nao responde estoura o teto de tempo do operador, e a falha
/// volta como o envelope `timeout`, sem texto solto.
#[tokio::test]
async fn garra_agent_provider_lento_estoura_o_teto_e_vira_timeout() {
    let lento = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(60))
                .set_body_string("{}"),
        )
        .mount(&lento)
        .await;
    let router = router_com(config_agent_para(&lento.uri()));

    let erro =
        erro_agent(&pedir_agent(&router, json!({ "message": "faz x", "timeout_secs": 5 })).await);
    assert_eq!(erro["kind"], "timeout", "{erro}");
}

/// Um provider que responde com uma chave `sk-` no texto: ela nao sai na resposta
/// ao orquestrador. A mensagem que o orquestrador manda tambem nao chega ao modelo
/// com o segredo que ele colou.
struct RespondeComSegredo;

impl Respond for RespondeComSegredo {
    fn respond(&self, request: &wiremock::Request) -> ResponseTemplate {
        let corpo: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
        let texto = concat!("a chave e ", "sk-", "abcdefgh12345678");
        if corpo.get("stream").and_then(Value::as_bool) == Some(true) {
            let chunk = json!({
                "id": "c", "object": "chat.completion.chunk", "model": "m",
                "choices": [{"index": 0, "delta": {"role": "assistant", "content": texto}, "finish_reason": null}]
            });
            let fim = json!({
                "id": "c", "object": "chat.completion.chunk", "model": "m",
                "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
            });
            return ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(format!("data: {chunk}\n\ndata: {fim}\n\ndata: [DONE]\n\n"));
        }
        ResponseTemplate::new(200).set_body_json(json!({
            "id": "c", "object": "chat.completion", "model": "m",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": texto}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
        }))
    }
}

/// Entrada e saida passam pela redacao: o segredo que o orquestrador colou nao
/// chega ao provider, e o que o provider devolve nao volta ao orquestrador.
#[tokio::test]
async fn garra_agent_entrada_e_saida_saem_redigidas() {
    let falso = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(RespondeComSegredo)
        .mount(&falso)
        .await;
    let router = router_com(config_agent_para(&falso.uri()));

    let entrada = concat!("use esta chave ", "sk-", "zyxwvuts98765432 para testar");
    let resposta = pedir_agent(&router, json!({ "message": entrada })).await;
    let env = envelope(&resposta);
    assert_eq!(env["ok"], true, "{env}");
    let resposta_texto = env["answer"].as_str().unwrap_or_default();
    assert!(
        !resposta_texto.contains("abcdefgh12345678"),
        "{resposta_texto}"
    );
    assert!(resposta_texto.contains("[REDACTED]"), "{resposta_texto}");

    let recebidas = falso.received_requests().await.unwrap_or_default();
    assert!(!recebidas.is_empty(), "o provider nao recebeu nada");
    for req in &recebidas {
        let corpo = String::from_utf8_lossy(&req.body);
        assert!(
            !corpo.contains("zyxwvuts98765432"),
            "o segredo da entrada vazou ao modelo"
        );
    }
}
