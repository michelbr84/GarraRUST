//! #1462, opcao 3: leitura de sessao por id — cliente vs. operador.
//!
//! A PR #1468 fechou a **escrita** por id (`X-Session-Id` na rota compat e
//! `POST /api/sessions/{id}/messages`). A **leitura** ficou aberta:
//! `GET /api/sessions/{id}/history` devolvia o historico verbatim de qualquer
//! sessao em memoria — e a de um canal esta em memoria no caso normal, porque
//! a hidratacao do canal a poe la. Rota `/api/*`, auth-free por padrao, sem
//! LLM no meio, enumeravel por script com ids adivinhaveis por construcao
//! (`whatsapp-linked-<numero>`, `telegram-<chat>`).
//!
//! O dono decidiu a opcao 3 — separar as duas leituras:
//!
//! * **cliente** (`GET /api/sessions`, `GET /api/sessions/{id}/history`,
//!   `DELETE /api/sessions/{id}`, `resume` sem token no `/ws`): por id, so
//!   sessao das superficies locais do operador (`api`, `vscode`, `web`,
//!   `parrot`). Sessao de canal ou do mobile responde o mesmo `404` de id
//!   inexistente, byte a byte, e **nao e tocada**: nem hidratada de novo
//!   com a superficie do chamador, nem desconectada, nem listada;
//! * **operador** (`GET /admin/api/sessions/{id}/history`, ao lado do
//!   `GET /admin/api/sessions` que ja existia): qualquer sessao, com o
//!   cookie de sessao do `/admin` e a permissao `manage_sessions` — em
//!   memoria ou so no `sessions.db`, e sem hidratar. E por aqui que o Export
//!   do Web Console passa a funcionar para sessao de canal;
//! * **mobile** (`GET /chat/history`): a sessao vem do `sub` do JWT, nunca
//!   de um id do cliente — cada usuario le so a propria.
//!
//! Os testes sobem o `build_router` de verdade (as tres superficies no mesmo
//! processo, com os mesmos layers de producao) sobre um `AppState` com
//! `sessions.db` real, e cobrem os dois lugares onde a sessao pode estar: em
//! memoria e so no disco.

use std::sync::{Arc, LazyLock};

use async_trait::async_trait;
use futures::{SinkExt, Stream, StreamExt};
use garraia_agents::{
    AgentRuntime, ContentBlock, LlmProvider, LlmRequest, LlmResponse, StreamEvent,
};
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_db::{ChatSessionManager, SessionStore};
use garraia_gateway::admin::rbac::Role;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::push_channels::PushChannelStates;
use garraia_gateway::router::build_router;
use garraia_gateway::state::{AppState, CANAL_DA_API};
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;

/// Mesmo padrao de `x_session_id_nao_alcanca_sessao_de_canal.rs`:
/// `AppState::new` le e grava no diretorio de config, entao o binario inteiro
/// aponta para um tempdir.
static TEST_ENV: LazyLock<tempfile::TempDir> = LazyLock::new(|| {
    let dir = tempfile::tempdir().expect("temp config dir");
    // SAFETY: o `LazyLock` serializa isto contra as outras threads de teste
    // deste binario; todo teste chama `subir()` primeiro.
    unsafe {
        std::env::set_var("GARRAIA_CONFIG_DIR", dir.path());
        std::env::set_var(
            garraia_gateway::mcp::McpPersistenceService::DISABLE_AUTOPROVISION_ENV,
            "1",
        );
    }
    dir
});

/// Provider de roteiro: responde texto fixo a qualquer pedido.
struct Eco;

#[async_trait]
impl LlmProvider for Eco {
    fn provider_id(&self) -> &str {
        "eco"
    }

    async fn complete(&self, _request: &LlmRequest) -> garraia_common::Result<LlmResponse> {
        Ok(LlmResponse {
            content: vec![ContentBlock::Text {
                text: "eco".to_string(),
            }],
            model: "m".to_string(),
            stop_reason: None,
            usage: None,
        })
    }

