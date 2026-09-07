use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use garraia_agents::a2a::{
    A2AArtifact, A2AMessage, A2APart, A2ATask, AgentCapabilities, AgentCard, AgentSkill,
    CreateTaskRequest, TaskStatus,
};
use tracing::{info, warn};

use garraia_agents::exec_context::ExecContext;

use crate::state::SharedState;

/// GET /.well-known/agent.json — serve the agent card.
pub async fn agent_card(State(state): State<SharedState>) -> impl IntoResponse {
    let config = state.current_config();

    let skills: Vec<AgentSkill> = if !config.agents.is_empty() {
        config
            .agents
            .keys()
            .map(|name| AgentSkill {
                id: name.clone(),
                name: name.clone(),
                description: config
                    .agents
                    .get(name)
                    .and_then(|a| a.system_prompt.clone()),
                tags: vec![],
            })
            .collect()
    } else {
        vec![AgentSkill {
            id: "default".to_string(),
            name: "default".to_string(),
            description: config.agent.system_prompt.clone(),
            tags: vec![],
        }]
    };

    let card = AgentCard {
        name: "garraia".to_string(),
        description: Some("GarraIA AI agent".to_string()),
        url: format!("http://{}:{}", config.gateway.host, config.gateway.port),
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
        capabilities: AgentCapabilities {
            streaming: false,
            push_notifications: false,
        },
        skills,
    };

    Json(card)
}

/// O que o `target` de um pedido A2A resolve (#965).
///
/// Existe como enum e nao como `Option` porque sao **tres** casos, e juntar
/// dois deles e o bug: "nao pediu agente" e "pediu um que nao existe" levam a
/// respostas opostas — a primeira e sucesso com o agente padrao, a segunda e
/// 400. Um `Option<&NamedAgentConfig>` colapsaria as duas em `None`, que e
/// exatamente o que o `agent_router::resolve` faz e por que ele nao serve
/// aqui.
#[derive(Debug)]
pub(crate) enum Alvo<'a> {
    /// Sem `target`: o agente padrao atende, como sempre atendeu.
    Padrao,
    /// `target` que existe na config.
    Nomeado(&'a garraia_config::NamedAgentConfig),
    /// `target` que nao existe. Carrega os nomes conhecidos para a resposta.
    Desconhecido { conhecidos: Vec<String> },
}

/// Decide o alvo de um pedido A2A. Funcao pura — o handler so age sobre ela.
///
/// Toda a decisao mora aqui para que ela seja testavel sem subir servidor,
/// sem Postgres e sem provider: os testes de integracao do gateway que sobem
/// processo estao todos `#[ignore]`d por dependencia de infra, e um teste que
/// nao roda nao prova nada.
pub(crate) fn resolver_alvo<'a>(
    config: &'a garraia_config::AppConfig,
    target: Option<&str>,
) -> Alvo<'a> {
    match target {
        None => Alvo::Padrao,
        Some(nome) => match crate::agent_router::resolve_exact(config, nome) {
            Some(a) => Alvo::Nomeado(a),
            None => {
                let mut conhecidos: Vec<String> = config.agents.keys().cloned().collect();
                conhecidos.sort_unstable();
                Alvo::Desconhecido { conhecidos }
            }
        },
    }
}

