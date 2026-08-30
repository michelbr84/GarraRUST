//! Integration tests for the Projects API (Phase 1.3).
//!
//! Tests the full CRUD lifecycle: create, list, get, update, delete.
//! Spins up a real gateway server on a random port and uses HTTP requests.

use std::net::TcpListener;

use garraia_config::AppConfig;
use garraia_gateway::GatewayServer;
use serde_json::json;
use serial_test::serial;

/// Pick a random available port.
fn random_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind to random port");
    listener.local_addr().unwrap().port()
}

/// Ambiente de um teste: a URL base e os diretórios temporários que precisam
/// continuar vivos enquanto ele roda.
///
/// Antes o `TempDir` era descartado no fim de `start_test_gateway`, o que
/// apagava o diretório enquanto o servidor ainda apontava para ele. Agora ele
/// é devolvido para o teste segurar.
struct TestEnv {
    base: String,
    /// Raiz permitida para projetos neste teste.
    root: std::path::PathBuf,
    _config_dir: tempfile::TempDir,
    _root_dir: tempfile::TempDir,
}

/// Start the gateway in the background and return the base URL.
/// Waits up to 30 seconds for the server to accept connections.
///
/// Uses a temporary config directory to avoid loading real MCP server
/// configs from the user's home directory during tests.
async fn start_test_gateway() -> TestEnv {
    let port = random_port();
    let mut config = AppConfig::default();
    config.gateway.port = port;
    config.memory.enabled = false;
    config.mcp.clear();

    // Point config dir to a temp location so the loader won't find
    // any disk-based MCP configs (garraia.yaml, mcp.json, etc.)
    let tmp = tempfile::tempdir().expect("create temp config dir");
    // Raiz permitida para `path` de projeto. Sem isto o default seria o home
    // do usuário, e o teste dependeria da máquina que o roda.
    let root_dir = tempfile::tempdir().expect("create temp project root");
    let root = std::fs::canonicalize(root_dir.path()).expect("canonicalize project root");

    // SAFETY: we are in a test and no other threads are reading this env var yet.
    unsafe {
        std::env::set_var("GARRAIA_CONFIG_DIR", tmp.path().to_str().unwrap());
        std::env::set_var(
            garraia_gateway::project_root::ROOTS_ENV,
            root.to_str().unwrap(),
        );
    }

    tokio::spawn(async move {
        let server = GatewayServer::new(config);
        let _ = server.run().await;
    });

    // Wait for the server to actually accept TCP connections
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .build()
        .expect("build reqwest client");

    for _ in 0..60 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if client
            .get(format!("http://127.0.0.1:{port}/health"))
            .send()
            .await
            .is_ok()
        {
            break;
        }
    }

    TestEnv {
        base: format!("http://127.0.0.1:{port}"),
        root,
        _config_dir: tmp,
        _root_dir: root_dir,
    }
}