    async fn stream_complete(
        &self,
        _request: &LlmRequest,
    ) -> garraia_common::Result<
        std::pin::Pin<Box<dyn Stream<Item = garraia_common::Result<StreamEvent>> + Send>>,
    > {
        Ok(Box::pin(futures::stream::iter(vec![
            Ok(StreamEvent::TextDelta("eco".to_string())),
            Ok(StreamEvent::MessageStop),
        ])))
    }

    async fn health_check(&self) -> garraia_common::Result<bool> {
        Ok(true)
    }
}

struct Gateway {
    base: String,
    state: Arc<AppState>,
    admin_store: Arc<Mutex<AdminStore>>,
    /// Mantem o diretorio do `sessions.db` vivo enquanto o teste roda.
    _dados: tempfile::TempDir,
}

/// Sobe o `build_router` inteiro — `/api/sessions*`, `/admin/api/*`,
/// `/chat/history` e `/ws` — sobre um `AppState` com `sessions.db` real e
/// JWT configurado para o mobile.
async fn subir() -> Gateway {
    LazyLock::force(&TEST_ENV);
    let rt = AgentRuntime::new();
    rt.register_provider(Arc::new(Eco));

    let mut config = AppConfig::default();
    config.memory.enabled = false;
    config.mcp.clear();
    let mut state = AppState::new(config, Arc::new(rt), ChannelRegistry::new());
    state.allowlist = Arc::new(std::sync::Mutex::new(
        garraia_security::Allowlist::restricted(Vec::<String>::new()),
    ));
    let dados = tempfile::tempdir().expect("tempdir dos dados");
    let store = SessionStore::open(&dados.path().join("sessions.db")).expect("abrir store");
    let store = Arc::new(Mutex::new(store));
    state.set_session_store(Arc::clone(&store));
    state.set_chat_session_manager(Arc::new(ChatSessionManager::new(store)));
    state.set_auth_config(Arc::new(garraia_config::AuthConfig {
        jwt_secret: SecretString::from("j".repeat(32)),
        refresh_hmac_secret: SecretString::from("r".repeat(32)),
        login_database_url: SecretString::from("postgres://x@localhost/x".to_string()),
        signup_database_url: SecretString::from("postgres://y@localhost/y".to_string()),
        app_database_url: None,
    }));
    let state = Arc::new(state);

    let admin_store = Arc::new(Mutex::new(
        AdminStore::in_memory().expect("admin store em memoria"),
    ));
    let router = build_router(
        Arc::clone(&state),
        PushChannelStates::empty(),
        Arc::clone(&admin_store),
        Arc::new(vec![0u8; 32]),
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await;
    });
    Gateway {
        base: format!("127.0.0.1:{}", addr.port()),
        state,
        admin_store,
        _dados: dados,
    }
}

/// Um turno completo de outra superficie, gravado em memoria e no disco.
async fn turno(gw: &Gateway, sid: &str, canal: &str, remetente: &str, pergunta: &str) {
    let antes = gw.state.session_history(sid).len();
    gw.state
        .hydrate_session_history(sid, Some(canal), Some(remetente))
        .await;
    gw.state
        .persist_turn(
            sid,
            Some(canal),
            Some(remetente),
            pergunta,
            "resposta confidencial",
        )
        .await;
    assert_eq!(
        gw.state.session_history(sid).len(),
        antes + 2,
        "turno gravado"
    );
}

async fn conversa_de(gw: &Gateway, sid: &str, canal: &str, remetente: &str) {
    turno(gw, sid, canal, remetente, "pergunta confidencial da vitima").await;
}

fn canais_dos_turnos(gw: &Gateway, sid: &str) -> Vec<String> {
    gw.state
        .sessions
        .get(sid)
        .map(|s| s.canais_dos_turnos.iter().cloned().collect())
        .unwrap_or_default()
}

fn conectada(gw: &Gateway, sid: &str) -> Option<bool> {
    gw.state.sessions.get(sid).map(|s| s.connected)
}

async fn superficies_no_disco(gw: &Gateway, sid: &str) -> garraia_db::SessionSurfaces {
    gw.state
        .session_store
        .as_ref()
        .expect("store")
        .lock()
        .await
        .get_session_surfaces(sid)
        .expect("ler o sessions.db")
        .expect("a linha existe")
}

