//! #1121: o segundo fator no login do painel, montado no `build_router` de
//! verdade.
//!
//! Os testes de unidade da store provam o estado (segredo pendente, ligado,
//! contador de tentativas). So este arquivo prova que o gate esta **no caminho
//! do login**: que com 2FA ligado a senha nao basta, que o codigo errado nao
//! abre sessao, que o codigo certo abre, e que a sexta tentativa errada
//! trava. Um gate implementado no handler errado — ou num router que nao e o
//! que sobe — passaria nos outros e falharia aqui.
//!
//! Nenhum teste usa rede nem relogio falso: o codigo vem de
//! `totp::current_code`, que le a mesma hora que o handler vai ler.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use garraia_agents::AgentRuntime;
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_gateway::admin::middleware::{CSRF_HEADER, SESSION_COOKIE_NAME};
use garraia_gateway::admin::rbac::Role;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::push_channels::PushChannelStates;
use garraia_gateway::router::build_router;
use garraia_gateway::state::AppState;
use serde_json::{Value, json};
use tokio::sync::Mutex;
use tower::ServiceExt;

const USUARIO: &str = "dono";
const SENHA: &str = "senha-do-painel-de-teste";

/// `(status, corpo, valor do cookie de sessao, se vier)`
type Resposta = (StatusCode, Value, Option<String>);

/// Usuario criado + router montado sobre a **mesma** store, para que os testes
/// possam ligar o 2FA por dentro e ver o gate por fora.
fn cenario() -> (Router, Arc<Mutex<AdminStore>>) {
    let store = AdminStore::in_memory().expect("store em memoria");
    store
        .create_user(USUARIO, SENHA, Role::Admin)
        .expect("usuario de teste");

    let config = AppConfig::default();
    let state = Arc::new(AppState::new(
        config,
        Arc::new(AgentRuntime::new()),
        ChannelRegistry::new(),
    ));
    let admin_store = Arc::new(Mutex::new(store));
    let router = build_router(
        state,
        PushChannelStates::empty(),
        Arc::clone(&admin_store),
        Arc::new(vec![0u8; 32]),
    );
    (router, admin_store)
}

async fn chama(
    router: &Router,
    metodo: &str,
    uri: &str,
    corpo: Option<Value>,
    cookie: Option<&str>,
    csrf: Option<&str>,
) -> Resposta {
    let mut req = Request::builder().method(metodo).uri(uri);
    req = req.header(header::CONTENT_TYPE, "application/json");
    if let Some(c) = cookie {
        req = req.header(header::COOKIE, format!("{SESSION_COOKIE_NAME}={c}"));
    }
    if let Some(t) = csrf {
        req = req.header(CSRF_HEADER, t);
    }
    let body = corpo
        .map(|v| Body::from(serde_json::to_vec(&v).expect("json")))
        .unwrap_or_else(Body::empty);
    let mut req = req.body(body).expect("request");

    // O rate limiter le o IP do par em `ConnectInfo`, que em producao vem do
    // `into_make_service_with_connect_info`. Sem ele todo pedido morre em 500
    // no governor.
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            [127, 0, 0, 1],
            40414,
        ))));

    let resp = router.clone().oneshot(req).await.expect("resposta");
    let status = resp.status();
    let sessao = resp
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .and_then(|v| v.strip_prefix(&format!("{SESSION_COOKIE_NAME}=")))
        .map(str::to_string);
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("corpo");
    let json: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, json, sessao)
}

async fn login(router: &Router, corpo: Value) -> Resposta {
    chama(router, "POST", "/admin/api/login", Some(corpo), None, None).await
}

/// Login basico (sem 2FA) — devolve `(cookie, csrf)`.
async fn entrar(router: &Router) -> (String, String) {
    let (status, json, cookie) =
        login(router, json!({"username": USUARIO, "password": SENHA})).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "login sem 2FA deveria entrar: {json}"
    );
    let cookie = cookie.expect("sessao deveria vir no Set-Cookie");
    let csrf = json["csrf_token"]
        .as_str()
        .expect("csrf na resposta")
        .to_string();
    (cookie, csrf)
}

/// Liga o 2FA pela store (o enrollment por HTTP tem teste proprio) e devolve o
/// segredo, para o teste poder gerar codigo valido.
async fn ligar_2fa(store: &Arc<Mutex<AdminStore>>) -> String {
    let guard = store.lock().await;
    let user = guard
        .get_user_by_username(USUARIO)
        .expect("usuario de teste existe");
    let secret = garraia_gateway::totp::generate_totp_secret().expect("segredo");
    guard
        .set_pending_totp_secret(&user.id, &secret)
        .expect("segredo pendente");
    guard.enable_totp(&user.id).expect("2fa ligado");
    drop(guard);
    secret
}

