use std::net::TcpListener;

use futures::StreamExt;
use garraia_config::AppConfig;
use garraia_gateway::GatewayServer;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::handshake::client::generate_key;

/// Pick a random available port.
fn random_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind to random port");
    listener.local_addr().unwrap().port()
}

/// Start the gateway in the background and return the origin
/// (`ws://127.0.0.1:{port}`), without a path.
async fn start_test_gateway_base(config: AppConfig) -> String {
    let port = config.gateway.port;
    tokio::spawn(async move {
        let server = GatewayServer::new(config);
        let _ = server.run().await;
    });

    // Wait for the server to be ready
    for _ in 0..50 {
        if TcpListener::bind(format!("127.0.0.1:{port}")).is_err() {
            break; // port is in use = server is up
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    format!("ws://127.0.0.1:{port}")
}

/// Start the gateway in the background and return the WebSocket URL base.
async fn start_test_gateway(config: AppConfig) -> String {
    format!("{}/ws", start_test_gateway_base(config).await)
}

#[tokio::test]
async fn ws_rejects_missing_api_key_if_configured() {
    let port = random_port();
    let mut config = AppConfig::default();
    config.gateway.port = port;
    config.gateway.api_key = Some("secret-token".to_string());
    config.memory.enabled = false;

    let ws_url = start_test_gateway(config).await;

    // Connect without token
    let result = connect_async(&ws_url).await;
    assert!(result.is_err(), "Should fail without token");

    if let Err(tokio_tungstenite::tungstenite::Error::Http(resp)) = result {
        assert_eq!(resp.status(), 401);
    }
}

#[tokio::test]
async fn ws_rejects_wrong_api_key() {
    let port = random_port();
    let mut config = AppConfig::default();
    config.gateway.port = port;
    config.gateway.api_key = Some("secret-token".to_string());
    config.memory.enabled = false;

    let base_url = start_test_gateway(config).await;
    let ws_url = format!("{}?token=wrong-token", base_url);

    let result = connect_async(&ws_url).await;
    assert!(result.is_err(), "Should fail with wrong token");

    if let Err(tokio_tungstenite::tungstenite::Error::Http(resp)) = result {
        assert_eq!(resp.status(), 401);
    }
}

#[tokio::test]
#[ignore = "TODO(fix/ci-triage-2026-04-15): server.run() exits silently on startup in CI (missing Postgres since plan 0016 M4). Same root cause as e2e/playwright jobs. Deferred to the gateway-test-fixture follow-up PR."]
async fn ws_accepts_correct_api_key_query_param() {
    let port = random_port();
    let mut config = AppConfig::default();
    config.gateway.port = port;
    config.gateway.api_key = Some("secret-token".to_string());
    config.memory.enabled = false;

    let base_url = start_test_gateway(config).await;
    let ws_url = format!("{}?token=secret-token", base_url);

    let (ws, _) = connect_async(&ws_url)
        .await
        .expect("Should connect with correct token");
    let (_ws, _) = ws.split();
}

#[tokio::test]
#[ignore = "TODO(fix/ci-triage-2026-04-15): server.run() exits silently on startup in CI (missing Postgres since plan 0016 M4). Same root cause as e2e/playwright jobs. Deferred to the gateway-test-fixture follow-up PR."]
async fn ws_accepts_correct_api_key_header() {
    let port = random_port();
    let mut config = AppConfig::default();
    config.gateway.port = port;
    config.gateway.api_key = Some("secret-token".to_string());
    config.memory.enabled = false;

    let ws_url = start_test_gateway(config).await;

    let request = tokio_tungstenite::tungstenite::handshake::client::Request::builder()
        .uri(&ws_url)
        .header("Authorization", "Bearer secret-token")
        .header("Sec-WebSocket-Key", generate_key())
        .header("Sec-WebSocket-Version", "13")
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Host", format!("127.0.0.1:{}", port))
        .body(())
        .unwrap();

    let (ws, _) = connect_async(request)
        .await
        .expect("Should connect with correct header token");
    let (_ws, _) = ws.split();
}

#[tokio::test]
#[ignore = "TODO(fix/ci-triage-2026-04-15): server.run() exits silently on startup in CI (missing Postgres since plan 0016 M4). Same root cause as e2e/playwright jobs. Deferred to the gateway-test-fixture follow-up PR."]
async fn ws_allows_access_if_no_api_key_configured() {
    let port = random_port();
    let mut config = AppConfig::default();
    config.gateway.port = port;
    config.gateway.api_key = None;
    config.memory.enabled = false;

    let ws_url = start_test_gateway(config).await;

    let (ws, _) = connect_async(&ws_url)
        .await
        .expect("Should connect without token if none configured");
    let (_ws, _) = ws.split();
}

// ── #1240 / auditoria R4 do PR #1251: o `/ws/parrot` ──────────────────────
//
// O buraco: `handle_parrot_socket` roda um turno completo do agente — com as
// tools e a chave de LLM do dono — sobre a sessao persistente do desktop, e
// ate esta correcao a unica guarda da rota era o `origin_guard`, que passa
// **de proposito** quando nao ha header `Origin` (cliente nao-navegador nao
// manda um). Com `gateway.api_key` configurada e o gateway em `0.0.0.0` — o
// caso do app na LAN, que e o motivo de a chave existir — um
// `websocat ws://host:3888/ws/parrot` conectava sem credencial nenhuma e
// dirigia o agente. O irmao `/ws`, montado na linha de cima do `router.rs`,
// ja checava a chave.
//
// **Por que estes testes nao usam o `oneshot` do `origin_guard_layering.rs`**:
// o `WebSocketUpgrade` e um extractor, e rejeita com `426` na **extracao**
// quando a extensao `OnUpgrade` nao existe — sempre o caso num `oneshot`, que
// nao tem conexao hyper por tras. O gate mora no corpo do handler, depois da
// extracao, entao so um handshake de verdade o alcanca.
//
// **E por que nao usam o `start_test_gateway` acima**: o `GatewayServer::run`
// sai em silencio neste ambiente e no CI (e por isso que todo `ws_accepts_*`
// esta `#[ignore]`). Pior: com o servidor fora do ar, o par
// `assert!(result.is_err())` + `if let Err(Http(resp))` dos `ws_rejects_*`
// passa sem medir nada — o erro de conexao satisfaz o `assert!` e o `if let`
// nao entra. O harness daqui monta o `build_router` de verdade e o serve com
// `axum::serve`, sem depender do bootstrap, e as assercoes abaixo falham
// quando o servidor nao sobe em vez de passar caladas.

/// Sobe o `build_router` de verdade num socket efemero e devolve a origem
/// (`ws://127.0.0.1:{porta}`).
async fn sobe_router_de_teste(chave: Option<&str>) -> String {
    let mut config = AppConfig::default();
    config.gateway.api_key = chave.map(str::to_string);
    config.memory.enabled = false;

    let state = std::sync::Arc::new(garraia_gateway::state::AppState::new(
        config,
        std::sync::Arc::new(garraia_agents::AgentRuntime::new()),
        garraia_channels::ChannelRegistry::new(),
    ));
    let admin_store = std::sync::Arc::new(tokio::sync::Mutex::new(
        garraia_gateway::admin::store::AdminStore::in_memory().expect("admin store em memoria"),
    ));
    let router = garraia_gateway::router::build_router(
        state,
        garraia_gateway::push_channels::PushChannelStates::empty(),
        admin_store,
        std::sync::Arc::new(vec![0u8; 32]),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind efemero");
    let addr = listener.local_addr().expect("endereco local");
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await;
    });

    format!("ws://{addr}")
}

/// Exige que o handshake tenha sido **recusado com 401**.
///
/// Um erro de transporte e falha do teste, nao sucesso: significa que o
/// servidor nao subiu e que nada foi medido.
async fn exige_recusa_401(url: &str) {
    match connect_async(url).await {
        Err(tokio_tungstenite::tungstenite::Error::Http(resp)) => {
            assert_eq!(resp.status(), 401, "{url}: recusado, mas nao com 401");
        }
        Err(outro) => panic!("{url}: o handshake nem chegou ao gate ({outro}); o servidor subiu?"),
        Ok(_) => panic!("{url}: handshake ACEITO sem credencial valida — a #1240 esta aberta"),
    }
}

/// Exige que o handshake tenha sido **aceito**.
async fn exige_conexao(url: &str) {
    match connect_async(url).await {
        Ok((ws, _)) => {
            let (_ws, _) = ws.split();
        }
        Err(e) => panic!("{url}: handshake recusado ({e})"),
    }
}

/// O coracao da correcao: com a chave configurada, o socket do papagaio deixa
/// de responder a quem nao a tem — sem credencial, e com credencial errada
/// nos dois nomes de query que o `/ws` ja aceitava.
///
/// O `/ws` entra junto porque a assimetria entre os dois irmaos e justamente
/// o que a auditoria achou: eles sao montados em linhas vizinhas do
/// `router.rs` e precisam decidir igual.
#[tokio::test]
async fn com_chave_o_ws_parrot_exige_credencial() {
    let base = sobe_router_de_teste(Some("secret-token")).await;

    for rota in ["/ws", "/ws/parrot"] {
        exige_recusa_401(&format!("{base}{rota}")).await;
        exige_recusa_401(&format!("{base}{rota}?token=wrong-token")).await;
        exige_recusa_401(&format!("{base}{rota}?api_key=wrong-token")).await;
    }
}

/// A credencial certa abre. A query e aceita **aqui** e so aqui: a webview
/// Tauri abre o socket com `new WebSocket(...)`, que nao manda header — e e
/// por isso que a rota nao pode entrar no `is_gated_path`, que so le header.
/// O bearer tambem serve, para o cliente de CLI que consegue mandar header.
#[tokio::test]
async fn a_credencial_certa_abre_o_ws_parrot() {
    let base = sobe_router_de_teste(Some("secret-token")).await;

    for rota in ["/ws", "/ws/parrot"] {
        for query in ["token", "api_key"] {
            exige_conexao(&format!("{base}{rota}?{query}=secret-token")).await;
        }

        let pedido = tokio_tungstenite::tungstenite::handshake::client::Request::builder()
            .uri(format!("{base}{rota}"))
            .header("Authorization", "Bearer secret-token")
            .header("Sec-WebSocket-Key", generate_key())
            .header("Sec-WebSocket-Version", "13")
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Host", base.trim_start_matches("ws://").to_string())
            .body(())
            .unwrap();
        let (ws, _) = connect_async(pedido)
            .await
            .unwrap_or_else(|e| panic!("{rota}: recusou o bearer certo ({e})"));
        let (_ws, _) = ws.split();
    }
}

/// O contrato exato que o Garra Desktop passou a cumprir.
///
/// O gate acima so vale se o cliente de fato mandar a credencial, e ate a
/// revisao deste PR ele **nao mandava**: `ui/ws.js` conectava numa URL
/// constante, sem token, e nao havia plumbing de chave em lugar nenhum da
/// casca Tauri — o overlay levava 401 e caia em reconexao infinita, mudo.
/// Agora o `ws.js` monta `?token=${encodeURIComponent(chave)}`, com a chave
/// vinda do comando Tauri `gateway_api_key`.
///
/// `encodeURIComponent` e o par certo do decode de query do axum: ele escapa
/// `+` como `%2B`, entao uma chave com `+` nao chega como espaco (a pegadinha
/// classica do form-urlencoded). Este teste percorre uma chave com `/`, `+`,
/// `=` e espaco pelo caminho inteiro para que a escolha do encoder fique
/// travada nos dois lados.
#[tokio::test]
async fn o_token_do_desktop_chega_inteiro_mesmo_precisando_de_escape() {
    // Os mesmos caracteres que o simulador do ws.js exercita.
    let chave = "sk-cha ve/com+especiais=";
    let base = sobe_router_de_teste(Some(chave)).await;

    // `percent-encoding` nao e dep de teste aqui; este escape cobre
    // exatamente o conjunto acima, no mesmo formato do encodeURIComponent.
    let escapada = chave
        .replace('%', "%25")
        .replace(' ', "%20")
        .replace('/', "%2F")
        .replace('+', "%2B")
        .replace('=', "%3D");

    for rota in ["/ws", "/ws/parrot"] {
        exige_conexao(&format!("{base}{rota}?token={escapada}")).await;
    }
}

/// A pegadinha do `+`, isolada.
///
/// Um `+` cru e caractere valido de URI, entao nada falha no transporte — ele
/// so **decodifica como espaco**, e a chave chega diferente da configurada.
/// E o unico caractere em que "mandar sem escapar" produz um 401 silencioso
/// em vez de um erro visivel, e e exatamente por isso que o `ws.js` usa
/// `encodeURIComponent` (que o escapa como `%2B`) e nao uma interpolacao nua.
#[tokio::test]
async fn um_mais_nao_escapado_vira_espaco_e_a_chave_nao_bate() {
    let base = sobe_router_de_teste(Some("chave+com+mais")).await;

    for rota in ["/ws", "/ws/parrot"] {
        exige_conexao(&format!("{base}{rota}?token=chave%2Bcom%2Bmais")).await;
        exige_recusa_401(&format!("{base}{rota}?token=chave+com+mais")).await;
    }
}

/// A garantia de "sem chave nada muda": a instalacao default segue aberta, e
/// o Garra Desktop conecta sem credencial como sempre.
#[tokio::test]
async fn sem_chave_configurada_o_ws_parrot_nao_muda() {
    let base = sobe_router_de_teste(None).await;

    for rota in ["/ws", "/ws/parrot"] {
        exige_conexao(&format!("{base}{rota}")).await;
    }
}