fn json(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or(Value::Null)
}

fn sem_conteudo_da_vitima(bytes: &[u8]) -> bool {
    !String::from_utf8_lossy(bytes).contains("confidencial")
}

// ── Clientes das rotas ──────────────────────────────────────────────────

/// `GET /api/sessions/{id}/history` — a leitura de cliente. Devolve os
/// bytes crus do corpo para a comparacao byte a byte dos 404.
async fn history_cliente(gw: &Gateway, sid: &str) -> (u16, Vec<u8>) {
    let resp = reqwest::Client::new()
        .get(format!("http://{}/api/sessions/{sid}/history", gw.base))
        .send()
        .await
        .expect("get");
    let status = resp.status().as_u16();
    (status, resp.bytes().await.expect("corpo").to_vec())
}

async fn listar_cliente(gw: &Gateway) -> Vec<String> {
    let resp = reqwest::Client::new()
        .get(format!("http://{}/api/sessions", gw.base))
        .send()
        .await
        .expect("get");
    assert_eq!(resp.status().as_u16(), 200);
    let corpo: Value = resp.json().await.expect("json");
    corpo["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .filter_map(|s| s["session_id"].as_str().map(str::to_string))
        .collect()
}

async fn delete_cliente(gw: &Gateway, sid: &str) -> (u16, Value) {
    let resp = reqwest::Client::new()
        .delete(format!("http://{}/api/sessions/{sid}", gw.base))
        .send()
        .await
        .expect("delete");
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(Value::Null))
}

/// Cookie + CSRF de uma sessao do `/admin`.
struct SessaoAdmin {
    cookie: String,
}

/// Cria o primeiro admin via `POST /admin/api/setup`, que ja devolve o
/// cookie de sessao.
async fn criar_admin(gw: &Gateway) -> SessaoAdmin {
    let resp = reqwest::Client::new()
        .post(format!("http://{}/admin/api/setup", gw.base))
        .json(&json!({ "username": "root", "password": "senha-inicial-do-operador" }))
        .send()
        .await
        .expect("setup");
    assert_eq!(resp.status().as_u16(), 201, "setup do admin");
    let set_cookie = resp
        .headers()
        .get("set-cookie")
        .and_then(|v| v.to_str().ok())
        .expect("set-cookie")
        .to_string();
    let token = set_cookie
        .split(';')
        .next()
        .and_then(|parte| parte.trim().strip_prefix("garraia_admin_session="))
        .expect("token da sessao")
        .to_string();
    SessaoAdmin {
        cookie: format!("garraia_admin_session={token}"),
    }
}

/// Uma sessao de `viewer`, aberta direto no store: o papel le paineis, mas
/// nao tem `manage_sessions`.
async fn sessao_de_viewer(gw: &Gateway) -> SessaoAdmin {
    let guard = gw.admin_store.lock().await;
    let user = guard
        .create_user("leitor", "senha-inicial-do-leitor", Role::Viewer)
        .expect("viewer");
    let sessao = guard
        .create_session(&user.id, None, None)
        .expect("sessao do viewer");
    SessaoAdmin {
        cookie: format!("garraia_admin_session={}", sessao.token),
    }
}

/// `GET /admin/api/sessions/{id}/history` — a leitura do operador.
async fn history_admin(gw: &Gateway, sid: &str, sessao: Option<&SessaoAdmin>) -> (u16, Value) {
    let mut req = reqwest::Client::new().get(format!(
        "http://{}/admin/api/sessions/{sid}/history",
        gw.base
    ));
    if let Some(s) = sessao {
        req = req.header("cookie", &s.cookie);
    }
    let resp = req.send().await.expect("get");
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(Value::Null))
}