#[tokio::test]
async fn sem_2fa_a_senha_abre_a_sessao() {
    let (router, _) = cenario();
    let (status, json, _) = login(&router, json!({"username": USUARIO, "password": SENHA})).await;
    assert_eq!(status, StatusCode::OK, "senha so nao abriu: {json}");
}

#[tokio::test]
async fn com_2fa_a_senha_so_nao_basta_e_a_resposta_diz_por_que() {
    let (router, store) = cenario();
    ligar_2fa(&store).await;

    let (status, json, _) = login(&router, json!({"username": USUARIO, "password": SENHA})).await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        json["totp_required"], true,
        "o cliente precisa saber que e falta de codigo, nao senha errada: {json}"
    );
}

#[tokio::test]
async fn codigo_errado_nao_abre_a_sessao() {
    let (router, store) = cenario();
    ligar_2fa(&store).await;

    let (status, json, _) = login(
        &router,
        json!({"username": USUARIO, "password": SENHA, "totp_code": "000000"}),
    )
    .await;

    assert_ne!(status, StatusCode::OK, "codigo errado abriu sessao: {json}");
    assert_eq!(json["totp_required"], true);
}

#[tokio::test]
async fn codigo_certo_abre_a_sessao() {
    let (router, store) = cenario();
    let secret = ligar_2fa(&store).await;
    let code = garraia_gateway::totp::current_code(&secret).expect("codigo atual");

    let (status, json, cookie) = login(
        &router,
        json!({"username": USUARIO, "password": SENHA, "totp_code": code}),
    )
    .await;

    assert_eq!(status, StatusCode::OK, "codigo certo nao abriu: {json}");
    assert!(cookie.is_some(), "sessao sem cookie: {json}");
}

#[tokio::test]
async fn tantas_tentativas_erradas_travam_o_segundo_fator() {
    let (router, store) = cenario();
    ligar_2fa(&store).await;

    let mut ultimo = StatusCode::OK;
    for _ in 0..6 {
        let (status, _, _) = login(
            &router,
            json!({"username": USUARIO, "password": SENHA, "totp_code": "000000"}),
        )
        .await;
        ultimo = status;
    }

    assert_eq!(
        ultimo,
        StatusCode::TOO_MANY_REQUESTS,
        "sem travamento, o segundo fator e forcavel por forca bruta"
    );
}

/// O enrollment por HTTP: setup devolve um segredo pendente, e ele so passa a
/// valer depois que um codigo confirma.
#[tokio::test]
async fn enrollment_por_http_precisa_de_um_codigo_valido() {
    let (router, _store) = cenario();
    let (cookie, csrf) = entrar(&router).await;

    let (status, json, _) = chama(
        &router,
        "POST",
        "/admin/api/2fa/setup",
        Some(json!({})),
        Some(&cookie),
        Some(&csrf),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "setup deveria gerar segredo: {json}"
    );
    let secret = json["secret"]
        .as_str()
        .expect("segredo na resposta")
        .to_string();
    assert!(
        json["qr_uri"]
            .as_str()
            .is_some_and(|u| u.starts_with("otpauth://totp/")),
        "sem URI de QR: {json}"
    );

    // Pendente: o login ainda nao exige nada.
    let (status, json, _) = login(&router, json!({"username": USUARIO, "password": SENHA})).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "segredo pendente nao pode exigir codigo: {json}"
    );

    // Codigo errado nao confirma.
    let (status, _, _) = chama(
        &router,
        "POST",
        "/admin/api/2fa/verify",
        Some(json!({"code": "000000"})),
        Some(&cookie),
        Some(&csrf),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Codigo certo confirma, e dai a senha so nao basta mais.
    let code = garraia_gateway::totp::current_code(&secret).expect("codigo atual");
    let (status, json, _) = chama(
        &router,
        "POST",
        "/admin/api/2fa/verify",
        Some(json!({"code": code})),
        Some(&cookie),
        Some(&csrf),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "codigo certo nao confirmou: {json}");

    let (status, _, _) = login(&router, json!({"username": USUARIO, "password": SENHA})).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Refazer setup com 2FA ligado e recusado: exige desligar com codigo.
    let (status, _, _) = chama(
        &router,
        "POST",
        "/admin/api/2fa/setup",
        Some(json!({})),
        Some(&cookie),
        Some(&csrf),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "girar o segredo por baixo do dono o deixaria fora do painel"
    );
}

