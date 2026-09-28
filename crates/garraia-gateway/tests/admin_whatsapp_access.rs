//! `GET|POST /admin/api/whatsapp/access` e `GET .../access/audit` (#1402,
//! #1412, #1413, #1414): a API admin e o segundo consumidor do caminho
//! unico de mutacao — le pelo mesmo `visao::documento` da CLI, muda pelo
//! mesmo `mutacao::aplicar`, audita pelo mesmo `auditoria`, com `origem:
//! admin_api` e o username do admin como ator.
//!
//! Um so `#[tokio::test]` por arquivo: `GARRAIA_CONFIG_DIR` e processo-wide.

use std::sync::Arc;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use garraia_agents::{AgentRuntime, McpManager};
use garraia_channels::ChannelRegistry;
use garraia_config::{AppConfig, ChannelConfig, ConfigLoader};
use garraia_gateway::admin::middleware::AuthenticatedAdmin;
use garraia_gateway::admin::rbac::Role;
use garraia_gateway::admin::shared::AdminState;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::admin::whatsapp_access::{
    AccessMutationRequest, AuditQuery, admin_whatsapp_access, admin_whatsapp_access_audit,
    admin_whatsapp_access_mutate,
};
use garraia_gateway::state::AppState;
use tokio::sync::Mutex;

const NUMERO: &str = "5511999998888";
const DONO: &str = "5511977776666";
const ESTRANHO: &str = "5521955554444";

fn admin(role: Role) -> axum::Extension<AuthenticatedAdmin> {
    axum::Extension(AuthenticatedAdmin {
        user_id: "u-1402".to_string(),
        username: "operadora".to_string(),
        role,
        csrf_token: "csrf".to_string(),
        session_token: "sess".to_string(),
    })
}

fn pedido(action: &str) -> AccessMutationRequest {
    AccessMutationRequest {
        action: action.to_string(),
        identity: None,
        identity_last4: None,
        jid: None,
        level: None,
        write: None,
        preset: None,
        enabled: None,
        dry_run: false,
    }
}