async fn listar_admin(gw: &Gateway, sessao: &SessaoAdmin) -> Vec<String> {
    let resp = reqwest::Client::new()
        .get(format!("http://{}/admin/api/sessions", gw.base))
        .header("cookie", &sessao.cookie)
        .send()
        .await
        .expect("get");
    assert_eq!(resp.status().as_u16(), 200);
    let corpo: Value = resp.json().await.expect("json");
    corpo["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .filter_map(|s| s["id"].as_str().map(str::to_string))
        .collect()
}

fn jwt(gw: &Gateway, sub: &str) -> String {
    garraia_gateway::mobile_auth::issue_jwt_pub(&gw.state, sub, &format!("{sub}@example.test"))
        .expect("jwt")
}

/// `GET /chat/history` — a leitura do mobile, autenticada por JWT.
async fn history_mobile(gw: &Gateway, token: Option<&str>) -> (u16, Value) {
    let mut req = reqwest::Client::new().get(format!("http://{}/chat/history", gw.base));
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req.send().await.expect("get");
    let status = resp.status().as_u16();
    (status, resp.json().await.unwrap_or(Value::Null))
}

/// `resume` sem token no `/ws` — o caminho permissivo enquanto a sessao
/// esta em memoria. Devolve o primeiro frame de handshake (`resumed` ou
/// `connected`).
async fn ws_resume_sem_token(gw: &Gateway, sid: &str) -> Value {
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{}/ws", gw.base))
        .await
        .expect("ws connect");
    ws.send(Message::Text(
        json!({"type": "resume", "session_id": sid})
            .to_string()
            .into(),
    ))
    .await
    .expect("resume");
    loop {
        let frame = tokio::time::timeout(std::time::Duration::from_secs(20), ws.next())
            .await
            .expect("frame a tempo")
            .expect("socket aberto")
            .expect("frame valido");
        if let Message::Text(t) = frame
            && let Ok(v) = serde_json::from_str::<Value>(&t)
            && v["type"]
                .as_str()
                .is_some_and(|x| x == "resumed" || x == "connected")
        {
            return v;
        }
    }
}

// ── GET /api/sessions/{id}/history — leitura de cliente ─────────────────

/// A sessao de um canal, viva em memoria: o `GET` por id responde o mesmo
/// `404` de um id que nunca existiu — status e corpo identicos — e a sessao
/// fica como estava, sem a superficie REST anotada nela.
#[tokio::test]
async fn get_history_de_sessao_de_canal_em_memoria_e_404_identico_ao_de_id_inexistente() {
    let gw = subir().await;
    let sid = "whatsapp-linked-5511999990000";
    conversa_de(&gw, sid, "whatsapp_linked", "5511999990000").await;

    let (status, corpo) = history_cliente(&gw, sid).await;
    let (status_inexistente, corpo_inexistente) = history_cliente(&gw, "nunca-existiu").await;

    assert_eq!(
        status,
        404,
        "sessao de outra superficie e inexistente: {}",
        String::from_utf8_lossy(&corpo)
    );
    assert_eq!(status, status_inexistente);
    assert_eq!(
        corpo, corpo_inexistente,
        "o 404 nao pode confirmar que a sessao existe"
    );
    assert!(sem_conteudo_da_vitima(&corpo));
    assert_eq!(
        gw.state.session_history(sid).len(),
        2,
        "a conversa nao mudou"
    );
    assert_eq!(
        canais_dos_turnos(&gw, sid),
        vec!["whatsapp_linked".to_string()],
        "a leitura recusada nao anotou a superficie REST na sessao"
    );
}

/// Depois de um restart a sessao so existe no `sessions.db`: o `GET` por id
/// nao a traz para a memoria nem reescreve a linha dela.
#[tokio::test]
async fn get_history_de_sessao_de_canal_so_no_disco_e_404_e_nao_a_traz_para_a_memoria() {
    let gw = subir().await;
    let sid = "telegram-424242";
    conversa_de(&gw, sid, "telegram", "424242").await;
    gw.state.sessions.remove(sid);
    assert!(!gw.state.sessions.contains_key(sid), "so no disco");

    let (status, corpo) = history_cliente(&gw, sid).await;

    assert_eq!(status, 404, "{}", String::from_utf8_lossy(&corpo));
    assert!(sem_conteudo_da_vitima(&corpo));
    assert!(
        !gw.state.sessions.contains_key(sid),
        "a recusa nao hidrata a sessao da vitima"
    );
    assert_eq!(
        superficies_no_disco(&gw, sid).await.channel_id,
        "telegram",
        "a linha no banco continua do canal"
    );
}

/// O mobile deriva a sessao do `sub` do JWT: por id, ninguem a le.
#[tokio::test]
async fn get_history_de_sessao_do_mobile_por_id_e_404() {
    let gw = subir().await;
    let sid = "mobile-user-a";
    conversa_de(&gw, sid, "mobile", "user-a").await;

    let (status, corpo) = history_cliente(&gw, sid).await;

    assert_eq!(status, 404, "{}", String::from_utf8_lossy(&corpo));
    assert!(sem_conteudo_da_vitima(&corpo));
    assert_eq!(gw.state.session_history(sid).len(), 2);
    assert_eq!(canais_dos_turnos(&gw, sid), vec!["mobile".to_string()]);
}

/// O caminho legitimo nao muda: a sessao da propria superficie REST e lida
/// em memoria e, depois de sair dela, readotada do disco.
#[tokio::test]
async fn get_history_de_sessao_da_propria_api_e_200_em_memoria_e_readotada_do_disco() {
    let gw = subir().await;
    let sid = "sessao-da-api";
    conversa_de(&gw, sid, CANAL_DA_API, "anonymous").await;

    let (status, corpo) = history_cliente(&gw, sid).await;
    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&corpo));
    let mensagens = json(&corpo)["messages"]
        .as_array()
        .cloned()
        .expect("messages");
    assert_eq!(mensagens.len(), 2);
    assert_eq!(mensagens[0]["role"], "user");
    assert_eq!(mensagens[0]["content"], "pergunta confidencial da vitima");

    gw.state.sessions.remove(sid);
    let (status, corpo) = history_cliente(&gw, sid).await;
    assert_eq!(
        status,
        200,
        "readotada do disco: {}",
        String::from_utf8_lossy(&corpo)
    );
    assert_eq!(
        json(&corpo)["messages"].as_array().map(Vec::len),
        Some(2),
        "o historico inteiro voltou do sessions.db"
    );
}

