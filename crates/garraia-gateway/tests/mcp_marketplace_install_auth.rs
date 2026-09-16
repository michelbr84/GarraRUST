//! Gate de autenticacao de `POST /api/mcp/marketplace/install` (#1245).
//!
//! O handler nascia com a assinatura `(State, Json<InstallMcpRequest>)` — sem
//! extractor nenhum de autenticacao — montado direto no grupo aberto do
//! router. Qualquer chamador que alcancasse a porta registrava um servidor MCP
//! no registry, escolhendo pelo CORPO do pedido o `env` e os `extra_args` do
//! processo que o gateway ia executar.
//!
//! Este arquivo e o teste de regressao do gate. Sem a correcao, o primeiro
//! teste aqui recebe `201 CREATED` em vez de `401`.
//!
//! Por que integracao e nao unidade: `require_admin_auth` e `require_csrf`
//! entram por `axum::middleware::from_fn` no sub-router, entao a unica forma
//! honesta de provar "sem cookie → 401" e dirigir o router pelo Tower e olhar
//! o status — o mesmo caminho do trafego de producao. Espelha
//! `plugins_auth_ssrf.rs`, que faz isso para as rotas irmas de `/api/plugins/*`.
//!
//! Cobertura (regra 10 do CLAUDE.md — autorizacao antes de merge em rota
//! alterada):
//!
//! | Chamador                        | Esperado |
//! |---------------------------------|----------|
//! | sem cookie                      | 401      |
//! | cookie invalido                 | 401      |
//! | sessao valida, sem CSRF         | 403      |
//! | `Role::Viewer` (sem permissao)  | 403      |
//! | `Role::Operator`                | 201      |
//! | `Role::Admin`                   | 201      |
//! | `Role::Admin` + `env` bloqueada | 400      |

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use garraia_agents::AgentRuntime;
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_gateway::admin::rbac::Role;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::mcp_marketplace::build_marketplace_install_routes;
use garraia_gateway::state::AppState;
use http_body_util::BodyExt;
use tokio::sync::Mutex;
use tower::ServiceExt;

const CAMINHO: &str = "/api/mcp/marketplace/install";

/// Entrada do catalogo sem `env` obrigatoria — assim um 400 no teste significa
/// "a validacao nova recusou", nunca "faltou variavel exigida pelo catalogo".
const CORPO_OK: &str = r#"{"id":"sqlite"}"#;

/// Sessao do console: cookie + token de CSRF.
struct Sessao {
    token: String,
    csrf: String,
}

/// Sub-router real com um `AdminStore` em memoria que o teste continua
/// enxergando — e assim que ele confere o efeito colateral (o registry).
///
/// O fixture usa `sqlite` e nao `filesystem` porque `AppState::new` chama
/// `provision_filesystem_if_missing()`: o servidor `filesystem` ja nasce no
/// registry, entao "esta registrado" nunca provaria que foi ESTE pedido que o
/// registrou. A precondicao abaixo trava isso — se um dia o boot passar a
/// provisionar `sqlite` tambem, os testes falham alto em vez de virar
/// assertiva vazia.
async fn montar() -> (Router, Arc<AppState>, Arc<Mutex<AdminStore>>) {
    let state = Arc::new(AppState::new(
        AppConfig::default(),
        Arc::new(AgentRuntime::new()),
        ChannelRegistry::new(),
    ));
    assert!(
        state.mcp_registry.get("sqlite").await.is_none(),
        "precondicao do fixture: 'sqlite' nao pode ja estar no registry antes do pedido"
    );
    let store = Arc::new(Mutex::new(
        AdminStore::in_memory().expect("store em memoria"),
    ));
    let router = build_marketplace_install_routes(Arc::clone(&state), Arc::clone(&store));
    (router, state, store)
}

/// Cria um usuario com o papel pedido e abre sessao para ele.
async fn sessao_para(store: &Arc<Mutex<AdminStore>>, usuario: &str, papel: Role) -> Sessao {
    let guard = store.lock().await;
    let user = guard
        .create_user(usuario, "senha-de-teste-bem-longa", papel)
        .expect("criar usuario");
    let session = guard
        .create_session(&user.id, Some("127.0.0.1"), Some("teste"))
        .expect("criar sessao");
    Sessao {
        token: session.token,
        csrf: session.csrf_token,
    }
}

