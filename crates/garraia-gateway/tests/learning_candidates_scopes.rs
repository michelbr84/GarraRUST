//! #1903: o endpoint de candidatos do gateway enxerga o que a CLI minerou.
//!
//! O minerador (`garra skills mine`) grava em `~/.garra/skills/_candidates`,
//! enquanto o handler listava apenas `.garra/skills/_candidates` do projeto —
//! um candidato gerado pela CLI era invisivel no Web Console. Este teste monta
//! os dois escopos no MESMO filesystem, com o candidato global produzido pelo
//! minerador de verdade (mesma arvore de diretorios da CLI), e chama a rota
//! HTTP do router real.
//!
//! Processo unico e um unico teste: `HOME` e o diretorio de trabalho sao
//! trocados no ambiente deste binario de teste, que nao compartilha processo
//! com nenhuma outra suite.

use std::sync::Arc;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use garraia_agents::AgentRuntime;
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_gateway::admin::store::AdminStore;
use garraia_gateway::push_channels::PushChannelStates;
use garraia_gateway::router::build_router;
use garraia_learning::miner::{self, MineOptions};
use tokio::sync::Mutex;
use tower::ServiceExt;

mod config_dir_descartavel;

#[tokio::test]
async fn candidatos_do_gateway_incluem_o_escopo_global_da_cli() {
    let raiz = tempfile::tempdir().expect("tempdir");
    let home = raiz.path().join("home");
    let projeto = raiz.path().join("projeto");
    std::fs::create_dir_all(home.join(".garra/sessions")).unwrap();
    std::fs::create_dir_all(projeto.join(".garra/skills/_candidates")).unwrap();

    // Duas sessoes no formato SessionRecord, mineradas pelo MESMO caminho da
    // CLI (`Dirs::with_global`: base = pai de `.garra/skills`).
    for id in ["a", "b"] {
        let corpo = format!(
            r#"{{"session_id":"{id}","intent":"release","task_family":"release","commands":[{{"cmd":"git status --porcelain","exit_code":0}},{{"cmd":"cargo test -p garraia --lib","exit_code":0}}],"created_at":"2026-11-15T00:00:00Z"}}"#
        );
        std::fs::write(
            home.join(".garra/sessions").join(format!("{id}.json")),
            corpo,
        )
        .unwrap();
    }
    let opcoes = MineOptions {
        sessions_dir: home.join(".garra/sessions"),
        candidates_dir: home.join(".garra/skills/_candidates"),
        threshold: 2,
    };
    let padroes = miner::mine(&opcoes).expect("mineracao");
    assert_eq!(padroes.len(), 1, "um padrao repetido = um candidato global");

    // Candidato do escopo de projeto, ao lado do global.
    std::fs::write(
        projeto
            .join(".garra/skills/_candidates")
            .join("mined-projeto-abc12345.md"),
        "## Commands\n\n```\ncargo fmt\n```\n",
    )
    .unwrap();

    // O handler resolve HOME e o escopo de projeto do processo.
    // SAFETY: este binario de teste tem um unico teste, entao nada roda em
    // paralelo dentro do processo; a troca fica isolada das outras suites
    // (cada arquivo de teste e um processo separado).
    unsafe {
        std::env::set_var("HOME", &home);
    }
    std::env::set_current_dir(&projeto).expect("cwd do projeto");

    let config = AppConfig::default();
    let state = Arc::new(config_dir_descartavel::app_state(
        config,
        Arc::new(AgentRuntime::new()),
        ChannelRegistry::new(),
    ));
    let admin_store = Arc::new(Mutex::new(
        AdminStore::in_memory().expect("in-memory admin store"),
    ));
    let router = build_router(
        state,
        PushChannelStates::empty(),
        admin_store,
        Arc::new(vec![0u8; 32]),
    );

    let mut req = Request::builder()
        .method("GET")
        .uri("/api/learning/logs/candidates")
        .body(Body::empty())
        .expect("request");
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            [127, 0, 0, 1],
            40405,
        ))));
    let resp = router.oneshot(req).await.expect("resposta");
    assert_eq!(resp.status(), StatusCode::OK);
    let corpo = to_bytes(resp.into_body(), usize::MAX).await.expect("corpo");
    let parsed: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&corpo)).expect("json");

    let itens = parsed["candidates"]
        .as_array()
        .expect("lista de candidatos");
    let escopos: Vec<&str> = itens
        .iter()
        .map(|c| c["scope"].as_str().expect("escopo de cada candidato"))
        .collect();
    assert!(
        escopos.contains(&"global"),
        "o candidato minerado pela CLI (escopo global) desapareceu: {escopos:?}"
    );
    assert!(
        escopos.contains(&"project"),
        "o candidato de projeto precisa continuar aparecendo: {escopos:?}"
    );
    assert_eq!(
        itens.len(),
        2,
        "escopos isolados, sem duplicacao: {escopos:?}"
    );

    // Isolamento: o candidato global nao pode vir rotulado como projeto.
    for item in itens {
        let slug = item["slug"].as_str().unwrap();
        let eh_do_projeto = slug.contains("projeto");
        let escopo = item["scope"].as_str().unwrap();
        assert_eq!(
            eh_do_projeto,
            escopo == "project",
            "candidato {slug} rotulado como {escopo}"
        );
    }
}