/// A sessao do chat web e do operador (superficie local): segue legivel.
#[tokio::test]
async fn get_history_de_sessao_do_chat_web_e_200() {
    let gw = subir().await;
    let sid = "sessao-do-web";
    conversa_de(&gw, sid, "web", "operador").await;

    let (status, corpo) = history_cliente(&gw, sid).await;

    assert_eq!(status, 200, "{}", String::from_utf8_lossy(&corpo));
    assert_eq!(json(&corpo)["messages"].as_array().map(Vec::len), Some(2));
}

// ── GET /admin/api/sessions/{id}/history — leitura do operador ──────────

/// Com cookie de admin, a sessao de canal e lida em memoria e, depois de
/// sair dela, do disco — sem entrar na memoria e sem a linha ser reescrita.
/// Cada leitura vai para a auditoria.
#[tokio::test]
async fn admin_history_le_sessao_de_canal_em_memoria_e_no_disco_sem_hidratar() {
    let gw = subir().await;
    let admin = criar_admin(&gw).await;
    let sid = "discord-777";
    conversa_de(&gw, sid, "discord", "777").await;

    let (status, corpo) = history_admin(&gw, sid, Some(&admin)).await;
    assert_eq!(status, 200, "{corpo}");
    assert_eq!(corpo["session_id"], sid);
    let mensagens = corpo["messages"].as_array().expect("messages");
    assert_eq!(mensagens.len(), 2);
    assert_eq!(mensagens[0]["role"], "user");
    assert_eq!(mensagens[0]["content"], "pergunta confidencial da vitima");
    assert_eq!(mensagens[1]["role"], "assistant");
    assert_eq!(
        canais_dos_turnos(&gw, sid),
        vec!["discord".to_string()],
        "a leitura do operador nao reetiqueta a sessao"
    );

    gw.state.sessions.remove(sid);
    let (status, corpo) = history_admin(&gw, sid, Some(&admin)).await;
    assert_eq!(status, 200, "do disco: {corpo}");
    assert_eq!(corpo["messages"].as_array().map(Vec::len), Some(2));
    assert!(
        !gw.state.sessions.contains_key(sid),
        "a leitura do operador nao poe a sessao em memoria"
    );
    assert_eq!(
        superficies_no_disco(&gw, sid).await.channel_id,
        "discord",
        "a linha no banco nao foi reescrita"
    );

    let (status, corpo) = history_admin(&gw, "nunca-existiu", Some(&admin)).await;
    assert_eq!(status, 404, "{corpo}");

    let trilha =
        gw.admin_store
            .lock()
            .await
            .list_audit_log(50, 0, Some("session"), Some("read_history"));
    assert_eq!(trilha.len(), 2, "uma entrada por leitura servida");
    assert!(trilha.iter().all(|e| e.resource_id.as_deref() == Some(sid)));
    assert!(trilha.iter().all(|e| e.outcome == "success"));
}