/// Mesma montagem do `cenario()`, mas sobre um banco **em arquivo**: so assim
/// o teste pode mutilar o schema por uma segunda conexao, fora da store, e
/// provar o que o login faz quando o estado de 2FA nao pode ser lido.
fn cenario_arquivo(
    dir: &tempfile::TempDir,
) -> (Router, Arc<Mutex<AdminStore>>, std::path::PathBuf) {
    let path = dir.path().join("admin.db");
    let store = AdminStore::open(&path).expect("store em arquivo");
    store
        .create_user(USUARIO, SENHA, Role::Admin)
        .expect("usuario de teste");

    let config = AppConfig::default();
    let state = Arc::new(AppState::new(
        config,
        Arc::new(AgentRuntime::new()),
        ChannelRegistry::new(),
    ));
    let admin_store = Arc::new(Mutex::new(store));
    let router = build_router(
        state,
        PushChannelStates::empty(),
        Arc::clone(&admin_store),
        Arc::new(vec![0u8; 32]),
    );
    (router, admin_store, path)
}

/// #1121 (regressao dos vereditos de seguranca): estado de 2FA ilegivel
/// recusa o login, sem sessao. O caminho contrario — engolir o erro de
/// leitura como "2FA desligado" e abrir sessao so com a senha — e exatamente
/// o fail-open que este PR veio fechar. `verify_password` faz SELECT
/// explicito de apenas `id, password_hash, password_salt, role`, entao
/// derrubar somente a coluna `totp_enabled` isola o gate: antes da correcao
/// este login virava 200 com cookie de sessao.
#[tokio::test]
async fn estado_de_2fa_ilegivel_recusa_o_login_sem_abrir_sessao() {
    let dir = tempfile::tempdir().expect("diretorio temporario");
    let (router, store, path) = cenario_arquivo(&dir);
    let secret = ligar_2fa(&store).await;

    // Mutila o schema de fora da store: a coluna que o gate le deixa de
    // existir enquanto senha, sessao e audit_log continuam de pe.
    let conn = rusqlite::Connection::open(&path).expect("segunda conexao");
    conn.execute("ALTER TABLE admin_users DROP COLUMN totp_enabled", [])
        .expect("derrubar a coluna do gate");

    // Senha certa **e** codigo certo: com o estado ilegivel nenhum dos dois
    // pode decidir nada — autenticacao nao se faz no escuro.
    let code = garraia_gateway::totp::current_code(&secret).expect("codigo atual");
    let (status, json, sessao) = login(
        &router,
        json!({"username": USUARIO, "password": SENHA, "totp_code": code}),
    )
    .await;

    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "estado ilegivel tem que virar recusa 500, nao sessao: {json}"
    );
    assert!(
        sessao.is_none(),
        "sessao criada com o gate ilegivel e o fail-open de volta"
    );
}