#[tokio::test]
#[serial]
async fn project_crud_lifecycle() {
    let env = start_test_gateway().await;
    let base = &env.base;
    let client = reqwest::Client::new();

    // O `path` precisa ser um diretório existente sob a raiz permitida — antes
    // este teste usava `/tmp/test-project`, que nem existia.
    let project_dir = env.root.join("test-project");
    std::fs::create_dir_all(&project_dir).expect("create project dir");

    // ── Create ────────────────────────────────────────────────────────────
    let create_resp = client
        .post(format!("{base}/api/projects"))
        .json(&json!({
            "name": "test-project",
            "path": project_dir.to_str().unwrap(),
            "description": "A test project"
        }))
        .send()
        .await
        .expect("create request should succeed");

    assert_eq!(create_resp.status(), 201, "create should return 201");

    let create_body: serde_json::Value = create_resp.json().await.expect("valid JSON");
    let project_id = create_body["project"]["id"]
        .as_str()
        .expect("should have project id")
        .to_string();
    assert_eq!(create_body["project"]["name"], "test-project");
    assert_eq!(
        create_body["project"]["path"],
        project_dir.to_str().unwrap(),
        "o path guardado deve ser o canonicalizado"
    );
    assert_eq!(create_body["project"]["description"], "A test project");

    // ── List ──────────────────────────────────────────────────────────────
    let list_resp = client
        .get(format!("{base}/api/projects"))
        .send()
        .await
        .expect("list request should succeed");

    assert_eq!(list_resp.status(), 200);
    let list_body: serde_json::Value = list_resp.json().await.expect("valid JSON");
    let projects = list_body["projects"].as_array().expect("should be array");
    assert!(
        projects
            .iter()
            .any(|p| p["id"].as_str() == Some(&project_id)),
        "created project should appear in list"
    );

    // ── Get ───────────────────────────────────────────────────────────────
    let get_resp = client
        .get(format!("{base}/api/projects/{project_id}"))
        .send()
        .await
        .expect("get request should succeed");

    assert_eq!(get_resp.status(), 200);
    let get_body: serde_json::Value = get_resp.json().await.expect("valid JSON");
    assert_eq!(get_body["project"]["id"], project_id);
    assert_eq!(get_body["project"]["name"], "test-project");

    // ── Update ────────────────────────────────────────────────────────────
    let update_resp = client
        .put(format!("{base}/api/projects/{project_id}"))
        .json(&json!({
            "name": "renamed-project",
            "description": "Updated description"
        }))
        .send()
        .await
        .expect("update request should succeed");

    assert_eq!(update_resp.status(), 200);
    let update_body: serde_json::Value = update_resp.json().await.expect("valid JSON");
    assert_eq!(update_body["project"]["name"], "renamed-project");

    // Verify update persisted via GET
    let verify_resp = client
        .get(format!("{base}/api/projects/{project_id}"))
        .send()
        .await
        .expect("verify request should succeed");
    let verify_body: serde_json::Value = verify_resp.json().await.expect("valid JSON");
    assert_eq!(verify_body["project"]["name"], "renamed-project");

    // ── Delete ────────────────────────────────────────────────────────────
    let delete_resp = client
        .delete(format!("{base}/api/projects/{project_id}"))
        .send()
        .await
        .expect("delete request should succeed");

    assert_eq!(delete_resp.status(), 200);
    let delete_body: serde_json::Value = delete_resp.json().await.expect("valid JSON");
    assert_eq!(delete_body["ok"], true);

    // Verify deletion via GET (should 404)
    let gone_resp = client
        .get(format!("{base}/api/projects/{project_id}"))
        .send()
        .await
        .expect("gone request should succeed");
    assert_eq!(gone_resp.status(), 404, "deleted project should return 404");
}

#[tokio::test]
#[serial]
async fn get_nonexistent_project_returns_404() {
    let env = start_test_gateway().await;
    let base = &env.base;
    let client = reqwest::Client::new();

    let resp = client
        .get(format!("{base}/api/projects/nonexistent-id"))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), 404);
}

#[tokio::test]
#[serial]
async fn update_nonexistent_project_returns_404() {
    let env = start_test_gateway().await;
    let base = &env.base;
    let client = reqwest::Client::new();

    let resp = client
        .put(format!("{base}/api/projects/nonexistent-id"))
        .json(&json!({"name": "new-name"}))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), 404);
}

#[tokio::test]
#[serial]
async fn delete_nonexistent_project_returns_404() {
    let env = start_test_gateway().await;
    let base = &env.base;
    let client = reqwest::Client::new();

    let resp = client
        .delete(format!("{base}/api/projects/nonexistent-id"))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(resp.status(), 404);
}

// ── Confinamento de path (Frente 0b) ────────────────────────────────────────
//
// Antes de 2026-08-29, `CreateProjectRequest.path` era `String` crua do corpo
// JSON e `GET /api/projects/{id}/files` percorria esse diretório
// recursivamente. Como todo o `/api/*` é auth-free por decisão de design, a
// sequência POST {"path":"/etc"} + GET .../files enumerava `/etc` inteiro para
// qualquer um que alcançasse a porta.
//
// Estes testes são o guard dessa correção. Eles falham contra o código
// vulnerável.

