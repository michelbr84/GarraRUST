//! # Agent Mode System (Phase 5.1)
//!
//! Extended execution modes with ToolPolicyEngine, AutoRouter, and LlmRouter.
//! Builds on top of the existing `modes.rs` module.
//!
//! ## Components:
//! - `ToolPolicyEngine`: Centralized tool permission engine per mode
//! - `AutoRouter`: Keyword-based mode detection from message content
//! - `LlmRouter`: LLM-based mode detection (optional, more accurate)
//! - Mode persistence via session metadata

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::debug;

use crate::modes::{AgentMode, ModeProfile, ToolPolicy};
use crate::providers::{ChatMessage, ChatRole, ContentBlock, LlmProvider, LlmRequest, MessagePart};

/// Extended mode profile with additional fields for Phase 5.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeProfileExt {
    /// Base mode profile from modes.rs
    pub profile: ModeProfile,
    /// Additional system prompt addon (appended to base prompt)
    #[serde(default)]
    pub system_prompt_addon: Option<String>,
    /// Maximum iterations for agentic loops in this mode
    #[serde(default = "default_max_iterations")]
    pub max_iterations: u32,
}

fn default_max_iterations() -> u32 {
    20
}

impl ModeProfileExt {
    /// Create from an AgentMode with defaults
    pub fn from_mode(mode: AgentMode) -> Self {
        let profile = ModeProfile::from_mode(mode);
        let max_iterations = match mode {
            AgentMode::Ask => 3,
            AgentMode::Code => 50,
            AgentMode::Debug => 20,
            AgentMode::Review => 10,
            AgentMode::Auto => 30,
            AgentMode::Orchestrator => 100,
            AgentMode::Search => 10,
            AgentMode::Architect => 15,
            AgentMode::Edit => 10,
        };
        Self {
            profile,
            system_prompt_addon: None,
            max_iterations,
        }
    }
}

/// ToolPolicyEngine: centralized engine that determines which tools
/// are allowed or denied for a given mode.
pub struct ToolPolicyEngine {
    /// Policies indexed by mode name
    policies: HashMap<String, ToolPolicy>,
    /// Global deny list (always denied regardless of mode)
    global_deny: Vec<String>,
}

impl ToolPolicyEngine {
    /// Create a new ToolPolicyEngine with default policies for all modes
    pub fn new() -> Self {
        let mut policies = HashMap::new();

        for mode in AgentMode::all_modes() {
            let profile = ModeProfile::from_mode(mode);
            policies.insert(mode.as_str().to_string(), profile.tool_policy);
        }

        Self {
            policies,
            global_deny: Vec::new(),
        }
    }

    /// Add a tool to the global deny list
    pub fn add_global_deny(&mut self, tool_name: &str) {
        self.global_deny.push(tool_name.to_string());
    }

    /// Register a custom policy for a mode
    pub fn register_policy(&mut self, mode_name: &str, policy: ToolPolicy) {
        self.policies.insert(mode_name.to_string(), policy);
    }

    /// Check if a tool is allowed for a given mode
    pub fn is_allowed(&self, mode: AgentMode, tool_name: &str) -> bool {
        // Global deny always takes precedence
        if self.global_deny.iter().any(|t| t == tool_name) {
            return false;
        }

        let policy = match self.policies.get(mode.as_str()) {
            Some(p) => p,
            None => return true, // Unknown mode: allow
        };

        if policy.whitelist_mode {
            policy.allowed.is_empty() || policy.allowed.iter().any(|t| t == tool_name)
        } else {
            !policy.denied.iter().any(|t| t == tool_name)
        }
    }

    /// Get list of allowed tools for a mode (returns None if all are allowed)
    pub fn allowed_tools(&self, mode: AgentMode) -> Option<Vec<String>> {
        let policy = self.policies.get(mode.as_str())?;
        if policy.whitelist_mode && !policy.allowed.is_empty() {
            Some(policy.allowed.clone())
        } else {
            None
        }
    }

    /// Get list of denied tools for a mode
    pub fn denied_tools(&self, mode: AgentMode) -> Vec<String> {
        let mut denied = self.global_deny.clone();
        if let Some(policy) = self.policies.get(mode.as_str()) {
            denied.extend(policy.denied.iter().cloned());
        }
        denied
    }
}

impl Default for ToolPolicyEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// AutoRouter: analyzes message content using keyword heuristics
/// to detect the appropriate execution mode.
pub struct AutoRouter {
    /// Custom keyword overrides (keyword -> mode)
    custom_keywords: HashMap<String, AgentMode>,
}

