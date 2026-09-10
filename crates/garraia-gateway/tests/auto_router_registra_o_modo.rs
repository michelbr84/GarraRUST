//! #1103: o auto-router heurístico registra o modo que deduziu.
//!
//! O contrato travado aqui tem duas metades, e elas só valem juntas:
//!
//! 1. `POST /v1/chat/completions` sem modo explícito grava `agent_mode` no
//!    metadado da sessão, com `agent_mode_source = "auto"` — é o rastro que a
//!    issue pede para poder auditar, depois, em que modo a sessão rodou e que
//!    a heurística rodou.
//! 2. Esse modo **não** satisfaz `get_chosen_agent_mode`. Deduzir não é
//!    consentir (#988): se satisfizesse, quem nunca digitou `/mode` perderia
//!    `file_write` porque a heurística achou que a pergunta parecia busca.
//!
//! Nenhum teste fala com um LLM. O `AgentRuntime` sobe sem provedor, então o
//! turno termina em erro — e não importa: a gravação do modo acontece **antes**
//! da chamada ao modelo, que é justamente a propriedade de observabilidade que
//! a issue quer (o rastro sobrevive a um turno que falha).

use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
use garraia_agents::AgentRuntime;
use garraia_channels::ChannelRegistry;
use garraia_config::AppConfig;
use garraia_db::SessionStore;
use garraia_gateway::openai_api::build_openai_router;
use garraia_gateway::state::AppState;
use serde_json::json;
use tower::ServiceExt; // `oneshot`

/// Serializa os testes deste binário.
///
/// O harness padrão roda os testes de um mesmo binário em paralelo, e
/// `std::env::set_var` (unsafe no Edition 2024) exige que nenhuma outra
/// thread leia ou escreva o ambiente enquanto ele é chamado. Sem este
/// lock, um teste escreveria `GARRAIA_CONFIG_DIR` enquanto o outro o
/// lia dentro do `AppState::new` — corrida de memória, não só de
/// lógica.
///
/// O guard é mantido até o fim do corpo de cada teste, e o runtime
/// `#[tokio::test]` (que desova as tasks do `AppState`) cai quando o
/// corpo termina — antes do guard. Quando o próximo teste adquire o
/// lock, nenhuma thread do anterior sobreviveu para ler o ambiente.
/// `tokio::sync::Mutex`, não `std`: o guard atravessa `.await` (o
/// `clippy::await_holding_lock` proíbe o guard de `std` nessa posição), e
/// o `#[tokio::test` runtime cai dentro do corpo, antes do guard.
static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Aponta o config dir do processo para um temporário vazio.
///
/// Sem isto o boot lê `~/.config/garraia` de quem roda a suíte — allowlist de
/// diretórios, e os servidores MCP que estiverem lá. `set_var` copia o valor,
/// mas o diretório precisa continuar existindo enquanto `AppState::new` o lê:
/// por isso o `TempDir` volta para o teste, que o mantém vivo.
///
/// SAFETY (chamador): o chamador segura `ENV_LOCK` durante TODO o corpo do
/// teste (das duas funções `#[tokio::test]` abaixo, antes de qualquer outra
/// linha). Isso exclui a única fonte de threads concorrentes deste binário —
/// o outro teste — da janela de escrita; as tasks do runtime do próprio
/// teste são criadas depois e não sobrevivem ao guard (o runtime cai dentro
/// do corpo, o guard cai depois).
fn config_dir_de_teste() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp config dir");
    unsafe {
        std::env::set_var("GARRAIA_CONFIG_DIR", dir.path());
        std::env::set_var(
            garraia_gateway::mcp::McpPersistenceService::DISABLE_AUTOPROVISION_ENV,
            "1",
        );
    }
    dir
}

/// Sobe um `AppState` com `SessionStore` em memória.
fn estado_e_store() -> (Arc<AppState>, Arc<tokio::sync::Mutex<SessionStore>>) {
    let mut config = AppConfig::default();
    config.memory.enabled = false;

    let mut state = AppState::new(
        config,
        Arc::new(AgentRuntime::new()),
        ChannelRegistry::new(),
    );
    let store = Arc::new(tokio::sync::Mutex::new(
        SessionStore::in_memory().expect("store em memoria"),
    ));
    state.set_session_store(Arc::clone(&store));
    (Arc::new(state), store)
}

