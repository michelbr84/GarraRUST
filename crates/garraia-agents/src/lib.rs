#[cfg(feature = "mcp")]
pub mod mcp;

pub mod a2a;
pub mod agent_mode;
pub mod context_policy;
pub mod anthropic;
pub mod embeddings;
pub mod execution_budget;
pub mod memory_extractor;
pub mod modes;
pub mod multi_agent;
pub mod llama_cpp;
pub mod ollama;
pub mod openai;
pub mod orchestrator;
pub mod provider_resilience;
pub mod providers;
pub mod runtime;
pub mod tools;

pub use anthropic::AnthropicProvider;
pub use embeddings::{CohereEmbeddingProvider, EmbeddingProvider, OllamaEmbeddingProvider, OpenAiEmbeddingProvider};
pub use execution_budget::ExecutionBudget;
pub use llama_cpp::{KvCacheType, LlamaCppConfig, LlamaCppProvider};
pub use ollama::OllamaProvider;
pub use openai::OpenAiProvider;
pub use provider_resilience::{CircuitBreaker, FallbackConfig, ResilienceManager, RetryPolicy};
pub use providers::{
    ChatMessage, ChatRole, ContentBlock, LlmProvider, LlmRequest, LlmResponse, MessagePart,
    StreamEvent, ToolDefinition,
};
pub use runtime::AgentRuntime;
pub use modes::{
    AgentMode, ModeContext, ModeEngine, ModeLlmConfig, ModeLimits, ModeProfile, ToolPolicy,
};
pub use agent_mode::{
    AutoRouter, LlmRouter, ModeProfileExt, ModeSelectionMethod, SessionModeMetadata,
    ToolPolicyEngine,
};
pub use multi_agent::{
    AgentCoordinator, AgentHandle, AgentProgress, AgentResult, AgentStatus, MultiAgentSummary,
    SubAgentConfig,
};
pub use orchestrator::{
    Orchestrator, OrchestratorLimits, OrchestratorPlan, OrchestratorStep, OrchestratorSummary,
    StepDetail, StepStatus, ValidationResult,
};
pub use tools::{
    BashTool, CodeReviewTool, FileReadTool, FileWriteTool, ListDirTool, RepoSearchTool,
    RunTestsTool, ScheduleHeartbeat, Tool, ToolContext, ToolOutput, WebFetchTool, WebSearchTool,
    EventTrigger, EventType, ScheduledTask, TaskStatus, TriggerRegistry, WebhookTrigger,
};

#[cfg(feature = "mcp")]
pub use mcp::{McpManager, McpPromptInfo, McpResourceInfo, McpToolInfo};