impl AutoRouter {
    /// Create a new AutoRouter with default keyword mappings
    pub fn new() -> Self {
        Self {
            custom_keywords: HashMap::new(),
        }
    }

    /// Add a custom keyword mapping
    pub fn add_keyword(&mut self, keyword: &str, mode: AgentMode) {
        self.custom_keywords.insert(keyword.to_lowercase(), mode);
    }

    /// Analyze message content and return the detected mode
    pub fn detect_mode(&self, message: &str) -> AgentMode {
        let input = message.to_lowercase();

        // Check custom keywords first
        for (keyword, mode) in &self.custom_keywords {
            if input.contains(keyword) {
                debug!(keyword = %keyword, mode = %mode, "AutoRouter: custom keyword match");
                return *mode;
            }
        }

        // Debug mode: fix/bug/error patterns
        let debug_keywords = [
            "fix",
            "bug",
            "error",
            "erro",
            "panic",
            "crash",
            "stacktrace",
            "stack trace",
            "exception",
            "falha",
            "problema",
            "debug",
            "broken",
            "failing",
            "not working",
        ];
        if debug_keywords.iter().any(|kw| input.contains(kw)) {
            return AgentMode::Debug;
        }

        // Review mode: review/check patterns (checked before code to avoid
        // "review this code" matching the "code" keyword in code_keywords)
        let review_keywords = [
            "review",
            "check",
            "analyze",
            "audit",
            "inspect",
            "revisar",
            "verificar",
            "analisar",
            "auditar",
            "inspecionar",
            "code review",
            "pr review",
        ];
        if review_keywords.iter().any(|kw| input.contains(kw)) {
            return AgentMode::Review;
        }

        // Code mode: write/create/implement patterns
        let code_keywords = [
            "write",
            "create",
            "implement",
            "build",
            "make",
            "add",
            "escrever",
            "criar",
            "implementar",
            "construir",
            "fazer",
            "adicionar",
            "refactor",
            "refatorar",
            "generate",
            "gerar",
            "code",
            "coding",
        ];
        if code_keywords.iter().any(|kw| input.contains(kw)) {
            return AgentMode::Code;
        }

        // Ask mode: explain/what patterns
        let ask_keywords = [
            "explain",
            "what",
            "how",
            "why",
            "describe",
            "define",
            "explicar",
            "o que",
            "como",
            "por que",
            "descrever",
            "definir",
            "?",
            "tell me",
            "diga",
        ];
        if ask_keywords.iter().any(|kw| input.contains(kw)) {
            return AgentMode::Ask;
        }

        // Default: Ask (safest)
        AgentMode::Ask
    }
}

impl Default for AutoRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// LlmRouter: uses an LLM to decide the appropriate mode.
/// More accurate than keyword-based routing but requires an LLM call.
pub struct LlmRouter {
    /// Whether the LLM router is enabled
    enabled: bool,
}

impl LlmRouter {
    /// Create a new LlmRouter
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    /// Detect mode using an LLM
    pub async fn detect_mode(
        &self,
        provider: &Arc<dyn LlmProvider>,
        model: &str,
        message: &str,
    ) -> std::result::Result<AgentMode, String> {
        if !self.enabled {
            return Err("LlmRouter is disabled".to_string());
        }

        let prompt = format!(
            r#"Classify the following user message into exactly one mode. Reply with ONLY the mode name, nothing else.

Available modes:
- ask: Questions, explanations, information requests
- code: Writing, creating, implementing, building code
- debug: Fixing bugs, analyzing errors, debugging issues
- review: Code review, analysis, auditing
- search: Finding information in codebase
- architect: Design, planning, architecture decisions
- edit: Small targeted file edits

User message: "{}"

Mode:"#,
            message
        );

        let messages = vec![ChatMessage {
            role: ChatRole::User,
            content: MessagePart::Text(prompt),
        }];

        let request = LlmRequest {
            model: model.to_string(),
            messages,
            system: Some(
                "You are a message classifier. Reply with exactly one word: the mode name."
                    .to_string(),
            ),
            max_tokens: Some(10),
            temperature: Some(0.0),
            tools: vec![],
        };

        let response = provider
            .complete(&request)
            .await
            .map_err(|e| format!("LLM router error: {}", e))?;

        let text = response
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.trim().to_lowercase()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");

        AgentMode::from_str(&text).ok_or_else(|| format!("LLM returned invalid mode: {}", text))
    }
}

impl Default for LlmRouter {
    fn default() -> Self {
        Self::new(false)
    }
}

