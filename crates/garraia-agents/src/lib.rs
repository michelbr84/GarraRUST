#[cfg(feature = "mcp")]
pub mod mcp;

pub mod a2a;
pub mod context_policy;
pub mod anthropic;
pub mod embeddings;
pub mod execution_budget;
pub mod memory_extractor;
pub mod modes;
pub mod ollama;
pub mod openai;
pub mod orchestrator;
pub mod provider_resilience;
pub mod providers;
pub mod runtime;
pub mod tools;

pub use anthropic::AnthropicProvider;
pub use embeddings::{CohereEmbeddingProvider, EmbeddingProvider, OllamaEmbeddingProvider};
pub use execution_budget::ExecutionBudget;
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
pub use orchestrator::{
    Orchestrator, OrchestratorLimits, OrchestratorPlan, OrchestratorStep, OrchestratorSummary,
    StepDetail, StepStatus, ValidationResult,
};
pub use tools::{
    BashTool, FileReadTool, FileWriteTool, ScheduleHeartbeat, Tool, ToolContext, ToolOutput,
    WebFetchTool, WebSearchTool,
};

#[cfg(feature = "mcp")]
pub use mcp::{McpManager, McpPromptInfo, McpResourceInfo, McpToolInfo};
