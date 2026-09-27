//! `GET /admin/api/whatsapp/doctor` (#1420): o card "Test WhatsApp" do Web
//! Console roda o MESMO motor do `garraia doctor whatsapp`, colhido em
//! processo. O que este teste prova:
//!
//! 1. Viewer le (`Channels/Read`) e recebe as linhas do doctor — vinculo,
//!    gateway, acesso, perfil, workspace, MCP e provider.
//! 2. O shape e o `report` do `garraia doctor whatsapp --json`: `status`,
//!    `version`, `checks[]` com `id`/`status`/`detail`/`next_step` e nada mais,
//!    no vocabulario do `/api/diagnostics`.
//! 3. O que a CLI diria com o mesmo disco e a mesma config, o console diz:
//!    sem sessao e vermelho com o passo de vincular, o gateway (este processo)
//!    responde pela ponte, o acesso e contagem.
//! 4. `?lang=en` muda o texto; lixo cai em pt-BR.
//! 5. Nenhum segredo (chave do gateway, chave do provider) nem identidade
//!    (numero, dono) aparece no corpo.
//!
//! Um so `#[tokio::test]` por arquivo: `GARRAIA_CONFIG_DIR` e processo-wide.

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use garraia_agents::{AgentRuntime, McpManager};
use garraia_channels::ChannelRegistry;
use garraia_config::{AppConfig, ChannelConfig, ConfigLoader, LlmProviderConfig};
use garraia_gateway::admin::middleware::AuthenticatedAdmin;
use garraia_gateway::admin::rbac::Role;
use garraia_gateway::admin::shared::AdminState;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::admin::whatsapp_doctor::{DoctorQuery, admin_whatsapp_doctor};
use garraia_gateway::state::AppState;
use tokio::sync::Mutex;

const NUMERO: &str = "5511999998888";
const DONO: &str = "5511977776666";
/// Com "cara" de segredo, para a varredura do corpo ter o que procurar.
const CHAVE_DO_GATEWAY: &str = "gw-segredo-0123456789abcdef0123456789";
const CHAVE_DO_PROVIDER: &str = "sk-or-segredo-abcdef0123456789abcdef01";

const IDS_ESPERADOS: [&str; 7] = [
    "whatsapp.linked",
    "whatsapp.gateway",
    "whatsapp.access",
    "execution.profile",
    "files.workspace",
    "mcp.visibility",
    "provider.default",
];

fn admin(role: Role) -> axum::Extension<AuthenticatedAdmin> {
    axum::Extension(AuthenticatedAdmin {
        user_id: "u-1420".to_string(),
        username: "operadora".to_string(),
        role,
        csrf_token: "csrf".to_string(),
        session_token: "sess".to_string(),
    })
}

async fn corpo(resp: axum::response::Response) -> (StatusCode, serde_json::Value) {
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
        .await
        .expect("body");
    let json = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    (status, json)
}

async fn doctor(
    state: &AdminState,
    role: Role,
    lang: Option<&str>,
) -> (StatusCode, serde_json::Value) {
    corpo(
        admin_whatsapp_doctor(
            State(state.clone()),
            admin(role),
            Query(DoctorQuery {
                lang: lang.map(str::to_string),
            }),
        )
        .await
        .into_response(),
    )
    .await
}

fn check<'a>(doc: &'a serde_json::Value, id: &str) -> &'a serde_json::Value {
    doc["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .find(|c| c["id"] == id)
        .unwrap_or_else(|| panic!("sem a linha {id}: {doc}"))
}