async fn instalar(
    router: Router,
    sessao: Option<&Sessao>,
    com_csrf: bool,
    corpo: &str,
) -> StatusCode {
    let mut req = Request::builder()
        .method("POST")
        .uri(CAMINHO)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(s) = sessao {
        req = req.header(header::COOKIE, format!("garraia_admin_session={}", s.token));
        if com_csrf {
            req = req.header("x-csrf-token", &s.csrf);
        }
    }
    let req = req.body(Body::from(corpo.to_owned())).expect("request");

    let resp = router.oneshot(req).await.expect("oneshot");
    let status = resp.status();
    // Drena o corpo para a conexao fechar limpa mesmo se a assertiva falhar.
    let _ = resp.into_body().collect().await;
    status
}

// ── O gate que faltava ──────────────────────────────────────────────────────

/// **Teste de regressao do #1245.** Sem a correcao isto devolve 201 e o
/// servidor MCP fica registrado por um chamador anonimo.
#[tokio::test]
async fn install_sem_cookie_devolve_401() {
    let (router, state, _store) = montar().await;

    let status = instalar(router, None, false, CORPO_OK).await;

    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "POST {CAMINHO} tem de recusar chamador anonimo"
    );
    // O gate tem de barrar ANTES do handler: nada pode ter sido registrado.
    assert!(
        state.mcp_registry.get("sqlite").await.is_none(),
        "chamador anonimo nao pode registrar servidor MCP no registry"
    );
}

#[tokio::test]
async fn install_com_cookie_invalido_devolve_401() {
    let (router, state, _store) = montar().await;

    let req = Request::builder()
        .method("POST")
        .uri(CAMINHO)
        .header(header::CONTENT_TYPE, "application/json")
        .header(
            header::COOKIE,
            "garraia_admin_session=nao-e-um-token-valido",
        )
        .header("x-csrf-token", "tanto-faz")
        .body(Body::from(CORPO_OK))
        .expect("request");

    let resp = router.oneshot(req).await.expect("oneshot");

    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "o middleware valida a sessao contra o AdminStore, nao so a presenca do cookie"
    );
    assert!(state.mcp_registry.get("sqlite").await.is_none());
}

#[tokio::test]
async fn install_sem_csrf_devolve_403() {
    let (router, state, store) = montar().await;
    let sessao = sessao_para(&store, "admin-sem-csrf", Role::Admin).await;

    let status = instalar(router, Some(&sessao), false, CORPO_OK).await;

    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "sessao valida sem token de CSRF nao pode instalar — e o vetor do \
         navegador do dono rodando pagina de terceiro"
    );
    assert!(state.mcp_registry.get("sqlite").await.is_none());
}

// ── Autorizacao por papel ───────────────────────────────────────────────────

#[tokio::test]
async fn install_como_viewer_devolve_403() {
    let (router, state, store) = montar().await;
    let sessao = sessao_para(&store, "viewer", Role::Viewer).await;

    let status = instalar(router, Some(&sessao), true, CORPO_OK).await;

    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "Role::Viewer nao carrega Permission::ManagePlugins"
    );
    assert!(
        state.mcp_registry.get("sqlite").await.is_none(),
        "403 tem de ser antes de qualquer efeito no registry"
    );
}

#[tokio::test]
async fn install_como_operator_instala() {
    let (router, state, store) = montar().await;
    let sessao = sessao_para(&store, "operator", Role::Operator).await;

    let status = instalar(router, Some(&sessao), true, CORPO_OK).await;

    assert_eq!(
        status,
        StatusCode::CREATED,
        "Role::Operator carrega Permission::ManagePlugins"
    );
    let registrado = state
        .mcp_registry
        .get("sqlite")
        .await
        .expect("servidor registrado");
    assert_eq!(registrado.config.command.as_deref(), Some("npx"));
}

