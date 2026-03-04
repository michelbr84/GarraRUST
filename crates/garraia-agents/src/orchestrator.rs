//! # Orchestrator Mode (Agent Executor Multi-Step)
//!
//! Este módulo implementa o modo "Orchestrator" que coordena tarefas multi-step
//! com loops e validações conforme especificação M7-1 do roadmap.
//!
//! ## Funcionalidades:
//! - Planejamento: Gera lista de etapas (steps) para completar tarefa
//! - Execução sequencial: Executa cada step com tools
//! - Validação: Verifica se resultado do step é válido
//! - Retry: Tenta novamente se falhar (com limite)
//! - Resumo: Retorna sumário do que foi feito

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn, error};

use crate::providers::{
    ChatMessage, ChatRole, ContentBlock, LlmProvider, LlmRequest, MessagePart,
};
use crate::tools::{Tool, ToolContext};

/// Configurações de limites do Orchestrator
#[derive(Debug, Clone)]
pub struct OrchestratorLimits {
    /// Máximo de iterações (loops)
    pub max_loops: u32,
    /// Timeout por step em segundos
    pub timeout_secs: u64,
    /// Tentativas em caso de falha
    pub retry_count: u32,
}

impl Default for OrchestratorLimits {
    fn default() -> Self {
        Self {
            max_loops: 10,
            timeout_secs: 30,
            retry_count: 2,
        }
    }
}

impl OrchestratorLimits {
    pub fn new(max_loops: u32, timeout_secs: u64, retry_count: u32) -> Self {
        Self {
            max_loops,
            timeout_secs,
            retry_count,
        }
    }
}

/// Status de um step durante a execução
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StepStatus {
    /// Step pendente
    Pending,
    /// Step em execução
    Running,
    /// Step completado com sucesso
    Completed,
    /// Step falhou
    Failed,
    /// Step em retry
    Retrying,
    /// Step validado (pode continuar)
    Validated,
}

impl Default for StepStatus {
    fn default() -> Self {
        StepStatus::Pending
    }
}

/// Uma única etapa do plano de execução
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorStep {
    /// Identificador único do step
    pub id: String,
    /// Descrição do que o step faz
    pub description: String,
    /// Ferramenta a ser usada (bash, file_read, file_write, etc)
    pub tool_name: String,
    /// Parâmetros para a ferramenta
    pub tool_input: serde_json::Value,
    /// Condição para executar este step (opcional)
    pub condition: Option<String>,
    /// Validação esperada após execução (opcional)
    pub validation: Option<String>,
    /// Status atual do step
    #[serde(default)]
    pub status: StepStatus,
    /// Resultado da execução (se completado)
    pub result: Option<String>,
    /// Número de tentativas realizadas
    #[serde(default)]
    pub attempts: u32,
    /// Erro se falhou
    pub error: Option<String>,
}

impl OrchestratorStep {
    pub fn new(id: u32, description: &str, tool_name: &str, tool_input: serde_json::Value) -> Self {
        Self {
            id: format!("step_{}", id),
            description: description.to_string(),
            tool_name: tool_name.to_string(),
            tool_input,
            condition: None,
            validation: None,
            status: StepStatus::Pending,
            result: None,
            attempts: 0,
            error: None,
        }
    }

    pub fn with_condition(mut self, condition: &str) -> Self {
        self.condition = Some(condition.to_string());
        self
    }

    pub fn with_validation(mut self, validation: &str) -> Self {
        self.validation = Some(validation.to_string());
        self
    }
}

/// Plano de execução gerado pelo orchestrator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorPlan {
    /// Tarefa original do usuário
    pub task: String,
    /// Lista de steps a executar
    pub steps: Vec<OrchestratorStep>,
    /// Step atual sendo executado
    #[serde(default)]
    pub current_step: usize,
    /// Se o plano foi completado
    #[serde(default)]
    pub completed: bool,
    /// Resumo final (preenchido após execução)
    pub summary: Option<String>,
}

impl OrchestratorPlan {
    pub fn new(task: &str) -> Self {
        Self {
            task: task.to_string(),
            steps: Vec::new(),
            current_step: 0,
            completed: false,
            summary: None,
        }
    }

    pub fn add_step(&mut self, step: OrchestratorStep) {
        self.steps.push(step);
    }