/// O ataque original, ponta a ponta: registrar `/etc` e listar.
#[tokio::test]
#[serial]
async fn create_project_rejects_path_outside_allowed_roots() {
    let env = start_test_gateway().await;
    let base = &env.base;
    let client = reqwest::Client::new();

    let evil_paths = ["/etc", "/", "/root", "/proc"];
    for evil in evil_paths {
        let resp = client
            .post(format!("{base}/api/projects"))
            .json(&json!({ "name": "pwn", "path": evil }))
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(
            resp.status(),
            400,
            "POST /api/projects deveria recusar `{evil}`, fora das raizes permitidas"
        );
    }

    // E nada foi registrado — senão o GET .../files ainda teria o que enumerar.
    //
    // Filtra pelo que ESTE teste tentou criar, em vez de exigir a lista global
    // vazia. `PROJECTS` (projects_handler.rs:36) é um `static LazyLock<DashMap>`
    // — estado global do PROCESSO, compartilhado por todos os testes deste
    // binário e alheio a tempdir, porta ou config. Com a asserção de lista
    // vazia, este teste só passava se rodasse antes de
    // `list_project_files_works_inside_the_allowed_root`, que cria um projeto
    // legítimo e não o remove; `#[serial]` serializa os testes mas não fixa a
    // ordem entre eles. Foi assim que o Security Gate (BOLA) quebrou no PR #879.
    //
    // Filtrar pela raiz permitida também não serve: cada teste tem a sua, num
    // tempdir próprio, então um projeto legítimo de outro teste apareceria como
    // fuga. O discriminador correto é o par (nome, caminho) que este teste
    // POSTou — e ele continua falhando se `confine` deixar passar qualquer um.
    let list: serde_json::Value = client
        .get(format!("{base}/api/projects"))
        .send()
        .await
        .expect("list")
        .json()
        .await
        .expect("valid JSON");
    let registrados: Vec<&serde_json::Value> = list["projects"]
        .as_array()
        .expect("array")
        .iter()
        .filter(|p| {
            p["name"].as_str() == Some("pwn")
                || p["path"]
                    .as_str()
                    .is_some_and(|path| evil_paths.contains(&path))
        })
        .collect();
    assert!(
        registrados.is_empty(),
        "nenhum dos caminhos recusados deveria ter sido registrado: {registrados:?}"
    );
}

/// `..` escapando da raiz permitida. `canonicalize` achata antes de comparar.
#[tokio::test]
#[serial]
async fn create_project_rejects_dotdot_traversal() {
    let env = start_test_gateway().await;
    let base = &env.base;
    let client = reqwest::Client::new();

    let escape = format!("{}/../../../../etc", env.root.display());
    let resp = client
        .post(format!("{base}/api/projects"))
        .json(&json!({ "name": "pwn", "path": escape }))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(
        resp.status(),
        400,
        "`..` nao pode escapar da raiz permitida"
    );
}

/// Symlink dentro da raiz apontando para fora: é o motivo de a comparação
/// acontecer depois de `canonicalize`, e não sobre a string crua.
#[cfg(unix)]
#[tokio::test]
#[serial]
async fn create_project_rejects_symlink_escaping_the_root() {
    let env = start_test_gateway().await;
    let base = &env.base;
    let client = reqwest::Client::new();

    let link = env.root.join("fuga");
    std::os::unix::fs::symlink("/etc", &link).expect("symlink");

    let resp = client
        .post(format!("{base}/api/projects"))
        .json(&json!({ "name": "pwn", "path": link.to_str().unwrap() }))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(
        resp.status(),
        400,
        "symlink para fora da raiz deve ser recusado apos resolucao"
    );
}

