//! # Multi-Agent Orchestration (Phase 5.2)
//!
//! Manages multiple sub-agents executing in parallel or as pipelines.
//!
//! ## Components:
//! - `AgentCoordinator`: Manages lifecycle of sub-agents
//! - `AgentHandle`: Join handle + cancel token + progress channel
//! - `parallel_execute`: Run agents concurrently via tokio::spawn
//! - `pipeline_execute`: Chain agents (output A -> input B)

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tracing::{info, warn};

use crate::modes::AgentMode;
use crate::providers::{ChatMessage, ChatRole, ContentBlock, LlmProvider, LlmRequest, MessagePart};

/// Result of a single agent execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    /// Task that was executed
    pub task: String,
    /// Mode the agent operated in
    pub mode: String,
    /// Output text from the agent
    pub output: String,
    /// Whether the execution succeeded
    pub success: bool,
    /// Error message if failed
    #[serde(default)]
    pub error: Option<String>,
    /// Duration in milliseconds
    pub duration_ms: u64,
}

/// Progress update from a running agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentProgress {
    /// Task identifier
    pub task: String,
    /// Current status
    pub status: AgentStatus,
    /// Progress message
    pub message: String,
}

/// Status of an agent execution
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    /// Queued but not yet started
    Pending,
    /// Currently running
    Running,
    /// Completed successfully
    Completed,
    /// Failed with error
    Failed,
    /// Cancelled by user or coordinator
    Cancelled,
}

/// Handle to a running agent — allows monitoring and cancellation
pub struct AgentHandle {
    /// Tokio join handle for the agent task
    join_handle: JoinHandle<AgentResult>,
    /// Send cancel signal
    cancel_tx: watch::Sender<bool>,
    /// Receive progress updates
    progress_rx: mpsc::Receiver<AgentProgress>,
    /// Task description
    pub task: String,
}

impl AgentHandle {
    /// Wait for the agent to complete and return its result
    pub async fn join(self) -> AgentResult {
        match self.join_handle.await {
            Ok(result) => result,
            Err(e) => AgentResult {
                task: self.task,
                mode: "unknown".to_string(),
                output: String::new(),
                success: false,
                error: Some(format!("Agent task panicked: {}", e)),
                duration_ms: 0,
            },
        }
    }

    /// Cancel the agent execution
    pub fn cancel(&self) {
        let _ = self.cancel_tx.send(true);
    }

    /// Try to receive a progress update (non-blocking)
    pub fn try_recv_progress(&mut self) -> Option<AgentProgress> {
        self.progress_rx.try_recv().ok()
    }
}

/// Configuration for a sub-agent
#[derive(Debug, Clone)]
pub struct SubAgentConfig {
    /// Task description / prompt
    pub task: String,
    /// Execution mode
    pub mode: AgentMode,
    /// System prompt override
    pub system_prompt: Option<String>,
    /// Maximum tokens for response
    pub max_tokens: Option<u32>,
    /// Temperature
    pub temperature: Option<f64>,
    /// Timeout in seconds
    pub timeout_secs: u64,
}

impl SubAgentConfig {
    /// Create a new sub-agent config with defaults
    pub fn new(task: impl Into<String>, mode: AgentMode) -> Self {
        Self {
            task: task.into(),
            mode,
            system_prompt: None,
            max_tokens: Some(4096),
            temperature: Some(0.7),
            timeout_secs: 60,
        }
    }

    /// Set system prompt
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    /// Set timeout
    pub fn with_timeout(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }
}

/// AgentCoordinator: manages multiple sub-agents with parallel and pipeline execution
pub struct AgentCoordinator {
    /// LLM provider for sub-agents
    provider: Arc<dyn LlmProvider>,
    /// Model to use for sub-agents
    model: String,
    /// Default system prompt for sub-agents
    default_system_prompt: String,
    /// Maximum concurrent agents
    max_concurrent: usize,
}

impl AgentCoordinator {
    /// Create a new AgentCoordinator
    pub fn new(provider: Arc<dyn LlmProvider>, model: impl Into<String>) -> Self {
        Self {
            provider,
            model: model.into(),
            default_system_prompt:
                "You are an AI assistant. Complete the assigned task precisely and concisely."
                    .to_string(),
            max_concurrent: 5,
        }
    }