    /// Retorna o próximo step pendente
    pub fn next_pending_step(&self) -> Option<&OrchestratorStep> {
        self.steps.iter().find(|s| s.status == StepStatus::Pending)
    }

    /// Retorna o número de steps pendentes
    pub fn pending_count(&self) -> usize {
        self.steps.iter().filter(|s| s.status == StepStatus::Pending).count()
    }

    /// Retorna o número de steps completados
    pub fn completed_count(&self) -> usize {
        self.steps.iter().filter(|s| s.status == StepStatus::Completed).count()
    }
}

/// Resultado da validação de um step
#[derive(Debug, Clone)]
pub struct ValidationResult {
    /// Se a validação passou
    pub passed: bool,
    /// Mensagem de feedback
    pub message: String,
    /// Se deve fazer retry
    pub should_retry: bool,
}

/// Resumo da execução do orchestrator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorSummary {
    /// Total de steps
    pub total_steps: usize,
    /// Steps completados com sucesso
    pub successful_steps: usize,
    /// Steps que falharam
    pub failed_steps: usize,
    /// Steps que precisaram de retry
    pub retried_steps: usize,
    /// Tempo total estimado (em segundos)
    pub total_time_secs: u64,
    /// Resumo textual
    pub summary_text: String,
    /// Detalhes por step
    pub step_details: Vec<StepDetail>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepDetail {
    pub id: String,
    pub description: String,
    pub status: String,
    pub attempts: u32,
    pub result_preview: Option<String>,
    pub error: Option<String>,
}

/// Orchestrator - Executor multi-step
pub struct Orchestrator {
    /// Tools disponíveis para execução
    tools: Vec<Box<dyn Tool>>,
    /// Limites de execução
    limits: OrchestratorLimits,
    /// Histórico de execução
    execution_history: Vec<OrchestratorPlan>,
}

impl Orchestrator {
    pub fn new() -> Self {
        Self {
            tools: Vec::new(),
            limits: OrchestratorLimits::default(),
            execution_history: Vec::new(),
        }
    }

    pub fn with_limits(mut self, limits: OrchestratorLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Registra uma ferramenta
    pub fn register_tool(&mut self, tool: Box<dyn Tool>) {
        info!("orchestrator registered tool: {}", tool.name());
        self.tools.push(tool);
    }

    /// Encontra uma ferramenta pelo nome
    fn find_tool(&self, name: &str) -> Option<&dyn Tool> {
        self.tools
            .iter()
            .find(|t| t.name() == name)
            .map(|t| t.as_ref())
    }

    /// Gera um plano de execução a partir do task usando LLM
    pub async fn generate_plan(
        &self,
        provider: &Arc<dyn LlmProvider>,
        model: &str,
        task: &str,
    ) -> Result<OrchestratorPlan, String> {
        let prompt = format!(
            r#"Você é um orquestrador de tarefas. Given a user task, break it down into specific, executable steps.

Tarefa: {}

Analise a tarefa e gere uma lista de steps. Para cada step, especifique:
1. Descrição clara do que fazer
2. Ferramenta a usar (bash, file_read, file_write, repo_search, web_search, web_fetch)
3. Parâmetros necessários para a ferramenta

Retorne em JSON com formato:
{{
  "steps": [
    {{
      "id": "step_1",
      "description": "Descrição do que fazer",
      "tool_name": "nome da ferramenta",
      "tool_input": {{ "param1": "valor1" }}
    }}
  ]
}}

Apenas retorne o JSON, sem explicações."#,
            task
        );

        let messages = vec![ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text(prompt),
        }];

        let request = LlmRequest {
            model: model.to_string(),
            messages,
            system: Some("You are a task planning assistant. Generate execution plans in JSON format.".to_string()),
            max_tokens: Some(4096),
            temperature: Some(0.3),
            tools: vec![],
        };

        let response = provider.complete(&request)
            .await
            .map_err(|e| format!("LLM error: {}", e))?;

        let response_text = extract_text(&response.content);

        // Parse JSON from response
        let plan = self.parse_plan_from_response(&response_text, task)?;
        
        info!("Generated plan with {} steps", plan.steps.len());
        Ok(plan)
    }