/// O PUT não pode reabrir o que o POST fechou.
#[tokio::test]
#[serial]
async fn update_project_rejects_path_outside_allowed_roots() {
    let env = start_test_gateway().await;
    let base = &env.base;
    let client = reqwest::Client::new();

    let good = env.root.join("legit");
    std::fs::create_dir_all(&good).expect("create dir");

    let created: serde_json::Value = client
        .post(format!("{base}/api/projects"))
        .json(&json!({ "name": "legit", "path": good.to_str().unwrap() }))
        .send()
        .await
        .expect("create")
        .json()
        .await
        .expect("valid JSON");
    let id = created["project"]["id"].as_str().expect("id").to_string();

    let resp = client
        .put(format!("{base}/api/projects/{id}"))
        .json(&json!({ "path": "/etc" }))
        .send()
        .await
        .expect("request should succeed");
    assert_eq!(resp.status(), 400, "PUT nao pode mover o projeto para /etc");

    // E o path guardado continua o original.
    let after: serde_json::Value = client
        .get(format!("{base}/api/projects/{id}"))
        .send()
        .await
        .expect("get")
        .json()
        .await
        .expect("valid JSON");
    assert_eq!(after["project"]["path"], good.to_str().unwrap());
}

/// `working_dir` **não** é uma superfície viva hoje, e este teste fixa isso.
///
/// `POST /api/sessions` roteia para `api::create_session` (`router.rs:116-118`),
/// cujo `CreateSessionRequest` tem só `agent_id` — o `working_dir` do corpo é
/// descartado pelo serde e nunca vira diretório. O
/// `projects_handler::create_session_with_project`, que aceita `working_dir`,
/// é `pub` mas **não está roteado em lugar nenhum**.
///
/// Ele já foi endurecido junto com o resto (confina o `working_dir` antes de
/// usá-lo), então rotear depois não reabre o buraco. Se alguém trocar a rota
/// para ele, este teste passa a ver um 400 no lugar do 201 e é o sinal de
/// reavaliar o que aqui está escrito.
#[tokio::test]
#[serial]
async fn post_sessions_does_not_expose_working_dir_today() {
    let env = start_test_gateway().await;
    let base = &env.base;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{base}/api/sessions"))
        .json(&json!({ "working_dir": "/etc" }))
        .send()
        .await
        .expect("request should succeed");

    assert_eq!(
        resp.status(),
        201,
        "a rota viva ignora working_dir; um 400 aqui significa que \
         create_session_with_project foi roteado — reavalie o comentario acima"
    );

    let body: serde_json::Value = resp.json().await.expect("valid JSON");
    assert!(
        body.get("working_dir").is_none_or(|v| v.is_null()),
        "a resposta nao deve ecoar um working_dir nao confinado: {body}"
    );
}

/// O caminho feliz continua funcionando: diretório real sob a raiz permitida
/// lista os arquivos que tem.
#[tokio::test]
#[serial]
async fn list_project_files_works_inside_the_allowed_root() {
    let env = start_test_gateway().await;
    let base = &env.base;
    let client = reqwest::Client::new();

    let dir = env.root.join("com-arquivos");
    std::fs::create_dir_all(dir.join("sub")).expect("create dirs");
    std::fs::write(dir.join("raiz.txt"), b"a").expect("write");
    std::fs::write(dir.join("sub/aninhado.txt"), b"b").expect("write");

    let created: serde_json::Value = client
        .post(format!("{base}/api/projects"))
        .json(&json!({ "name": "com-arquivos", "path": dir.to_str().unwrap() }))
        .send()
        .await
        .expect("create")
        .json()
        .await
        .expect("valid JSON");
    let id = created["project"]["id"].as_str().expect("id").to_string();

    let resp = client
        .get(format!("{base}/api/projects/{id}/files"))
        .send()
        .await
        .expect("files request should succeed");
    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.expect("valid JSON");
    let files: Vec<&str> = body["files"]
        .as_array()
        .expect("array")
        .iter()
        .map(|f| f.as_str().expect("string"))
        .collect();

    assert!(
        files.contains(&"raiz.txt"),
        "esperava raiz.txt em {files:?}"
    );
    assert!(
        files.contains(&"sub/aninhado.txt"),
        "esperava sub/aninhado.txt em {files:?}"
    );
}