/// POST /a2a/tasks — create a new task.
pub async fn create_task(
    State(state): State<SharedState>,
    Json(body): Json<CreateTaskRequest>,
) -> impl IntoResponse {
    let task_id = body.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Extract text from message parts
    let user_text: String = body
        .message
        .parts
        .iter()
        .filter_map(|p| match p {
            A2APart::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    if user_text.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "message must contain at least one text part" })),
        )
            .into_response();
    }

    // O agente-alvo (#965), resolvido **antes** de a tarefa ser criada.
    //
    // Recusar cedo importa: um `target` desconhecido que so falhasse depois
    // deixaria uma tarefa `working` no mapa para sempre, e o chamador leria o
    // 400 como se a tarefa nao existisse — quando ela existe, orfa.
    let config = state.current_config();
    let agente = match resolver_alvo(&config, body.target.as_deref()) {
        Alvo::Padrao => None,
        Alvo::Nomeado(a) => Some(a),
        // **Nunca** cair no padrao. O `resolve` normal cairia, e quem pediu a
        // Hera receberia a Garra com 200 e sem sinal nenhum — acreditar numa
        // restricao que nao existe e pior que nao ter restricao.
        Alvo::Desconhecido { conhecidos } => {
            // O nome pedido **nao** entra no log: ele vem de um endpoint sem
            // autenticacao, e ecoar entrada de fora para o arquivo de log e o
            // caminho curto para poluicao e injecao de linha.
            warn!("A2A task rejected: unknown target agent");
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "unknown target agent",
                    // Listar os nomes nao vaza nada: o
                    // `GET /.well-known/agent.json` ja publica todos como
                    // `skills`, e sem a lista o chamador so pode adivinhar.
                    "known": conhecidos,
                })),
            )
                .into_response();
        }
    };

    // Create an initial task in "working" status
    let task = A2ATask {
        id: task_id.clone(),
        status: TaskStatus::Working,
        messages: vec![body.message],
        artifacts: vec![],
        metadata: body.metadata,
    };

    // Store the task
    state.a2a_tasks.insert(task_id.clone(), task);

    info!("A2A task created: {task_id}");

    // Process through agent runtime
    let session_id = format!("a2a:{task_id}");
    state
        .hydrate_session_history(&session_id, Some("a2a"), None)
        .await;
    let history = state.session_history(&session_id);
    let continuity_key = state.continuity_key(None);

    // Sem `ExecContext` de modo, e de proposito.
    //
    // A sessao aqui e `a2a:{task_id}` — nasce nesta funcao e morre com a
    // tarefa, entao nao existe usuario que tenha escolhido modo para ela; ler
    // o store devolveria `None` sempre. O que restringe uma tarefa A2A sao os
    // guardrails da propria superficie (#929), nao o `/mode` de alguem.
    //
    // Se um dia a tarefa A2A passar a herdar a sessao de quem a pediu, este e
    // o ponto onde o modo escolhido precisa entrar.
    let result = match agente {
        // Com agente nomeado, o provider, o prompt e o teto de tokens sao os
        // dele — e o mesmo caminho que o `POST /api/chat` ja usa para
        // `agent_id`.
        Some(ac) => {
            state
                .agents
                .process_message_with_agent_config(
                    &session_id,
                    &user_text,
                    &history,
                    continuity_key.as_deref(),
                    None,
                    ac.provider.as_deref(),
                    None,
                    ac.system_prompt.as_deref(),
                    ac.max_tokens,
                    &ExecContext::default(),
                )
                .await
        }
        None => {
            state
                .agents
                .process_message_with_context(
                    &session_id,
                    &user_text,
                    &history,
                    continuity_key.as_deref(),
                    None,
                )
                .await
        }
    };

    match result {
        Ok(response_text) => {
            state
                .persist_turn(&session_id, Some("a2a"), None, &user_text, &response_text)
                .await;

            // Update task to completed with artifact
            if let Some(mut task) = state.a2a_tasks.get_mut(&task_id) {
                task.status = TaskStatus::Completed;
                let response_parts = vec![A2APart::Text {
                    text: response_text,
                }];
                task.messages.push(A2AMessage {
                    role: "agent".to_string(),
                    parts: response_parts.clone(),
                });
                task.artifacts.push(A2AArtifact {
                    name: Some("response".to_string()),
                    parts: response_parts,
                    index: Some(0),
                });
            }

            match state.a2a_tasks.get(&task_id) {
                Some(task) => {
                    (StatusCode::OK, Json(serde_json::json!(task.clone()))).into_response()
                }
                None => {
                    warn!("A2A task {task_id} vanished after completion");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": "task state lost" })),
                    )
                        .into_response()
                }
            }
        }
        Err(e) => {
            warn!("A2A task {task_id} failed: {e}");
            if let Some(mut task) = state.a2a_tasks.get_mut(&task_id) {
                task.status = TaskStatus::Failed;
            }
            match state.a2a_tasks.get(&task_id) {
                Some(task) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!(task.clone())),
                )
                    .into_response(),
                None => {
                    warn!("A2A task {task_id} vanished after failure");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({ "error": format!("task failed: {e}") })),
                    )
                        .into_response()
                }
            }
        }
    }
}

/// GET /a2a/tasks/:id — get task status.
pub async fn get_task(
    State(state): State<SharedState>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    match state.a2a_tasks.get(&task_id) {
        Some(task) => (StatusCode::OK, Json(serde_json::json!(task.clone()))).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "task not found" })),
        )
            .into_response(),
    }
}