    /// Parse o plano a partir da resposta do LLM
    fn parse_plan_from_response(&self, response: &str, task: &str) -> Result<OrchestratorPlan, String> {
        // Try to extract JSON from response
        let json_str = extract_json_from_text(response)
            .ok_or_else(|| "Could not extract JSON from response".to_string())?;

        let parsed: serde_json::Value = serde_json::from_str(&json_str)
            .map_err(|e| format!("Failed to parse JSON: {}", e))?;

        let steps_array = parsed.get("steps")
            .and_then(|v| v.as_array())
            .ok_or_else(|| "Missing 'steps' array in response".to_string())?;

        let mut plan = OrchestratorPlan::new(task);

        for (idx, step_val) in steps_array.iter().enumerate() {
            let description = step_val.get("description")
                .and_then(|v| v.as_str())
                .unwrap_or("No description")
                .to_string();

            let tool_name = step_val.get("tool_name")
                .and_then(|v| v.as_str())
                .unwrap_or("bash")
                .to_string();

            let tool_input = step_val.get("tool_input")
                .cloned()
                .unwrap_or(serde_json::json!({}));

            let mut step = OrchestratorStep::new(
                (idx + 1) as u32,
                &description,
                &tool_name,
                tool_input,
            );

            if let Some(validation) = step_val.get("validation").and_then(|v| v.as_str()) {
                step = step.with_validation(validation);
            }

            if let Some(condition) = step_val.get("condition").and_then(|v| v.as_str()) {
                step = step.with_condition(condition);
            }

            plan.add_step(step);
        }

        if plan.steps.is_empty() {
            // Fallback: criar um único step com a tarefa
            plan.add_step(OrchestratorStep::new(
                1,
                &format!("Execute task: {}", task),
                "bash",
                serde_json::json!({ "command": task }),
            ));
        }

        Ok(plan)
    }

    /// Executa o plano completo
    pub async fn execute_plan(
        &mut self,
        provider: Option<&Arc<dyn LlmProvider>>,
        session_id: &str,
    ) -> Result<OrchestratorSummary, String> {
        let mut plan = match self.execution_history.pop() {
            Some(p) => p,
            None => return Err("No plan to execute. Generate a plan first.".to_string()),
        };

        let mut successful_steps = 0;
        let mut failed_steps = 0;
        let mut retried_steps = 0;
        let start_time = std::time::Instant::now();

        // Loop principal de execução
        let mut loop_count = 0;
        
        while !plan.completed && loop_count < self.limits.max_loops {
            loop_count += 1;
            info!("Orchestrator loop {} of {}", loop_count, self.limits.max_loops);

            // Encontrar próximo step pendente
            let next_step_idx = plan.steps.iter()
                .position(|s| s.status == StepStatus::Pending);

            if let Some(idx) = next_step_idx {
                let step = &mut plan.steps[idx];
                step.status = StepStatus::Running;
                plan.current_step = idx;

                // Executar o step
                let result = self.execute_step(step, session_id).await;

                match result {
                    Ok(()) => {
                        step.status = StepStatus::Completed;
                        successful_steps += 1;
                        
                        // Validar resultado se houver validação definida
                        if let Some(validation) = &step.validation {
                            let validation_result = self.validate_step_result(step, validation, provider).await;
                            if !validation_result.passed {
                                warn!("Step {} validation failed: {}", step.id, validation_result.message);
                                if validation_result.should_retry && step.attempts < self.limits.retry_count {
                                    step.status = StepStatus::Retrying;
                                    retried_steps += 1;
                                    continue;
                                }
                            } else {
                                step.status = StepStatus::Validated;
                            }
                        }
                    }
                    Err(e) => {
                        error!("Step {} failed: {}", step.id, e);
                        step.error = Some(e.clone());
                        
                        // Tentar retry se não excedeu limite
                        if step.attempts < self.limits.retry_count {
                            step.attempts += 1;
                            step.status = StepStatus::Retrying;
                            retried_steps += 1;
                            warn!("Retrying step {} (attempt {}/{})", 
                                  step.id, step.attempts, self.limits.retry_count);
                        } else {
                            step.status = StepStatus::Failed;
                            failed_steps += 1;
                        }
                    }
                }
            } else {
                // Não há mais steps pendentes
                plan.completed = true;
                break;
            }
        }

        if loop_count >= self.limits.max_loops && !plan.completed {
            warn!("Orchestrator reached max loops limit");
        }

        // Gerar summary
        let summary = self.generate_summary(&plan, successful_steps, failed_steps, retried_steps, start_time);
        
        // Armazenar no histórico
        plan.summary = Some(summary.summary_text.clone());
        self.execution_history.push(plan);

        Ok(summary)
    }