#[tokio::test]
async fn install_como_admin_instala_com_args_e_env_do_corpo() {
    let (router, state, store) = montar().await;
    let sessao = sessao_para(&store, "admin", Role::Admin).await;

    // `extra_args` e `env` continuam sendo recurso legitimo — o #1245 nao
    // removeu a funcionalidade, so exigiu admin autenticado e autorizado.
    let corpo = r#"{"id":"sqlite",
                    "extra_args":["/srv/dados"],
                    "env":{"MCP_LOG":"debug"}}"#;
    let status = instalar(router, Some(&sessao), true, corpo).await;

    assert_eq!(status, StatusCode::CREATED);
    let registrado = state
        .mcp_registry
        .get("sqlite")
        .await
        .expect("servidor registrado");
    assert!(
        registrado.config.args.contains(&"/srv/dados".to_string()),
        "extra_args do corpo tem de chegar ao config: args={:?}",
        registrado.config.args
    );
    assert_eq!(
        registrado.config.env.get("MCP_LOG").map(String::as_str),
        Some("debug")
    );
}

// ── Denylist de env (#1245) ─────────────────────────────────────────────────

/// Mesmo um admin autorizado nao redefine o que decide QUAL binario o filho
/// executa: o comando vem do catalogo, e `PATH`/`LD_PRELOAD`/`NODE_OPTIONS`
/// transformariam a entrada vetada em execucao de codigo arbitrario.
#[tokio::test]
async fn install_recusa_env_que_sequestra_o_processo() {
    for (nome, valor) in [
        ("PATH", "/tmp/atacante"),
        ("LD_PRELOAD", "/tmp/evil.so"),
        ("LD_AUDIT", "/tmp/evil.so"),
        ("DYLD_INSERT_LIBRARIES", "/tmp/evil.dylib"),
        ("NODE_OPTIONS", "--require /tmp/evil.js"),
        // Case-insensitive: no Windows os nomes ja sao, e no Unix recusar a
        // mais e o lado certo para errar.
        ("Ld_Preload", "/tmp/evil.so"),
    ] {
        let (router, state, store) = montar().await;
        let sessao = sessao_para(&store, "admin", Role::Admin).await;
        let corpo = format!(r#"{{"id":"sqlite","env":{{"{nome}":"{valor}"}}}}"#);

        let status = instalar(router, Some(&sessao), true, &corpo).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "env '{nome}' tem de ser recusada mesmo vinda de admin"
        );
        assert!(
            state.mcp_registry.get("sqlite").await.is_none(),
            "env '{nome}' recusada nao pode deixar servidor registrado"
        );
    }
}

// ── Montagem no router de producao ──────────────────────────────────────────

/// O que de fato regrediu no #1245 foi a **montagem**: a rota vivia no grupo
/// aberto do `build_router`, sem camada de auth nenhuma. Os testes acima
/// dirigem o sub-router protegido diretamente, entao continuariam verdes com o
/// `build_router` montando a rota em qualquer outro lugar — eles provam o
/// gate, nao a fiacao.
///
/// Este teste fecha essa brecha indo pelo router de verdade, o mesmo que o
/// `main` monta. Sem cookie de admin o pedido tem de morrer antes do handler.
///
/// Nota sobre o modo de falha que ele cobre: re-adicionar a rota ao grupo
/// aberto *sem tirar o merge* nao passa despercebido, porque `Router::merge`
/// do axum 0.8 faz panic em rota sobreposta (verificado) — o gateway nem sobe.
/// O caso silencioso, e o que este teste existe para pegar, e alguem **mover**
/// a rota de volta para o grupo aberto removendo o merge junto: nada entra em
/// conflito, tudo compila, o binario sobe, e o install volta a ser anonimo.
#[tokio::test]
async fn install_no_router_de_producao_exige_admin() {
    use garraia_gateway::push_channels::PushChannelStates;
    use garraia_gateway::router::build_router;

    let state = Arc::new(AppState::new(
        AppConfig::default(),
        Arc::new(AgentRuntime::new()),
        ChannelRegistry::new(),
    ));
    assert!(state.mcp_registry.get("sqlite").await.is_none());
    let store = Arc::new(Mutex::new(
        AdminStore::in_memory().expect("store em memoria"),
    ));
    let router = build_router(
        Arc::clone(&state),
        PushChannelStates::empty(),
        store,
        Arc::new(vec![0u8; 32]),
    );

    let mut req = Request::builder()
        .method("POST")
        .uri(CAMINHO)
        .header(header::CONTENT_TYPE, "application/json")
        // Same-origin: isola este teste da guarda anti-CSRF generica (#1182),
        // que e outra camada e tem os proprios testes. O que se mede aqui e o
        // gate de admin, nao o de origem.
        .header(header::HOST, "127.0.0.1:3888")
        .header(header::ORIGIN, "http://127.0.0.1:3888")
        .body(Body::from(CORPO_OK))
        .expect("request");
    // O governor le o IP de `ConnectInfo`; sem ele o pedido morre em 500.
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            [127, 0, 0, 1],
            41120,
        ))));

    let resp = router.oneshot(req).await.expect("oneshot");

    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "no router de producao, POST {CAMINHO} sem sessao de admin tem de dar 401 — \
         se der 201, a rota voltou para o grupo aberto e o #1245 regrediu"
    );
    assert!(
        state.mcp_registry.get("sqlite").await.is_none(),
        "nada pode ter sido registrado por um chamador anonimo"
    );
}