/// #1121 (pass-3): erro de leitura do SEGREDO e recusa 500 — nao pode virar
/// "codigo invalido" (login), "2FA nao configurado" (verify) nem
/// "2FA nao ligado" (disable). O `totp_enabled` continua de pe; so a coluna
/// do segredo some, isolando exatamente a consulta que o veredito apontou.
#[tokio::test]
async fn segredo_ilegivel_recusa_sem_mentir_sobre_o_estado() {
    let dir = tempfile::tempdir().expect("diretorio temporario");
    let (router, store, path) = cenario_arquivo(&dir);
    let secret = ligar_2fa(&store).await;

    let conn = rusqlite::Connection::open(&path).expect("segunda conexao");
    conn.execute("ALTER TABLE admin_users DROP COLUMN totp_secret", [])
        .expect("derrubar a coluna do segredo");
    drop(conn);

    // Login: senha certa E codigo certo, mas o segredo nao pode ser lido —
    // sem sessao, sem "codigo invalido".
    let code = garraia_gateway::totp::current_code(&secret).expect("codigo atual");
    let (status, json, sessao) = login(
        &router,
        json!({"username": USUARIO, "password": SENHA, "totp_code": code}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "erro de leitura do segredo no login tem que virar 500: {json}"
    );
    assert!(sessao.is_none(), "sem segredo legivel, sem sessao");
}

/// Idem, pelos endpoints de enrollment: com sessao de admin na mao e um
/// segredo pendente guardado, a coluna do segredo some antes do pedido. O
/// `verify` e o `disable` recebem 500 (nao os 400 de "nao configurado"), e
/// o `status` — que le outra coluna — segue respondendo.
#[tokio::test]
async fn segredo_ilegivel_nos_endpoints_de_enrollment_e_500_nao_400() {
    let dir = tempfile::tempdir().expect("diretorio temporario");
    let (router, _store, path) = cenario_arquivo(&dir);
    let (cookie, csrf) = entrar(&router).await;

    // Segredo pendente guardado com a coluna ainda de pe.
    let (_, json, _) = chama(
        &router,
        "POST",
        "/admin/api/2fa/setup",
        Some(json!({})),
        Some(&cookie),
        Some(&csrf),
    )
    .await;
    assert!(
        json["secret"].as_str().is_some(),
        "setup deveria guardar o segredo pendente: {json}"
    );

    let conn = rusqlite::Connection::open(&path).expect("segunda conexao");
    conn.execute("ALTER TABLE admin_users DROP COLUMN totp_secret", [])
        .expect("derrubar a coluna do segredo");
    drop(conn);

    for uri in ["/admin/api/2fa/verify", "/admin/api/2fa/disable"] {
        let (status, json, _) = chama(
            &router,
            "POST",
            uri,
            Some(json!({"code": "000000"})),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(
            status,
            StatusCode::INTERNAL_SERVER_ERROR,
            "{uri}: erro de leitura do segredo e 500, nao '2FA nao configurado': {json}"
        );
    }

    // O status le `totp_enabled`, que continua de pe: 200 e a resposta de
    // sempre (nao ligado — o segredo estava so pendente).
    let (status, json, _) = chama(
        &router,
        "GET",
        "/admin/api/2fa/status",
        None,
        Some(&cookie),
        Some(&csrf),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "status le outra coluna: {json}");
    assert_eq!(json["enabled"], false);
}

/// #1121 (pass-4): segredo VAZIO com 2FA ligado e estado inconsistente —
/// corrupcao manual, migration defeituosa. Nenhum dos dois caminhos pode
/// mentir o motivo: o login nao pode virar "codigo invalido" com sessao
/// destravada na contagem de lockout, e o disable nao pode deixar o 2FA
/// ligado irrecuperavel respondendo 401 para sempre. Os dois recusam 500.
#[tokio::test]
async fn segredo_vazio_recusa_login_e_disable_com_estado_inconsistente() {
    let dir = tempfile::tempdir().expect("diretorio temporario");
    let (router, store, path) = cenario_arquivo(&dir);
    // Sessao primeiro: depois que o 2FA liga, o login exigiria codigo.
    let (cookie, csrf) = entrar(&router).await;
    let secret = ligar_2fa(&store).await;
    let code = garraia_gateway::totp::current_code(&secret).expect("codigo atual");

    let conn = rusqlite::Connection::open(&path).expect("segunda conexao");
    conn.execute(
        "UPDATE admin_users SET totp_secret = '' WHERE username = ?1",
        [USUARIO],
    )
    .expect("esvaziar o segredo");

    // Login: senha certa E codigo certo — mas o segredo guardado e vazio,
    // e avaliar o codigo contra ele seria decidir no escuro.
    let (status, json, sessao) = login(
        &router,
        json!({"username": USUARIO, "password": SENHA, "totp_code": code}),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "segredo vazio no login tem que virar 500: {json}"
    );
    assert!(sessao.is_none(), "estado inconsistente nao abre sessao");

    // Disable com sessao de admin: sem segredo legivel, o 2FA ligado nao
    // pode ser desligado avaliando o codigo contra nada — 500, nao 401.
    let (status, json, _) = chama(
        &router,
        "POST",
        "/admin/api/2fa/disable",
        Some(json!({"code": code})),
        Some(&cookie),
        Some(&csrf),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "segredo vazio no disable tem que virar 500, nao 'codigo invalido': {json}"
    );
}

/// #1121 (pass-4): o mesmo estado inconsistente no `verify`, onde o segredo
/// e PENDENTE (2FA ainda nao ligado): a recusa e 500, nao o 400 de "2FA nao
/// configurado" nem o 401 de codigo errado — o codigo certo estava la, o que
/// falta e o segredo contra o qual avalia-lo.
#[tokio::test]
async fn segredo_vazio_no_verify_e_estado_inconsistente() {
    let dir = tempfile::tempdir().expect("diretorio temporario");
    let (router, _store, path) = cenario_arquivo(&dir);
    let (cookie, csrf) = entrar(&router).await;

    let (_, json, _) = chama(
        &router,
        "POST",
        "/admin/api/2fa/setup",
        Some(json!({})),
        Some(&cookie),
        Some(&csrf),
    )
    .await;
    let secret = json["secret"]
        .as_str()
        .expect("setup deveria devolver o segredo pendente")
        .to_string();

    let conn = rusqlite::Connection::open(&path).expect("segunda conexao");
    conn.execute(
        "UPDATE admin_users SET totp_secret = '' WHERE username = ?1",
        [USUARIO],
    )
    .expect("esvaziar o segredo");

    let code = garraia_gateway::totp::current_code(&secret).expect("codigo atual");
    let (status, json, _) = chama(
        &router,
        "POST",
        "/admin/api/2fa/verify",
        Some(json!({"code": code})),
        Some(&cookie),
        Some(&csrf),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::INTERNAL_SERVER_ERROR,
        "segredo vazio no verify e 500, nao 'nao configurado' nem 'codigo invalido': {json}"
    );
}