    /// Set default system prompt
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.default_system_prompt = prompt.into();
        self
    }

    /// Set maximum concurrent agents
    pub fn with_max_concurrent(mut self, max: usize) -> Self {
        self.max_concurrent = max;
        self
    }

    /// Spawn a single agent as a background task
    pub fn spawn_agent(&self, config: SubAgentConfig) -> AgentHandle {
        let (cancel_tx, cancel_rx) = watch::channel(false);
        let (progress_tx, progress_rx) = mpsc::channel(16);
        let task_desc = config.task.clone();

        let provider = Arc::clone(&self.provider);
        let model = self.model.clone();
        let default_prompt = self.default_system_prompt.clone();

        let join_handle = tokio::spawn(async move {
            let start = std::time::Instant::now();

            // Send "running" progress
            let _ = progress_tx
                .send(AgentProgress {
                    task: config.task.clone(),
                    status: AgentStatus::Running,
                    message: "Agent started".to_string(),
                })
                .await;

            // Check for cancellation
            if *cancel_rx.borrow() {
                return AgentResult {
                    task: config.task,
                    mode: config.mode.as_str().to_string(),
                    output: String::new(),
                    success: false,
                    error: Some("Cancelled before execution".to_string()),
                    duration_ms: start.elapsed().as_millis() as u64,
                };
            }

            let system_prompt = config.system_prompt.unwrap_or(default_prompt);

            let messages = vec![ChatMessage {
                role: ChatRole::User,
                content: MessagePart::Text(config.task.clone()),
            }];

            let request = LlmRequest {
                model,
                messages,
                system: Some(system_prompt),
                max_tokens: config.max_tokens,
                temperature: config.temperature,
                tools: vec![],
            };

            let result = tokio::time::timeout(
                std::time::Duration::from_secs(config.timeout_secs),
                provider.complete(&request),
            )
            .await;

            let duration_ms = start.elapsed().as_millis() as u64;

            match result {
                Ok(Ok(response)) => {
                    let output = response
                        .content
                        .iter()
                        .filter_map(|block| match block {
                            ContentBlock::Text { text } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");

                    let _ = progress_tx
                        .send(AgentProgress {
                            task: config.task.clone(),
                            status: AgentStatus::Completed,
                            message: "Agent completed".to_string(),
                        })
                        .await;

                    AgentResult {
                        task: config.task,
                        mode: config.mode.as_str().to_string(),
                        output,
                        success: true,
                        error: None,
                        duration_ms,
                    }
                }
                Ok(Err(e)) => {
                    let _ = progress_tx
                        .send(AgentProgress {
                            task: config.task.clone(),
                            status: AgentStatus::Failed,
                            message: format!("Agent failed: {}", e),
                        })
                        .await;

                    AgentResult {
                        task: config.task,
                        mode: config.mode.as_str().to_string(),
                        output: String::new(),
                        success: false,
                        error: Some(format!("LLM error: {}", e)),
                        duration_ms,
                    }
                }
                Err(_) => {
                    let _ = progress_tx
                        .send(AgentProgress {
                            task: config.task.clone(),
                            status: AgentStatus::Failed,
                            message: format!("Agent timed out after {}s", config.timeout_secs),
                        })
                        .await;

                    AgentResult {
                        task: config.task,
                        mode: config.mode.as_str().to_string(),
                        output: String::new(),
                        success: false,
                        error: Some(format!("Timeout after {}s", config.timeout_secs)),
                        duration_ms,
                    }
                }
            }
        });

        AgentHandle {
            join_handle,
            cancel_tx,
            progress_rx,
            task: task_desc,
        }
    }

    /// Execute multiple agents in parallel and collect results
    pub async fn parallel_execute(&self, tasks: Vec<SubAgentConfig>) -> Vec<AgentResult> {
        info!(count = tasks.len(), "Starting parallel agent execution");

        // Limit concurrency
        let chunks: Vec<Vec<SubAgentConfig>> = tasks
            .into_iter()
            .collect::<Vec<_>>()
            .chunks(self.max_concurrent)
            .map(|c| c.to_vec())
            .collect();

        let mut all_results = Vec::new();

        for chunk in chunks {
            let handles: Vec<AgentHandle> = chunk
                .into_iter()
                .map(|config| self.spawn_agent(config))
                .collect();

            let mut results = Vec::with_capacity(handles.len());
            for handle in handles {
                results.push(handle.join().await);
            }

            all_results.extend(results);
        }

        let success_count = all_results.iter().filter(|r| r.success).count();
        info!(
            total = all_results.len(),
            success = success_count,
            "Parallel execution completed"
        );

        all_results
    }

    /// Execute agents as a pipeline: output of agent A becomes input to agent B
    pub async fn pipeline_execute(&self, tasks: Vec<SubAgentConfig>) -> AgentResult {
        info!(steps = tasks.len(), "Starting pipeline execution");

        if tasks.is_empty() {
            return AgentResult {
                task: "empty pipeline".to_string(),
                mode: "none".to_string(),
                output: String::new(),
                success: false,
                error: Some("No tasks in pipeline".to_string()),
                duration_ms: 0,
            };
        }

        let start = std::time::Instant::now();
        let mut previous_output = String::new();
        let mut last_result: Option<AgentResult> = None;

        for (idx, mut config) in tasks.into_iter().enumerate() {
            // Append previous output to task as context
            if !previous_output.is_empty() {
                config.task = format!(
                    "{}\n\n--- Context from previous step ---\n{}",
                    config.task, previous_output
                );
            }

            info!(
                step = idx + 1,
                task = %config.task.chars().take(100).collect::<String>(),
                "Pipeline step executing"
            );

            let handle = self.spawn_agent(config);
            let result = handle.join().await;

            if !result.success {
                warn!(step = idx + 1, error = ?result.error, "Pipeline step failed");
                return AgentResult {
                    task: result.task,
                    mode: result.mode,
                    output: result.output,
                    success: false,
                    error: Some(format!(
                        "Pipeline failed at step {}: {}",
                        idx + 1,
                        result.error.unwrap_or_default()
                    )),
                    duration_ms: start.elapsed().as_millis() as u64,
                };
            }

            previous_output = result.output.clone();
            last_result = Some(result);
        }

        let mut final_result = last_result.unwrap_or(AgentResult {
            task: "pipeline".to_string(),
            mode: "none".to_string(),
            output: String::new(),
            success: false,
            error: Some("No results produced".to_string()),
            duration_ms: 0,
        });

        final_result.duration_ms = start.elapsed().as_millis() as u64;
        final_result
    }
}

/// Summary of a multi-agent execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiAgentSummary {
    /// Total agents executed
    pub total_agents: usize,
    /// Successful executions
    pub successful: usize,
    /// Failed executions
    pub failed: usize,
    /// Total duration in milliseconds
    pub total_duration_ms: u64,
    /// Individual results
    pub results: Vec<AgentResult>,
}