/// Sem cookie e `401`; `viewer` (le paineis, mas nao tem `manage_sessions`)
/// e `403`. Nenhum dos dois ve a conversa.
#[tokio::test]
async fn admin_history_sem_cookie_e_401_e_viewer_e_403() {
    let gw = subir().await;
    let _admin = criar_admin(&gw).await;
    let sid = "telegram-1";
    conversa_de(&gw, sid, "telegram", "1").await;

    let (status, corpo) = history_admin(&gw, sid, None).await;
    assert_eq!(status, 401, "{corpo}");
    assert!(sem_conteudo_da_vitima(corpo.to_string().as_bytes()));

    let viewer = sessao_de_viewer(&gw).await;
    let (status, corpo) = history_admin(&gw, sid, Some(&viewer)).await;
    assert_eq!(status, 403, "{corpo}");
    assert!(sem_conteudo_da_vitima(corpo.to_string().as_bytes()));
}

// ── GET /chat/history — mobile ──────────────────────────────────────────

/// Cada JWT le so a sessao do proprio `sub`; sem JWT e `401`; e o id da
/// sessao do mobile pela rota de cliente e `404`.
#[tokio::test]
async fn mobile_chat_history_e_so_a_do_proprio_sub() {
    let gw = subir().await;
    turno(&gw, "mobile-user-a", "mobile", "user-a", "segredo de a").await;
    turno(&gw, "mobile-user-b", "mobile", "user-b", "segredo de b").await;

    let (status, corpo) = history_mobile(&gw, Some(&jwt(&gw, "user-a"))).await;
    assert_eq!(status, 200, "{corpo}");
    assert_eq!(corpo["session_id"], "mobile-user-a");
    assert!(corpo.to_string().contains("segredo de a"));
    assert!(!corpo.to_string().contains("segredo de b"));

    let (status, corpo) = history_mobile(&gw, Some(&jwt(&gw, "user-b"))).await;
    assert_eq!(status, 200, "{corpo}");
    assert_eq!(corpo["session_id"], "mobile-user-b");
    assert!(!corpo.to_string().contains("segredo de a"));

    let (status, _) = history_mobile(&gw, None).await;
    assert_eq!(status, 401);

    let (status, corpo) = history_cliente(&gw, "mobile-user-a").await;
    assert_eq!(status, 404, "{}", String::from_utf8_lossy(&corpo));
}

// ── GET /api/sessions — listagem ────────────────────────────────────────

/// A listagem de cliente nomeia so o que o cliente alcanca; a do operador,
/// tudo. O id de canal carrega telefone/chat id, e nomea-lo ja e confirmar
/// que a conversa existe.
#[tokio::test]
async fn listagem_de_cliente_omite_sessao_de_canal_e_a_administrativa_lista_todas() {
    let gw = subir().await;
    let admin = criar_admin(&gw).await;
    conversa_de(&gw, "telegram-9", "telegram", "9").await;
    conversa_de(&gw, "mobile-user-z", "mobile", "user-z").await;
    conversa_de(&gw, "sessao-da-api", CANAL_DA_API, "anonymous").await;
    conversa_de(&gw, "sessao-do-web", "web", "operador").await;

    let cliente = listar_cliente(&gw).await;
    assert!(
        cliente.contains(&"sessao-da-api".to_string()),
        "{cliente:?}"
    );
    assert!(
        cliente.contains(&"sessao-do-web".to_string()),
        "{cliente:?}"
    );
    assert!(
        !cliente.contains(&"telegram-9".to_string()),
        "a listagem de cliente nao nomeia sessao de canal: {cliente:?}"
    );
    assert!(
        !cliente.contains(&"mobile-user-z".to_string()),
        "nem a do mobile: {cliente:?}"
    );

    let operador = listar_admin(&gw, &admin).await;
    for sid in [
        "telegram-9",
        "mobile-user-z",
        "sessao-da-api",
        "sessao-do-web",
    ] {
        assert!(operador.contains(&sid.to_string()), "{sid} em {operador:?}");
    }
}