/// `npm_config_*` e bloqueada por FAMILIA, nao por nome exato: o npm
/// interpreta qualquer variavel com esse prefixo como chave de config, e todo
/// comando do catalogo e `npx -y`. `npm_config_registry` apontando para um
/// servidor do atacante faz o `npx` baixar e executar o pacote dele.
#[tokio::test]
async fn install_recusa_familia_npm_config() {
    for nome in [
        "npm_config_registry",
        // O npm normaliza a caixa, entao a maiuscula sequestra igual.
        "NPM_CONFIG_REGISTRY",
        "Npm_Config_Script_Shell",
        "npm_config_node_options",
    ] {
        let (router, state, store) = montar().await;
        let sessao = sessao_para(&store, "admin", Role::Admin).await;
        let corpo = format!(r#"{{"id":"sqlite","env":{{"{nome}":"http://atacante.invalid/"}}}}"#);

        let status = instalar(router, Some(&sessao), true, &corpo).await;

        assert_eq!(
            status,
            StatusCode::BAD_REQUEST,
            "env '{nome}' tem de ser recusada pela familia npm_config_*"
        );
        assert!(
            state.mcp_registry.get("sqlite").await.is_none(),
            "env '{nome}' recusada nao pode deixar servidor registrado"
        );
    }
}

/// O prefixo termina em `_`, entao um nome que apenas COMECA parecido nao e
/// varrido junto. Trava a familia no lugar certo: `npm_config_` e config do
/// npm, `npm_configuracao` e so uma variavel qualquer do servidor.
#[tokio::test]
async fn install_nao_bloqueia_nome_parecido_com_npm_config() {
    let (router, state, store) = montar().await;
    let sessao = sessao_para(&store, "admin", Role::Admin).await;

    let corpo = r#"{"id":"sqlite","env":{"npm_configuracao_do_servidor":"ok"}}"#;
    let status = instalar(router, Some(&sessao), true, corpo).await;

    assert_eq!(
        status,
        StatusCode::CREATED,
        "'npm_configuracao_do_servidor' nao e chave de config do npm e nao pode cair na familia"
    );
    assert!(state.mcp_registry.get("sqlite").await.is_some());
}

/// A denylist e estreita de proposito: credencial que o catalogo exige passa.
#[tokio::test]
async fn install_aceita_env_de_credencial_do_catalogo() {
    let (router, state, store) = montar().await;
    let sessao = sessao_para(&store, "admin", Role::Admin).await;

    let corpo = r#"{"id":"github","env":{"GITHUB_PERSONAL_ACCESS_TOKEN":"ghp_exemplo"}}"#;
    let status = instalar(router, Some(&sessao), true, corpo).await;

    assert_eq!(
        status,
        StatusCode::CREATED,
        "a denylist cobre sequestro de processo, nao 'variavel que parece sensivel'"
    );
    assert!(state.mcp_registry.get("github").await.is_some());
}