impl MultiAgentSummary {
    /// Create summary from results
    pub fn from_results(results: Vec<AgentResult>) -> Self {
        let total = results.len();
        let successful = results.iter().filter(|r| r.success).count();
        let total_duration = results.iter().map(|r| r.duration_ms).max().unwrap_or(0);

        Self {
            total_agents: total,
            successful,
            failed: total - successful,
            total_duration_ms: total_duration,
            results,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sub_agent_config() {
        let config = SubAgentConfig::new("test task", AgentMode::Code)
            .with_system_prompt("Custom prompt")
            .with_timeout(120);

        assert_eq!(config.task, "test task");
        assert_eq!(config.mode, AgentMode::Code);
        assert_eq!(config.system_prompt.as_deref(), Some("Custom prompt"));
        assert_eq!(config.timeout_secs, 120);
    }

    #[test]
    fn test_agent_result_serialization() {
        let result = AgentResult {
            task: "test".to_string(),
            mode: "code".to_string(),
            output: "done".to_string(),
            success: true,
            error: None,
            duration_ms: 100,
        };

        let json = serde_json::to_string(&result).expect("should serialize");
        let parsed: AgentResult = serde_json::from_str(&json).expect("should deserialize");
        assert_eq!(parsed.task, "test");
        assert!(parsed.success);
    }

    #[test]
    fn test_multi_agent_summary() {
        let results = vec![
            AgentResult {
                task: "t1".to_string(),
                mode: "code".to_string(),
                output: "ok".to_string(),
                success: true,
                error: None,
                duration_ms: 100,
            },
            AgentResult {
                task: "t2".to_string(),
                mode: "ask".to_string(),
                output: String::new(),
                success: false,
                error: Some("timeout".to_string()),
                duration_ms: 200,
            },
        ];

        let summary = MultiAgentSummary::from_results(results);
        assert_eq!(summary.total_agents, 2);
        assert_eq!(summary.successful, 1);
        assert_eq!(summary.failed, 1);
    }
}