// ── DELETE /api/sessions/{id} ───────────────────────────────────────────

/// O `DELETE` por id e outra escrita: numa sessao de canal responde o `404`
/// generico e nao a desconecta, nao revoga nada e nao grava a marca de
/// logout na linha dela.
#[tokio::test]
async fn delete_de_sessao_de_canal_por_id_e_404_e_nao_a_desconecta() {
    let gw = subir().await;
    let sid = "signal-5511888880000";
    conversa_de(&gw, sid, "signal", "5511888880000").await;
    assert_eq!(conectada(&gw, sid), Some(true), "precondicao");

    let (status, corpo) = delete_cliente(&gw, sid).await;

    assert_eq!(status, 404, "{corpo}");
    assert_eq!(corpo["error"], "session not found");
    assert_eq!(
        conectada(&gw, sid),
        Some(true),
        "a sessao do canal segue viva"
    );
    assert_eq!(gw.state.session_history(sid).len(), 2);
    assert!(
        !superficies_no_disco(&gw, sid).await.api_logout,
        "nenhuma marca de logout na linha do canal"
    );
}

// ── /ws — resume sem token ──────────────────────────────────────────────

/// `resume` sem token so retoma sessao das superficies locais: o id de uma
/// sessao de canal em memoria ganha uma sessao nova, e a do canal nao e
/// tocada. A do chat web continua retomavel sem token, como sempre.
#[tokio::test]
async fn ws_resume_sem_token_nao_retoma_sessao_de_canal() {
    let gw = subir().await;
    let sid = "telegram-31337";
    conversa_de(&gw, sid, "telegram", "31337").await;

    let ack = ws_resume_sem_token(&gw, sid).await;

    assert_eq!(ack["type"], "connected", "nao retomada: {ack}");
    assert_ne!(ack["session_id"], sid, "ganhou uma sessao nova: {ack}");
    assert_eq!(canais_dos_turnos(&gw, sid), vec!["telegram".to_string()]);
    assert_eq!(gw.state.session_history(sid).len(), 2);

    let web = "sessao-do-web-viva";
    conversa_de(&gw, web, "web", "operador").await;
    let ack = ws_resume_sem_token(&gw, web).await;
    assert_eq!(ack["type"], "resumed", "sessao do chat web: {ack}");
    assert_eq!(ack["session_id"], web);
    assert_eq!(ack["history_length"], 2);
}

// ── Web Console ─────────────────────────────────────────────────────────

/// O Export da pagina Sessions exporta sessao de canal: tem de vir pela rota
/// administrativa, com o cookie do `/admin`, e nao pela de cliente. Varredura
/// de fonte, no padrao do `mobile_chat.rs`: um teste de navegador exigiria
/// login no `/admin`, e o que precisa ficar preso e a rota que um refactor
/// troca com facilidade.
#[test]
fn console_exporta_pela_rota_administrativa() {
    let html = include_str!("../src/webchat.html");
    let export = html
        .split("async function exportSession(")
        .nth(1)
        .expect("o console tem exportSession");
    let corpo = &export[..export.find("\nasync function ").unwrap_or(export.len())];
    assert!(
        corpo.contains("/admin/api/sessions/") && corpo.contains("/history"),
        "o Export do console tem de usar a rota administrativa: {corpo}"
    );
    assert!(
        html.contains("data-testid=\"session-export\""),
        "o botao de Export precisa de data-testid estavel (contrato de teste)"
    );
    assert!(
        html.contains("data-testid=\"sessions-admin-hint\""),
        "a dica de login no /admin precisa de data-testid estavel"
    );
}