    /// Executa um único step
    async fn execute_step(&self, step: &mut OrchestratorStep, session_id: &str) -> Result<(), String> {
        info!("Executing step {} with tool {}", step.id, step.tool_name);

        // Encontrar ferramenta
        let tool = self.find_tool(&step.tool_name)
            .ok_or_else(|| format!("Tool not found: {}", step.tool_name))?;

        // Criar contexto
        let context = ToolContext {
            session_id: session_id.to_string(),
            user_id: None,
            is_heartbeat: false,
            is_confirmation_approved: false,
        };

        // Executar com timeout
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(self.limits.timeout_secs),
            tool.execute(&context, step.tool_input.clone()),
        )
        .await
        .map_err(|_| format!("Step {} timed out after {}s", step.id, self.limits.timeout_secs))?
        .map_err(|e| format!("Tool execution error: {}", e))?;

        if output.is_error {
            step.result = Some(output.content.clone());
            return Err(format!("Step failed: {}", output.content));
        }

        step.result = Some(output.content.clone());
        Ok(())
    }

    /// Valida o resultado de um step
    async fn validate_step_result(
        &self,
        step: &OrchestratorStep,
        validation: &str,
        _provider: Option<&Arc<dyn LlmProvider>>,
    ) -> ValidationResult {
        // Validação básica: verificar se resultado contém certas keywords
        let result = step.result.as_deref().unwrap_or("");
        
        // Validações simples por padrão
        let validation_lower = validation.to_lowercase();
        
        if validation_lower.contains("não vazio") || validation_lower.contains("not empty") {
            if result.trim().is_empty() {
                return ValidationResult {
                    passed: false,
                    message: "Result is empty but expected non-empty".to_string(),
                    should_retry: true,
                };
            }
        }
        
        if validation_lower.contains("sucesso") || validation_lower.contains("success") {
            if result.to_lowercase().contains("erro") || result.to_lowercase().contains("error") {
                return ValidationResult {
                    passed: false,
                    message: "Result contains error".to_string(),
                    should_retry: true,
                };
            }
        }

        // Verificar indicadores de falha comuns
        let failure_indicators = ["failed", "error:", "exception", "panic", "não encontrado", "not found"];
        for indicator in &failure_indicators {
            if result.to_lowercase().contains(indicator) {
                return ValidationResult {
                    passed: false,
                    message: format!("Result contains failure indicator: {}", indicator),
                    should_retry: true,
                };
            }
        }

        ValidationResult {
            passed: true,
            message: "Validation passed".to_string(),
            should_retry: false,
        }
    }

    /// Gera o resumo final
    fn generate_summary(
        &self,
        plan: &OrchestratorPlan,
        successful_steps: usize,
        failed_steps: usize,
        retried_steps: usize,
        start_time: std::time::Instant,
    ) -> OrchestratorSummary {
        let total_time = start_time.elapsed().as_secs();
        
        let step_details: Vec<StepDetail> = plan.steps.iter().map(|s| {
            StepDetail {
                id: s.id.clone(),
                description: s.description.clone(),
                status: format!("{:?}", s.status),
                attempts: s.attempts,
                result_preview: s.result.as_ref().map(|r| {
                    if r.len() > 100 {
                        format!("{}...", &r[..100])
                    } else {
                        r.clone()
                    }
                }),
                error: s.error.clone(),
            }
        }).collect();

        let summary_text = if plan.completed {
            if failed_steps > 0 {
                format!(
                    "Plano executado com {} steps completados, {} falharam. Tempo total: {}s. Retry: {}",
                    successful_steps, failed_steps, total_time, retried_steps
                )
            } else {
                format!(
                    "Plano executado com sucesso! {} steps completados em {}s.",
                    successful_steps, total_time
                )
            }
        } else {
            format!(
                "Plano parcialmente executado: {} de {} steps. {} falharam. Tempo: {}s",
                successful_steps,
                plan.steps.len(),
                failed_steps,
                total_time
            )
        };

        OrchestratorSummary {
            total_steps: plan.steps.len(),
            successful_steps,
            failed_steps,
            retried_steps,
            total_time_secs: total_time,
            summary_text,
            step_details,
        }
    }

    /// Armazena um plano para execução
    pub fn set_plan(&mut self, plan: OrchestratorPlan) {
        self.execution_history.push(plan);
    }

    /// Retorna o último plano gerado
    pub fn get_last_plan(&self) -> Option<&OrchestratorPlan> {
        self.execution_history.last()
    }

    /// Check de segurança para comandos bash
    pub fn validate_bash_command(command: &str) -> ValidationResult {
        let cmd_lower = command.to_lowercase();
        
        // Padrões perigosos
        let dangerous_patterns = [
            "rm -rf",
            "rm -r /",
            "del /s",
            "format c:",
            "> /dev/sd",
            "chmod 777",
            "curl | sh",
            "wget | sh",
            "dd if=",
            "mkfs",
            "fdisk",
        ];

        for pattern in &dangerous_patterns {
            if cmd_lower.contains(pattern) {
                return ValidationResult {
                    passed: false,
                    message: format!("Comando perigoso detectado: {}", pattern),
                    should_retry: false,
                };
            }
        }

        // Comandos que precisam de confirmação
        let warning_patterns = ["git push", "git commit", "npm publish", "cargo publish"];
        for pattern in &warning_patterns {
            if cmd_lower.contains(pattern) {
                return ValidationResult {
                    passed: true,
                    message: format!("Aviso: comando potencialmente destrutivo: {}", pattern),
                    should_retry: false,
                };
            }
        }

        ValidationResult {
            passed: true,
            message: "Comando aprovado no check de segurança".to_string(),
            should_retry: false,
        }
    }
}