#[tokio::test]
async fn o_console_roda_o_mesmo_motor_do_doctor_whatsapp_da_cli() {
    let dir = tempfile::tempdir().expect("temp config dir");
    unsafe {
        std::env::set_var("GARRAIA_CONFIG_DIR", dir.path());
    }
    let loader = ConfigLoader::with_dir(dir.path());
    loader.ensure_dirs().expect("dirs");

    let mut config = AppConfig::default();
    let mut settings = HashMap::new();
    settings.insert("allow".to_string(), serde_json::json!([NUMERO]));
    settings.insert("owners".to_string(), serde_json::json!([DONO]));
    config.channels.insert(
        "whatsapp_linked".to_string(),
        ChannelConfig {
            channel_type: "whatsapp_linked".to_string(),
            enabled: Some(true),
            settings,
        },
    );
    config.gateway.api_key = Some(CHAVE_DO_GATEWAY.to_string());
    config.llm.insert(
        "openrouter".to_string(),
        LlmProviderConfig {
            provider: "openrouter".to_string(),
            model: Some("z-ai/glm-5.3-flash".to_string()),
            api_key: Some(CHAVE_DO_PROVIDER.to_string()),
            base_url: None,
            extra: HashMap::new(),
        },
    );
    config.agent.default_provider = Some("openrouter".to_string());
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

    // ── 1. Viewer le: 200 com as linhas do doctor ──
    let (status, doc) = doctor(&admin_state, Role::Viewer, None).await;
    assert_eq!(status, StatusCode::OK, "{doc}");
    let ids: Vec<&str> = doc["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .filter_map(|c| c["id"].as_str())
        .collect();
    for esperado in IDS_ESPERADOS {
        assert!(ids.contains(&esperado), "sem {esperado} em {ids:?}");
    }
    // Sem sessao em disco nao ha chave a julgar — como na CLI.
    assert!(
        !ids.contains(&"whatsapp.session_key"),
        "session_key so com vinculo: {ids:?}"
    );

    // ── 2. O shape do `report` do `garraia doctor whatsapp --json` ──
    assert_eq!(doc["version"], serde_json::json!(env!("CARGO_PKG_VERSION")));
    assert_eq!(doc["lang"], serde_json::json!("pt"));
    assert!(
        ["ok", "warning", "error"].contains(&doc["status"].as_str().unwrap_or("?")),
        "{doc}"
    );
    for c in doc["checks"].as_array().expect("checks") {
        let mut chaves: Vec<&str> = c
            .as_object()
            .expect("objeto")
            .keys()
            .map(String::as_str)
            .collect();
        chaves.sort_unstable();
        assert!(
            chaves == ["detail", "id", "status"]
                || chaves == ["detail", "id", "next_step", "status"],
            "chaves fora do contrato da CLI: {chaves:?} em {c}"
        );
        let s = c["status"].as_str().unwrap_or("?");
        assert!(
            ["ok", "warning", "error", "not_configured"].contains(&s),
            "status fora do vocabulario do diagnostics: {c}"
        );
        assert!(c["detail"].is_string(), "{c}");
        if s == "warning" || s == "error" {
            assert!(
                c["next_step"].as_str().is_some_and(|p| !p.is_empty()),
                "linha nao-ok sem proximo passo: {c}"
            );
        }
    }

    // ── 3. Consistente com o que a CLI diria com este disco e esta config ──
    let vinculo = check(&doc, "whatsapp.linked");
    assert_eq!(vinculo["status"], serde_json::json!("error"), "{vinculo}");
    assert!(
        vinculo["next_step"]
            .as_str()
            .unwrap_or("")
            .contains("whatsapp"),
        "o passo e vincular: {vinculo}"
    );
    // O gateway e ESTE processo: a linha traz o bind e o que o proprio
    // diagnostics diz da ponte — nunca "nao respondeu" nem "parado".
    let gw = check(&doc, "whatsapp.gateway");
    let detalhe = gw["detail"].as_str().unwrap_or("");
    assert!(
        detalhe.contains(&config.gateway.host) && detalhe.contains("ponte"),
        "{gw}"
    );
    assert!(
        !detalhe.contains("nao respondeu") && !detalhe.contains("parado"),
        "{gw}"
    );
    let acesso = check(&doc, "whatsapp.access");
    assert_eq!(acesso["status"], serde_json::json!("ok"), "{acesso}");
    let detalhe = acesso["detail"].as_str().unwrap_or("");
    assert!(
        detalhe.contains("autorizados: 2") && detalhe.contains("donos: 1"),
        "contagens, nunca identidades: {acesso}"
    );
    // Com o gateway de pe o provider e o que ele registrou — e este runtime
    // de teste nao registrou nenhum.
    let provider = check(&doc, "provider.default");
    assert_eq!(provider["status"], serde_json::json!("error"), "{provider}");
    assert!(
        provider["detail"]
            .as_str()
            .unwrap_or("")
            .contains("segundo o gateway"),
        "{provider}"
    );

    // ── 4. `?lang=en` muda o texto; lixo cai em pt-BR ──
    let (status, en) = doctor(&admin_state, Role::Viewer, Some("en-US")).await;
    assert_eq!(status, StatusCode::OK, "{en}");
    assert_eq!(en["lang"], serde_json::json!("en"));
    let vinculo_en = check(&en, "whatsapp.linked");
    assert_eq!(
        vinculo_en["detail"],
        serde_json::json!("no personal WhatsApp linked"),
        "{vinculo_en}"
    );
    assert_ne!(vinculo_en["detail"], vinculo["detail"]);
    assert_eq!(
        vinculo_en["status"], vinculo["status"],
        "o idioma muda o texto, nunca o semaforo"
    );
    let (_, lixo) = doctor(&admin_state, Role::Viewer, Some("xx")).await;
    assert_eq!(lixo["lang"], serde_json::json!("pt"));

    // ── 5. Nenhum segredo nem identidade no corpo ──
    for texto in [doc.to_string(), en.to_string()] {
        for proibido in [CHAVE_DO_GATEWAY, CHAVE_DO_PROVIDER, NUMERO, DONO] {
            assert!(!texto.contains(proibido), "{proibido} em {texto}");
        }
    }

    unsafe {
        std::env::remove_var("GARRAIA_CONFIG_DIR");
    }
}
