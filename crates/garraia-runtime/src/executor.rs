use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

use garraia_tools::{execute_with_timeout, ToolContext, ToolError, ToolInput, ToolRegistry};

use crate::meta_controller::MetaController;
use crate::state::TaskState;

/// Configurações do runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeSettings {
    /// Timeout em segundos para cada execução de ferramenta.
    pub tool_timeout_secs: u64,

    /// Máximo de chamadas de ferramenta por turno.
    pub max_tool_calls_per_turn: u32,

    /// Máximo de iterações do loop LLM por turno.
    pub max_llm_iterations: u32,
}

impl Default for RuntimeSettings {
    fn default() -> Self {
        Self {
            tool_timeout_secs: 30,
            max_tool_calls_per_turn: 4,
            max_llm_iterations: 8,
        }
    }
}

/// Erros do runtime.
#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("erro de ferramenta: {0}")]
    Tool(#[from] ToolError),

    #[error("runtime falhou: {0}")]
    Failed(String),

    #[error("transição de estado inválida: {from} -> {to}")]
    InvalidTransition { from: String, to: String },
}

/// Ação decidida pelo agente/LLM.
#[derive(Debug, Clone)]
pub enum Action {
    /// Resposta final ao usuário.
    Reply(String),

    /// Chamar uma ferramenta.
    CallTool {
        name: String,
        payload: serde_json::Value,
    },

    /// Aguardar confirmação do usuário.
    Wait(String),
}

/// Resultado de um turno de execução.
#[derive(Debug)]
pub struct TurnResult {
    /// Texto de saída (resposta ou mensagem de espera).
    pub output: String,

    /// Estado final do turno.
    pub final_state: TaskState,

    /// Quantas ferramentas foram chamadas.
    pub tools_called: u32,

    /// Quantas iterações do loop foram executadas.
    pub iterations: u32,
}

/// Executa um turno completo do agente com state machine explícita.
///
/// Fluxo: Planning → Executing → (ToolUse ↔ Executing)* → Completed/Waiting/Failed
pub async fn run_turn(
    settings: &RuntimeSettings,
    registry: &ToolRegistry,
    ctx: &ToolContext,
    decide_action: impl Fn(&str, &[String]) -> Action,
    user_text: &str,
) -> Result<TurnResult, RuntimeError> {
    let mut state = TaskState::Planning;
    let mut meta = MetaController::new();
    let mut tool_results: Vec<String> = Vec::new();
    let mut iterations = 0u32;

    // === PLANNING ===
    // TODO: carregar memória, contexto, histórico
    transition(&mut state, TaskState::Executing)?;

    // === LOOP PRINCIPAL ===
    loop {
        iterations += 1;

        if iterations > settings.max_llm_iterations {
            transition(&mut state, TaskState::Failed)?;
            return Err(RuntimeError::Failed(format!(
                "limite de {} iterações excedido",
                settings.max_llm_iterations
            )));
        }

        // Chamar o agente/LLM para decidir ação
        let action = decide_action(user_text, &tool_results);

        match action {
            Action::Reply(text) => {
                transition(&mut state, TaskState::Completed)?;
                return Ok(TurnResult {
                    output: text,
                    final_state: TaskState::Completed,
                    tools_called: meta.calls_made(),
                    iterations,
                });
            }

            Action::CallTool { name, payload } => {
                transition(&mut state, TaskState::ToolUse)?;

                // Verificar budget
                if !meta.can_call_tool(settings.max_tool_calls_per_turn) {
                    transition(&mut state, TaskState::Waiting)?;
                    return Ok(TurnResult {
                        output: format!(
                            "Cheguei no limite de {} ferramentas neste turno. Quer que eu continue?",
                            settings.max_tool_calls_per_turn
                        ),
                        final_state: TaskState::Waiting,
                        tools_called: meta.calls_made(),
                        iterations,
                    });
                }

                // Registrar e verificar loop
                meta.record_tool_call(&name);
                if meta.detect_loop() {
                    transition(&mut state, TaskState::Failed)?;
                    return Err(RuntimeError::Failed(format!(
                        "loop detectado: ferramenta '{}' chamada repetidamente",
                        name
                    )));
                }

                // Buscar ferramenta
                let tool = registry.get(&name).ok_or_else(|| {
                    RuntimeError::Failed(format!("ferramenta '{}' não registrada", name))
                })?;

                // Executar com timeout
                let input = ToolInput {
                    name: name.clone(),
                    payload,
                };
                let timeout = Duration::from_secs(settings.tool_timeout_secs);

                let tool_output = execute_with_timeout(tool, ctx, input, timeout).await?;

                // Guardar resultado e voltar para Executing
                tool_results.push(serde_json::to_string(&tool_output.payload).unwrap_or_default());
                transition(&mut state, TaskState::Executing)?;
                continue;
            }

            Action::Wait(msg) => {
                transition(&mut state, TaskState::Waiting)?;
                return Ok(TurnResult {
                    output: msg,
                    final_state: TaskState::Waiting,
                    tools_called: meta.calls_made(),
                    iterations,
                });
            }
        }
    }
}

/// Transição de estado com validação.
fn transition(current: &mut TaskState, target: TaskState) -> Result<(), RuntimeError> {
    if current.can_transition_to(target) {
        *current = target;
        Ok(())
    } else {
        Err(RuntimeError::InvalidTransition {
            from: format!("{}", current),
            to: format!("{}", target),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use garraia_tools::{ToolContext, ToolOutput};

    fn criar_settings() -> RuntimeSettings {
        RuntimeSettings {
            tool_timeout_secs: 5,
            max_tool_calls_per_turn: 4,
            max_llm_iterations: 8,
        }
    }

    fn criar_contexto() -> ToolContext {
        ToolContext {
            request_id: "test-001".into(),
        }
    }

    #[tokio::test]
    async fn resposta_direta_completa() {
        let settings = criar_settings();
        let registry = ToolRegistry::new();
        let ctx = criar_contexto();

        let result = run_turn(
            &settings,
            &registry,
            &ctx,
            |text, _| Action::Reply(format!("Echo: {}", text)),
            "olá",
        )
        .await
        .unwrap();

        assert_eq!(result.output, "Echo: olá");
        assert_eq!(result.final_state, TaskState::Completed);
        assert_eq!(result.tools_called, 0);
        assert_eq!(result.iterations, 1);
    }

    #[tokio::test]
    async fn aguardar_retorna_waiting() {
        let settings = criar_settings();
        let registry = ToolRegistry::new();
        let ctx = criar_contexto();

        let result = run_turn(
            &settings,
            &registry,
            &ctx,
            |_, _| Action::Wait("Aguardando confirmação...".into()),
            "teste",
        )
        .await
        .unwrap();

        assert_eq!(result.final_state, TaskState::Waiting);
    }

    #[tokio::test]
    async fn ferramenta_nao_registrada_falha() {
        let settings = criar_settings();
        let registry = ToolRegistry::new(); // vazio
        let ctx = criar_contexto();

        let result = run_turn(
            &settings,
            &registry,
            &ctx,
            |_, _| Action::CallTool {
                name: "inexistente".into(),
                payload: serde_json::json!({}),
            },
            "teste",
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn estado_concluido_apos_resposta() {
        let settings = criar_settings();
        let registry = ToolRegistry::new();
        let ctx = criar_contexto();

        let result = run_turn(
            &settings,
            &registry,
            &ctx,
            |_, _| Action::Reply("pronto".into()),
            "olá",
        )
        .await
        .unwrap();

        assert_eq!(result.final_state, TaskState::Completed);
    }
}