impl Default for Orchestrator {
    fn default() -> Self {
        Self::new()
    }
}

/// Extrai texto de ContentBlocks
fn extract_text(content: &[ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Extrai JSON de texto que pode conter código markdown
fn extract_json_from_text(text: &str) -> Option<String> {
    // Try direct parse first
    if let Ok(_) = serde_json::from_str::<serde_json::Value>(text) {
        return Some(text.to_string());
    }

    // Try to find JSON in markdown code block
    if let Some(start) = text.find("```json") {
        let start = start + 7;
        if let Some(end) = text[start..].find("```") {
            return Some(text[start..start + end].trim().to_string());
        }
    }

    // Try to find any JSON object
    if let Some(start) = text.find('{') {
        let mut brace_count = 0;
        for (i, c) in text[start..].chars().enumerate() {
            match c {
                '{' => brace_count += 1,
                '}' => {
                    brace_count -= 1;
                    if brace_count == 0 {
                        return Some(text[start..start + i + 1].to_string());
                    }
                }
                _ => {}
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_orchestrator_limits_defaults() {
        let limits = OrchestratorLimits::default();
        assert_eq!(limits.max_loops, 10);
        assert_eq!(limits.timeout_secs, 30);
        assert_eq!(limits.retry_count, 2);
    }

    #[test]
    fn test_orchestrator_step_creation() {
        let step = OrchestratorStep::new(
            1,
            "Test step",
            "bash",
            serde_json::json!({ "command": "echo hello" }),
        );
        assert_eq!(step.id, "step_1");
        assert_eq!(step.status, StepStatus::Pending);
        assert_eq!(step.attempts, 0);
    }

    #[test]
    fn test_orchestrator_plan() {
        let mut plan = OrchestratorPlan::new("Test task");
        plan.add_step(OrchestratorStep::new(
            1, "Step 1", "bash", serde_json::json!({})
        ));
        plan.add_step(OrchestratorStep::new(
            2, "Step 2", "file_read", serde_json::json!({})
        ));

        assert_eq!(plan.steps.len(), 2);
        assert_eq!(plan.pending_count(), 2);
        assert_eq!(plan.completed_count(), 0);
    }

    #[test]
    fn test_bash_security_check_dangerous() {
        let result = Orchestrator::validate_bash_command("rm -rf /");
        assert!(!result.passed);
        assert!(result.message.contains("perigoso"));
    }

    #[test]
    fn test_bash_security_check_safe() {
        let result = Orchestrator::validate_bash_command("ls -la");
        assert!(result.passed);
    }

    #[test]
    fn test_bash_security_warning() {
        let result = Orchestrator::validate_bash_command("git push");
        assert!(result.passed);
        assert!(result.message.contains("Aviso"));
    }

    #[test]
    fn test_extract_json_from_text() {
        let text = r#"Some text before
```json
{"steps": [{"id": "1"}]}
```
more text"#;
        let json = extract_json_from_text(text);
        assert!(json.is_some());
    }
}