/// Session mode metadata for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionModeMetadata {
    /// Current active mode
    pub current_mode: String,
    /// How the mode was selected
    pub selection_method: ModeSelectionMethod,
    /// Timestamp of last mode change
    pub last_changed: String,
    /// Previous mode (for undo)
    #[serde(default)]
    pub previous_mode: Option<String>,
}

/// How the mode was selected
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModeSelectionMethod {
    /// User explicitly set via command
    Manual,
    /// Auto-detected from message content
    AutoRouter,
    /// Detected by LLM
    LlmRouter,
    /// Channel default
    ChannelDefault,
    /// System default
    Default,
}

impl SessionModeMetadata {
    /// Create new metadata for a mode change
    pub fn new(mode: AgentMode, method: ModeSelectionMethod) -> Self {
        Self {
            current_mode: mode.as_str().to_string(),
            selection_method: method,
            last_changed: chrono::Utc::now().to_rfc3339(),
            previous_mode: None,
        }
    }

    /// Update mode with tracking
    pub fn update(&mut self, new_mode: AgentMode, method: ModeSelectionMethod) {
        self.previous_mode = Some(self.current_mode.clone());
        self.current_mode = new_mode.as_str().to_string();
        self.selection_method = method;
        self.last_changed = chrono::Utc::now().to_rfc3339();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_router_debug_keywords() {
        let router = AutoRouter::new();
        assert_eq!(router.detect_mode("fix this bug"), AgentMode::Debug);
        assert_eq!(router.detect_mode("I have an error"), AgentMode::Debug);
        assert_eq!(router.detect_mode("crash on startup"), AgentMode::Debug);
    }

    #[test]
    fn test_auto_router_code_keywords() {
        let router = AutoRouter::new();
        assert_eq!(router.detect_mode("write a function"), AgentMode::Code);
        assert_eq!(router.detect_mode("create a new module"), AgentMode::Code);
        assert_eq!(router.detect_mode("implement the API"), AgentMode::Code);
    }

    #[test]
    fn test_auto_router_review_keywords() {
        let router = AutoRouter::new();
        assert_eq!(router.detect_mode("review this code"), AgentMode::Review);
        assert_eq!(router.detect_mode("check the PR"), AgentMode::Review);
    }

    #[test]
    fn test_auto_router_ask_keywords() {
        let router = AutoRouter::new();
        assert_eq!(router.detect_mode("explain how this works"), AgentMode::Ask);
        assert_eq!(router.detect_mode("what is Rust?"), AgentMode::Ask);
    }

    #[test]
    fn test_auto_router_default() {
        let router = AutoRouter::new();
        assert_eq!(router.detect_mode("hello there"), AgentMode::Ask);
    }

    #[test]
    fn test_auto_router_custom_keyword() {
        let mut router = AutoRouter::new();
        router.add_keyword("deploy", AgentMode::Orchestrator);
        assert_eq!(
            router.detect_mode("deploy to production"),
            AgentMode::Orchestrator
        );
    }

    #[test]
    fn test_tool_policy_engine_defaults() {
        let engine = ToolPolicyEngine::new();
        // Code mode allows everything
        assert!(engine.is_allowed(AgentMode::Code, "bash"));
        assert!(engine.is_allowed(AgentMode::Code, "file_write"));

        // Search mode: whitelist only
        assert!(engine.is_allowed(AgentMode::Search, "file_read"));
        assert!(!engine.is_allowed(AgentMode::Search, "file_write"));
    }

    #[test]
    fn test_tool_policy_engine_global_deny() {
        let mut engine = ToolPolicyEngine::new();
        engine.add_global_deny("dangerous_tool");
        assert!(!engine.is_allowed(AgentMode::Code, "dangerous_tool"));
        assert!(!engine.is_allowed(AgentMode::Ask, "dangerous_tool"));
    }

    #[test]
    fn test_session_mode_metadata() {
        let mut meta = SessionModeMetadata::new(AgentMode::Ask, ModeSelectionMethod::Default);
        assert_eq!(meta.current_mode, "ask");
        assert!(meta.previous_mode.is_none());

        meta.update(AgentMode::Code, ModeSelectionMethod::Manual);
        assert_eq!(meta.current_mode, "code");
        assert_eq!(meta.previous_mode.as_deref(), Some("ask"));
    }

    #[test]
    fn test_mode_profile_ext() {
        let ext = ModeProfileExt::from_mode(AgentMode::Code);
        assert_eq!(ext.max_iterations, 50);
        assert_eq!(ext.profile.name, "code");

        let ext = ModeProfileExt::from_mode(AgentMode::Ask);
        assert_eq!(ext.max_iterations, 3);
    }
}