/// Dispara um turno pelo endpoint OpenAI-compatible e devolve o corpo.
///
/// O status não é afirmado: sem provedor configurado o turno falha de
/// propósito, e o que este arquivo afirma acontece antes disso.
async fn post_chat(state: Arc<AppState>, session_id: &str, content: &str) -> String {
    let router = build_openai_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .header("x-session-id", session_id)
        .body(Body::from(
            json!({
                "model": "nao-importa",
                "messages": [{ "role": "user", "content": content }],
            })
            .to_string(),
        ))
        .expect("request builder");

    let resp = router
        .oneshot(req)
        .await
        .expect("o router responde sem fazer bind");
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("corpo legivel");
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Lê o metadado cru da sessão. `get_agent_mode` não expõe a origem, e é a
/// origem — `"auto"` — que distingue dedução de escolha.
fn metadado(store: &SessionStore, session_id: &str) -> serde_json::Value {
    let raw: String = store
        .connection()
        .query_row(
            "SELECT metadata FROM sessions WHERE id = ?1",
            [session_id],
            |row| row.get(0),
        )
        .expect("a linha da sessao existe");
    serde_json::from_str(&raw).expect("metadado e JSON")
}

/// O modo deduzido chega ao banco marcado como `auto`, e não autoriza nada.
#[tokio::test]
async fn modo_deduzido_e_gravado_como_auto_e_nao_liga_politica() {
    // Segura o ENV_LOCK antes de tocar o ambiente (ver doc do static).
    let _serializa = ENV_LOCK.lock().await;
    let _dir = config_dir_de_teste();
    let (state, store) = estado_e_store();

    // Frase que a heurística classifica sem ambiguidade — a mesma coberta em
    // `garraia-agents/src/auto_router.rs`: os três sinais de `debug` abrem a
    // margem mínima sobre a segunda colocada.
    let _ = post_chat(
        Arc::clone(&state),
        "sessao-1103",
        "why does this crash? the server is not working",
    )
    .await;

    let store = store.lock().await;
    let metadata = metadado(&store, "sessao-1103");

    assert_eq!(
        metadata.get("agent_mode").and_then(|v| v.as_str()),
        Some("debug"),
        "o modo deduzido tem de ficar gravado; foi ele que o roteador escolheu"
    );
    assert_eq!(
        metadata.get("agent_mode_source").and_then(|v| v.as_str()),
        Some("auto"),
        "a origem e o que permite auditar que foi deduzido, e nao escolhido"
    );

    // A outra metade do contrato: aparece, mas nao autoriza.
    assert_eq!(
        store
            .get_agent_mode("sessao-1103")
            .expect("leitura do modo"),
        Some("debug".to_string()),
        "o /mode e o GET /api/mode/current mostram o modo deduzido"
    );
    assert_eq!(
        store
            .get_chosen_agent_mode("sessao-1103")
            .expect("leitura do modo escolhido"),
        None,
        "deduzir nao e consentir: nenhuma ToolPolicy liga por isso"
    );
}

/// Heurística em dúvida não inventa modo.
///
/// É o que torna o primeiro teste honesto: sem este, gravar `debug` ou
/// `search` em qualquer mensagem também passaria. A mesma dúvida é a
/// explicação provável do `{}` reportado na issue — uma frase como
/// "Traceback … linha 42" marca um sinal só, abaixo do mínimo, e aí não há
/// mesmo o que registrar.
#[tokio::test]
async fn sem_deducao_nao_ha_registro() {
    // Mesmo contrato do teste de cima: ambiente só sob ENV_LOCK.
    let _serializa = ENV_LOCK.lock().await;
    let _dir = config_dir_de_teste();
    let (state, store) = estado_e_store();

    let _ = post_chat(Arc::clone(&state), "sessao-1103-vazia", "hello").await;

    let store = store.lock().await;
    let metadata = metadado(&store, "sessao-1103-vazia");

    assert_eq!(
        metadata.get("agent_mode"),
        None,
        "nenhum modo foi deduzido, entao nenhum modo e registrado"
    );
    assert_eq!(
        store
            .get_chosen_agent_mode("sessao-1103-vazia")
            .expect("leitura do modo escolhido"),
        None
    );
}