async fn corpo(resp: axum::response::Response) -> (StatusCode, serde_json::Value) {
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .expect("body");
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

#[tokio::test]
async fn a_api_admin_le_muda_e_audita_pelo_mesmo_motor_da_cli() {
    let dir = tempfile::tempdir().expect("temp config dir");
    unsafe {
        std::env::set_var("GARRAIA_CONFIG_DIR", dir.path());
    }
    let loader = ConfigLoader::with_dir(dir.path());
    loader.ensure_dirs().expect("dirs");

    let mut config = AppConfig::default();
    let mut settings = std::collections::HashMap::new();
    settings.insert("allow".to_string(), serde_json::json!([NUMERO]));
    settings.insert("owners".to_string(), serde_json::json!([DONO]));
    settings.insert("default_mode".to_string(), serde_json::json!("code"));
    config.channels.insert(
        "whatsapp_linked".to_string(),
        ChannelConfig {
            channel_type: "whatsapp_linked".to_string(),
            enabled: Some(true),
            settings,
        },
    );
    // O `data_dir` do audit e o da config; aponta para dentro do tempdir.
    config.data_dir = Some(dir.path().join("data"));
    loader.save(&config).expect("save");

    let mut state = AppState::new(
        config.clone(),
        Arc::new(AgentRuntime::new()),
        ChannelRegistry::new(),
    );
    state.mcp_manager_arc = Some(Arc::new(McpManager::new()));
    let admin_state = AdminState {
        store: Arc::new(Mutex::new(
            AdminStore::in_memory().expect("in-memory admin store"),
        )),
        app_state: Arc::new(state),
        encryption_key: Arc::new(vec![0u8; 32]),
    };

    // ── GET: o documento da politica, sem identidade ──
    let (status, doc) = corpo(
        admin_whatsapp_access(State(admin_state.clone()), admin(Role::Viewer))
            .await
            .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    assert_eq!(doc["policy"]["admission"], serde_json::json!("restricted"));
    assert_eq!(
        doc["policy"]["execution_profile"],
        serde_json::json!("standard")
    );
    assert!(
        doc.get("hot_reload").is_some(),
        "diz se o gateway rele a quente: {doc}"
    );
    let texto = doc.to_string();
    for numero in [NUMERO, DONO] {
        assert!(
            !texto.contains(numero),
            "a API nunca revela identidade: {texto}"
        );
    }
    assert!(
        !texto.contains("\"identity\""),
        "sem `identity` na API: {texto}"
    );
    let principais = doc["policy"]["principals"].as_array().expect("lista");
    assert!(principais.iter().any(|p| p["principal"] == "dono"));
    assert!(principais.iter().any(|p| p["principal"] == "usuario"));

    // ── Viewer nao muda nada (403) ──
    let (status, _) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Viewer),
            Json(pedido("open")),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // ── POST dry_run: impacto pelo motor real, nada gravado nem auditado ──
    let mut req = pedido("write");
    req.identity = Some(format!("+{NUMERO}"));
    req.write = Some(false);
    req.dry_run = true;
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    assert_eq!(doc["dry_run"], serde_json::json!(true));
    assert_eq!(doc["changed"], serde_json::json!(true));
    assert_eq!(doc["written"], serde_json::json!(false));
    let impacto = doc["impact"].as_array().expect("impact");
    assert!(
        impacto
            .iter()
            .any(|d| d["loses"].as_array().is_some_and(|l| l
                .iter()
                .any(|c| c.as_str().unwrap_or("").contains("escrita")))),
        "{doc}"
    );
    assert!(!doc.to_string().contains(NUMERO));
    let relido = ConfigLoader::with_dir(dir.path())
        .load_sem_env()
        .expect("load");
    // `access.users` traz o legado (`allow`/`owners`) por construcao; o que
    // prova que nada foi gravado e a secao `access` nao existir no arquivo e
    // o usuario continuar como o legado o descreve (sem teto).
    assert!(
        !relido.channels["whatsapp_linked"]
            .settings
            .contains_key("access"),
        "dry_run nao grava: {:?}",
        relido.channels["whatsapp_linked"].settings.get("access")
    );
    let s = garraia_gateway::bootstrap::whatsapp_linked_settings(&relido);
    assert_eq!(
        garraia_gateway::bootstrap::whatsapp_linked_politica::principal_do_turno(
            &s, NUMERO, "x", false, false,
        ),
        garraia_gateway::bootstrap::whatsapp_linked_politica::Principal::Usuario(
            garraia_gateway::bootstrap::whatsapp_linked_politica::Alcance::COMPLETO
        )
    );
    let data_dir = dir.path().join("data");
    assert!(
        garraia_gateway::bootstrap::whatsapp_linked_politica::auditoria::ler(&data_dir, 10)
            .expect("ler")
            .is_empty(),
        "dry_run nao audita"
    );

    // ── #1403: `identity` sem `+` e recusada com codigo estavel, sem gravar;
    // formato US com parenteses e hifens e aceito e gravado so em digitos ──
    let mut req = pedido("level");
    req.identity = Some("21 98888-7777".to_string());
    req.level = Some("read".to_string());
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{doc}");
    assert_eq!(
        doc["error_code"],
        serde_json::json!("identity_missing_country_code"),
        "{doc}"
    );
    assert!(
        !doc["error"].as_str().unwrap_or_default().contains("98888"),
        "o erro nao repete a entrada: {doc}"
    );
    let mut req = pedido("level");
    req.identity = Some("+1 (415) 555-0100".to_string());
    req.level = Some("read".to_string());
    // Preview: prova a normalizacao sem gravar nem auditar (o bloco seguinte
    // conta exatamente um evento de audit).
    req.dry_run = true;
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    let texto = doc.to_string();
    assert!(
        texto.contains("…0100"),
        "o numero US entra normalizado e mascarado: {texto}"
    );
    assert!(!texto.contains("14155550100"), "{texto}");

    // ── POST real: grava, audita com origem admin_api e ator = username ──
    let mut req = pedido("level");
    req.identity = Some(format!("+{NUMERO}"));
    req.level = Some("read".to_string());
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    assert_eq!(doc["written"], serde_json::json!(true));
    assert_eq!(doc["audit"]["written"], serde_json::json!(true), "{doc}");
    assert_eq!(doc["policy"]["admission"], serde_json::json!("restricted"));
    let relido = ConfigLoader::with_dir(dir.path())
        .load_sem_env()
        .expect("load");
    let s = garraia_gateway::bootstrap::whatsapp_linked_settings(&relido);
    let p = garraia_gateway::bootstrap::whatsapp_linked_politica::principal_do_turno(
        &s, NUMERO, "x", false, false,
    );
    assert_eq!(
        p,
        garraia_gateway::bootstrap::whatsapp_linked_politica::Principal::Usuario(
            garraia_gateway::bootstrap::whatsapp_linked_politica::Alcance::LEITURA
        )
    );
    let eventos =
        garraia_gateway::bootstrap::whatsapp_linked_politica::auditoria::ler(&data_dir, 10)
            .expect("ler");
    assert_eq!(eventos.len(), 1);
    assert_eq!(eventos[0].origem, "admin_api");
    assert_eq!(eventos[0].ator, "operadora");
    assert_eq!(eventos[0].acao, "level");
    assert_eq!(eventos[0].alvo.as_deref(), Some("…8888"));

    // ── #1434: `action: preset` ──
    // Sem o campo `preset`: 400 fail-closed, com os nomes validos no texto.
    let mut req = pedido("preset");
    req.identity = Some(format!("+{NUMERO}"));
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{doc}");
    assert!(
        doc["error"].as_str().unwrap_or("").contains("full_pod"),
        "diz o que vale: {doc}"
    );
    // Preset desconhecido (inclusive um nome de NIVEL, que nao e preset): 400.
    let mut req = pedido("preset");
    req.identity = Some(format!("+{NUMERO}"));
    req.preset = Some("full".to_string());
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{doc}");
    // Preset valido: grava `level` + `write`, audita como `preset`.
    let mut req = pedido("preset");
    req.identity = Some(format!("+{NUMERO}"));
    req.preset = Some("developer".to_string());
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    assert_eq!(doc["written"], serde_json::json!(true), "{doc}");
    let eventos =
        garraia_gateway::bootstrap::whatsapp_linked_politica::auditoria::ler(&data_dir, 10)
            .expect("ler");
    assert_eq!(eventos[0].acao, "preset");
    assert_eq!(eventos[0].origem, "admin_api");
    // O `config.yml` guarda `level`/`write`, nunca um campo `preset`.
    let relido = ConfigLoader::with_dir(dir.path())
        .load_sem_env()
        .expect("load");
    let entrada = relido.channels["whatsapp_linked"].settings["access"]["users"][NUMERO]
        .as_object()
        .expect("entrada");
    assert_eq!(entrada["level"], serde_json::json!("full"));
    assert_eq!(entrada["write"], serde_json::json!(true));
    assert!(!entrada.contains_key("preset"), "{entrada:?}");
    // E o GET le de volta o rotulo, calculado do `level`/`write`.
    let (status, doc) = corpo(
        admin_whatsapp_access(State(admin_state.clone()), admin(Role::Viewer))
            .await
            .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    let usuario = doc["policy"]["principals"]
        .as_array()
        .expect("lista")
        .iter()
        .find(|p| p["principal"] == serde_json::json!("usuario"))
        .cloned()
        .expect("usuario");
    assert_eq!(
        usuario["preset"],
        serde_json::json!("full"),
        "developer e full_pod aparecem como `full`: {usuario}"
    );
    assert_eq!(
        doc["policy"]["default"]["preset"],
        serde_json::json!("chat_only"),
        "{doc}"
    );
    // Volta ao estado que o resto do teste espera (`read`, sem escrita).
    let mut req = pedido("preset");
    req.identity = Some(format!("+{NUMERO}"));
    req.preset = Some("read".to_string());
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");

    // ── #1434: `preset` no lugar de `level`/`write` em `default`,
    // `group-default` e `group` — sem acao nova e sem campo obrigatorio novo.
    const GRUPO: &str = "120363000000000000@g.us";
    let mut req = pedido("default");
    req.preset = Some("read".to_string());
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    assert_eq!(
        doc["policy"]["default"]["preset"],
        serde_json::json!("read")
    );
    let eventos =
        garraia_gateway::bootstrap::whatsapp_linked_politica::auditoria::ler(&data_dir, 1)
            .expect("ler");
    assert_eq!(
        eventos[0].acao, "default",
        "a acao auditada e a do alvo, nao `preset`"
    );
    // `developer`/`full_pod` sao `full`: recusados no default do desconhecido,
    // pela MESMA guarda de `level: full` (#1390).
    for nome in ["developer", "full_pod"] {
        let mut req = pedido("default");
        req.preset = Some(nome.to_string());
        let (status, doc) = corpo(
            admin_whatsapp_access_mutate(
                State(admin_state.clone()),
                HeaderMap::new(),
                admin(Role::Admin),
                Json(req),
            )
            .await
            .into_response(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{nome}: {doc}");
        assert!(
            doc["error"].as_str().unwrap_or("").contains("full"),
            "{nome}: {doc}"
        );
    }
    // Grupo PODE ter `full`: os quatro presets valem nos dois alvos de grupo.
    let mut req = pedido("group-default");
    req.preset = Some("chat_only".to_string());
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    let mut req = pedido("group");
    req.jid = Some(GRUPO.to_string());
    req.preset = Some("full_pod".to_string());
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    // O que foi para o `config.yml` continua sendo `level` + `write`.
    let relido = ConfigLoader::with_dir(dir.path())
        .load_sem_env()
        .expect("load");
    let grupos = &relido.channels["whatsapp_linked"].settings["access"]["groups"];
    assert_eq!(grupos["default"]["level"], serde_json::json!("chat"));
    assert_eq!(grupos["default"]["write"], serde_json::json!(false));
    // Um grupo PODE ser `full`: `full_pod` vale aqui, ao contrario do default
    // do desconhecido.
    assert_eq!(grupos[GRUPO]["level"], serde_json::json!("full"));
    assert_eq!(grupos[GRUPO]["write"], serde_json::json!(true));
    assert!(
        !serde_json::to_string(grupos)
            .expect("json")
            .contains("preset"),
        "{grupos}"
    );
    // Volta ao estado que o resto do teste espera (default `chat`).
    let mut req = pedido("default");
    req.level = Some("chat".to_string());
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");

    // ── POST invalido: 400 com a mensagem acionavel, nada muda ──
    let mut req = pedido("default");
    req.level = Some("full".to_string());
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{doc}");
    assert!(
        doc["error"].as_str().unwrap_or("").contains("read"),
        "{doc}"
    );
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(pedido("explodir")),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "acao desconhecida: {doc}");

    // ── open + block + reset pela API, e o audit lista mais recente primeiro ──
    let (status, _) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(pedido("open")),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let mut req = pedido("block");
    req.identity = Some(format!("+{ESTRANHO}"));
    let (status, _) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, doc) = corpo(
        admin_whatsapp_access_audit(
            State(admin_state.clone()),
            admin(Role::Viewer),
            Query(AuditQuery { limit: Some(2) }),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    let eventos = doc["events"].as_array().expect("events");
    assert_eq!(eventos.len(), 2, "limit");
    assert_eq!(eventos[0]["action"], serde_json::json!("block"));
    assert_eq!(eventos[1]["action"], serde_json::json!("open"));
    assert!(!doc.to_string().contains(ESTRANHO));

    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(pedido("reset")),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    assert_eq!(doc["policy"]["admission"], serde_json::json!("restricted"));
    // Segundo reset: nada muda, nada e auditado, 200.
    let antes = garraia_gateway::bootstrap::whatsapp_linked_politica::auditoria::ler(&data_dir, 50)
        .expect("ler")
        .len();
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(pedido("reset")),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    assert_eq!(doc["changed"], serde_json::json!(false));
    let depois =
        garraia_gateway::bootstrap::whatsapp_linked_politica::auditoria::ler(&data_dir, 50)
            .expect("ler")
            .len();
    assert_eq!(antes, depois);

    // ── identity_last4 (o que o console manda), owner/unowner/remove ──
    let mut req = pedido("level");
    req.identity_last4 = Some("…8888".to_string());
    req.level = Some("read".to_string());
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    let mut req = pedido("owner");
    req.identity_last4 = Some("8888".to_string());
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    let relido = ConfigLoader::with_dir(dir.path())
        .load_sem_env()
        .expect("load");
    let s = garraia_gateway::bootstrap::whatsapp_linked_settings(&relido);
    assert!(s.e_dono(NUMERO), "virou dono por role");
    // Duas identidades com o mesmo final: 409, e nada muda.
    let mut req = pedido("level");
    req.identity = Some("+55 21 99999-8888".to_string());
    req.level = Some("chat".to_string());
    let (status, _) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let mut req = pedido("block");
    req.identity_last4 = Some("8888".to_string());
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{doc}");
    let mut req = pedido("block");
    req.identity_last4 = Some("0000".to_string());
    let (status, _) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "final que ninguem tem");
    // unowner preserva o acesso; remove tira de tudo.
    let mut req = pedido("unowner");
    req.identity = Some(format!("+{NUMERO}"));
    let (status, _) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let mut req = pedido("remove");
    req.identity = Some(format!("+{NUMERO}"));
    let (status, doc) = corpo(
        admin_whatsapp_access_mutate(
            State(admin_state.clone()),
            HeaderMap::new(),
            admin(Role::Admin),
            Json(req),
        )
        .await
        .into_response(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    let relido = ConfigLoader::with_dir(dir.path())
        .load_sem_env()
        .expect("load");
    let s = garraia_gateway::bootstrap::whatsapp_linked_settings(&relido);
    assert_eq!(
        garraia_gateway::bootstrap::whatsapp_linked_politica::principal_do_turno(
            &s, NUMERO, "x", false, false,
        ),
        garraia_gateway::bootstrap::whatsapp_linked_politica::Principal::Estranho
    );
    assert!(!s.allow.iter().any(|a| a == NUMERO), "saiu do allow");
    let eventos =
        garraia_gateway::bootstrap::whatsapp_linked_politica::auditoria::ler(&data_dir, 10)
            .expect("ler");
    assert_eq!(eventos[0].acao, "remove");
    assert_eq!(eventos[1].acao, "unowner");

    unsafe {
        std::env::remove_var("GARRAIA_CONFIG_DIR");
    }
}