/// POST /a2a/tasks/:id/cancel — cancel a task.
pub async fn cancel_task(
    State(state): State<SharedState>,
    Path(task_id): Path<String>,
) -> impl IntoResponse {
    match state.a2a_tasks.get_mut(&task_id) {
        Some(mut task) => {
            if task.status == TaskStatus::Completed || task.status == TaskStatus::Failed {
                return (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({ "error": "task already finished" })),
                )
                    .into_response();
            }
            task.status = TaskStatus::Canceled;
            info!("A2A task canceled: {task_id}");
            let task = task.clone();
            (StatusCode::OK, Json(serde_json::json!(task))).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "task not found" })),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_agents::a2a::CreateTaskRequest;
    use garraia_config::{AppConfig, NamedAgentConfig};

    fn agente(prompt: &str) -> NamedAgentConfig {
        NamedAgentConfig {
            provider: None,
            model: None,
            system_prompt: Some(prompt.to_string()),
            max_tokens: None,
            max_context_tokens: None,
            tools: vec![],
        }
    }

    fn config_com_hera() -> AppConfig {
        let mut c = AppConfig::default();
        c.agents
            .insert("hera".to_string(), agente("Eu sou a Hera."));
        c.agents
            .insert("default".to_string(), agente("Eu sou a Garra."));
        c
    }

    /// Sem `target`, nada muda para quem ja usava o endpoint.
    #[test]
    fn sem_target_o_padrao_atende() {
        let c = config_com_hera();
        assert!(matches!(resolver_alvo(&c, None), Alvo::Padrao));
    }

    /// `target` que existe leva ao agente pedido — e ao **prompt** dele.
    #[test]
    fn target_conhecido_leva_ao_agente_pedido() {
        let c = config_com_hera();
        match resolver_alvo(&c, Some("hera")) {
            Alvo::Nomeado(a) => {
                assert_eq!(a.system_prompt.as_deref(), Some("Eu sou a Hera."))
            }
            outro => panic!("devia ser Nomeado: {outro:?}"),
        }
    }

    /// **O ponto da issue: desconhecido nao vira o padrao em silencio.**
    ///
    /// Se este teste falhar porque alguem trocou o `resolve_exact` pelo
    /// `resolve`, o sintoma em producao e um 200 com a resposta de outro
    /// agente — e o chamador nao tem como saber.
    #[test]
    fn target_desconhecido_nao_vira_o_padrao() {
        let c = config_com_hera();
        match resolver_alvo(&c, Some("forja")) {
            Alvo::Desconhecido { conhecidos } => {
                assert_eq!(conhecidos, vec!["default".to_string(), "hera".to_string()]);
            }
            outro => panic!("devia ser Desconhecido: {outro:?}"),
        }
    }

    /// Sem agente nomeado nenhum configurado, qualquer `target` e desconhecido
    /// — inclusive `"default"`, que ai nao existe como agente **nomeado**.
    #[test]
    fn sem_agentes_configurados_todo_target_e_desconhecido() {
        let c = AppConfig::default();
        assert!(matches!(
            resolver_alvo(&c, Some("default")),
            Alvo::Desconhecido { .. }
        ));
        // E sem `target` continua funcionando: e o caminho legado.
        assert!(matches!(resolver_alvo(&c, None), Alvo::Padrao));
    }

    /// O corpo aceita `target`, `agentId` e `agent_id` — a issue cita os dois
    /// nomes, e nao ha razao para escolher por quem chama.
    #[test]
    fn o_corpo_aceita_as_tres_grafias() {
        for chave in ["target", "agentId", "agent_id"] {
            let json = serde_json::json!({
                "message": { "role": "user", "parts": [{ "type": "text", "text": "oi" }] },
                chave: "hera",
            });
            let req: CreateTaskRequest =
                serde_json::from_value(json).unwrap_or_else(|e| panic!("{chave}: {e}"));
            assert_eq!(req.target.as_deref(), Some("hera"), "grafia {chave}");
        }
    }

    /// E um corpo sem `target` continua desserializando — o campo e opcional,
    /// entao nenhum cliente A2A existente quebra.
    #[test]
    fn corpo_sem_target_continua_valido() {
        let json = serde_json::json!({
            "message": { "role": "user", "parts": [{ "type": "text", "text": "oi" }] }
        });
        let req: CreateTaskRequest = serde_json::from_value(json).expect("desserializa");
        assert!(req.target.is_none());
    }
}
