//! # Modos de Execução (Agent Modes)
//!
//! Este módulo implementa o sistema de modos de execução do GarraIA,
//! permitindo diferentes estratégias de execução baseadas no contexto e canal.
//!
//! ## Conceitos Fundamentais
//!
//! - **Modo**: Estratégia de execução definida por perfil (não personalidade)
//! - **Precedência**: Header > Comando > Preferência canal > Preferência user > default
//! - **Tool Policy**: Regras de permitir/negar ferramentas por modo
//!
//! ## Modos Disponíveis
//!
//! | Modo | Descrição | Ferramentas |
//! |------|-----------|-------------|
//! | `auto` | Decide automaticamente via heurísticas | todas (limitado) |
//! | `search` | Busca e inspeção sem modificar | read-only |
//! | `architect` | Design e planejamento | limitadas |
//! | `code` | Implementação ativa | todas |
//! | `ask` | Apenas perguntas (padrão Telegram) | opcional |
//! | `debug` | Análise de erros e logs | read-only + bash |
//! | `orchestrator` | Execução multi-etapas | todas |
//! | `review` | Revisão de código | read-only |
//! | `edit` | Edição pontual | arquivo + bash |

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Enum de modos de execução do agente.
/// Cada modo define uma estratégia diferente de comportamento.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentMode {
    /// Decide automaticamente o modo baseado no conteúdo da mensagem
    Auto,
    /// Modo de busca e inspeção - apenas leitura
    Search,
    /// Modo de design e arquitetura
    Architect,
    /// Modo de implementação ativa - permite escrita
    Code,
    /// Modo de perguntas - apenas texto (padrão Telegram)
    Ask,
    /// Modo de debug - análise de erros
    Debug,
    /// Modo de execução multi-etapas
    Orchestrator,
    /// Modo de revisão de código
    Review,
    /// Modo de edição pontual
    Edit,
}

impl AgentMode {
    /// Parse string para AgentMode (case-insensitive)
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "search" => Some(Self::Search),
            "architect" => Some(Self::Architect),
            "code" => Some(Self::Code),
            "ask" => Some(Self::Ask),
            "debug" => Some(Self::Debug),
            "orchestrator" => Some(Self::Orchestrator),
            "review" => Some(Self::Review),
            "edit" => Some(Self::Edit),
            _ => None,
        }
    }

    /// Retorna o nome do modo como string
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Search => "search",
            Self::Architect => "architect",
            Self::Code => "code",
            Self::Ask => "ask",
            Self::Debug => "debug",
            Self::Orchestrator => "orchestrator",
            Self::Review => "review",
            Self::Edit => "edit",
        }
    }

    /// Lista todos os modos disponíveis
    pub fn all_modes() -> Vec<Self> {
        vec![
            Self::Auto,
            Self::Search,
            Self::Architect,
            Self::Code,
            Self::Ask,
            Self::Debug,
            Self::Orchestrator,
            Self::Review,
            Self::Edit,
        ]
    }
}

impl Default for AgentMode {
    fn default() -> Self {
        // Telegram default é "ask", outros canais podem usar "auto"
        Self::Ask
    }
}

impl std::fmt::Display for AgentMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Política de ferramentas para um modo específico.
/// Define quais ferramentas são permitidas, negadas ou obrigatórias.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolPolicy {
    /// Ferramentas explicitamente permitidas (whitelist)
    #[serde(default)]
    pub allowed: Vec<String>,
    /// Ferramentas explicitamente negadas (blacklist)
    #[serde(default)]
    pub denied: Vec<String>,
    /// Ferramentas obrigatórias para este modo
    #[serde(default)]
    pub required: Vec<String>,
    /// Se true, nega todas as ferramentas não listadas em `allowed`
    #[serde(default)]
    pub whitelist_mode: bool,
}

/// Configurações de LLM específicas por modo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeLlmConfig {
    /// Temperatura padrão (0.0 - 2.0)
    #[serde(default = "default_temperature")]
    pub temperature: f64,
    /// Máximo de tokens na resposta
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    /// Top-p para nucleus sampling
    #[serde(default = "default_top_p")]
    pub top_p: f64,
    /// Presença de penalização
    #[serde(default)]
    pub presence_penalty: Option<f64>,
    /// Frequência de penalização
    #[serde(default)]
    pub frequency_penalty: Option<f64>,
}

fn default_temperature() -> f64 {
    0.7
}

fn default_max_tokens() -> u32 {
    4096
}

fn default_top_p() -> f64 {
    0.9
}

impl Default for ModeLlmConfig {
    fn default() -> Self {
        Self {
            temperature: 0.7,
            max_tokens: 4096,
            top_p: 0.9,
            presence_penalty: None,
            frequency_penalty: None,
        }
    }
}

/// Limites de execução por modo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeLimits {
    /// Máximo de loops de ferramentas por requisição
    #[serde(default = "default_max_tool_loops")]
    pub max_tool_loops: u32,
    /// Timeout em segundos para execução de ferramentas
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    /// Máximo deturnos de conversa
    #[serde(default = "default_max_turns")]
    pub max_turns: u32,
}

fn default_max_tool_loops() -> u32 {
    50
}

fn default_timeout_secs() -> u64 {
    30
}

fn default_max_turns() -> u32 {
    20
}

impl Default for ModeLimits {
    fn default() -> Self {
        Self {
            max_tool_loops: 50,
            timeout_secs: 30,
            max_turns: 20,
        }
    }
}

/// Perfil completo de um modo de execução.
/// Contém todas as configurações necessárias para executar um agente em um modo específico.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeProfile {
    /// Nome do modo
    pub name: String,
    /// Descrição curta do modo
    pub description: String,
    /// Template de system prompt (pode usar placeholders)
    #[serde(default)]
    pub system_prompt_template: Option<String>,
    /// Política de ferramentas
    #[serde(default)]
    pub tool_policy: ToolPolicy,
    /// Configurações de LLM
    #[serde(default)]
    pub llm_config: ModeLlmConfig,
    /// Limites de execução
    #[serde(default)]
    pub limits: ModeLimits,
    /// Se o modo permite ferramentas (override)
    #[serde(default)]
    pub tools_enabled: bool,
    /// Modo pai (para modos custom baseados em outro)
    #[serde(default)]
    pub base_mode: Option<String>,
}

impl ModeProfile {
    /// Cria um perfil padrão para um modo
    pub fn from_mode(mode: AgentMode) -> Self {
        match mode {
            AgentMode::Auto => Self::default_auto(),
            AgentMode::Search => Self::default_search(),
            AgentMode::Architect => Self::default_architect(),
            AgentMode::Code => Self::default_code(),
            AgentMode::Ask => Self::default_ask(),
            AgentMode::Debug => Self::default_debug(),
            AgentMode::Orchestrator => Self::default_orchestrator(),
            AgentMode::Review => Self::default_review(),
            AgentMode::Edit => Self::default_edit(),
        }
    }

    fn default_auto() -> Self {
        Self {
            name: "auto".to_string(),
            description: "Decide automaticamente o modo baseado no conteúdo".to_string(),
            system_prompt_template: Some(
                "You are an intelligent AI assistant. Analyze the user's request and choose the appropriate strategy. If the user asks to implement, create, or modify code → use code mode. If they ask to explain → use ask mode. If they share an error or stack trace → use debug mode.".to_string(),
            ),
            tool_policy: ToolPolicy::default(),
            llm_config: ModeLlmConfig::default(),
            limits: ModeLimits::default(),
            tools_enabled: true,
            base_mode: None,
        }
    }

    fn default_search() -> Self {
        Self {
            name: "search".to_string(),
            description: "Busca e inspeção sem modificar arquivos".to_string(),
            system_prompt_template: Some(
                "You are a search assistant. Your goal is to find information in the codebase without making any modifications. Use read-only tools like file_search, repo_search, and list_dir. Never use file_write or bash commands that modify files.".to_string(),
            ),
            tool_policy: ToolPolicy {
                allowed: vec![
                    "file_read".to_string(),
                    "repo_search".to_string(),
                    "list_dir".to_string(),
                    "web_search".to_string(),
                    "web_fetch".to_string(),
                ],
                denied: vec![
                    "file_write".to_string(),
                    "bash".to_string(),
                ],
                required: vec![],
                whitelist_mode: true,
            },
            llm_config: ModeLlmConfig {
                temperature: 0.3,
                max_tokens: 2048,
                top_p: 0.8,
                ..Default::default()
            },
            limits: ModeLimits {
                max_tool_loops: 10,
                timeout_secs: 15,
                max_turns: 5,
            },
            tools_enabled: true,
            base_mode: None,
        }
    }

    fn default_architect() -> Self {
        Self {
            name: "architect".to_string(),
            description: "Design, planejamento e arquitetura".to_string(),
            system_prompt_template: Some(
                "You are an architect assistant. Focus on high-level design, patterns, and best practices. Provide analysis and recommendations without implementing code directly. Use search tools to find relevant patterns.".to_string(),
            ),
            tool_policy: ToolPolicy {
                allowed: vec![
                    "file_read".to_string(),
                    "repo_search".to_string(),
                    "list_dir".to_string(),
                    "web_search".to_string(),
                    "web_fetch".to_string(),
                ],
                denied: vec!["file_write".to_string()],
                required: vec![],
                whitelist_mode: true,
            },
            llm_config: ModeLlmConfig {
                temperature: 0.5,
                max_tokens: 4096,
                top_p: 0.9,
                ..Default::default()
            },
            limits: ModeLimits {
                max_tool_loops: 15,
                timeout_secs: 20,
                max_turns: 10,
            },
            tools_enabled: true,
            base_mode: None,
        }
    }

    fn default_code() -> Self {
        Self {
            name: "code".to_string(),
            description: "Implementação ativa - permite escrita e execução".to_string(),
            system_prompt_template: Some(
                "You are a coding assistant. Implement solutions, create files, and run commands as needed. Use file_write for creating/modifying code, bash for running commands. Always follow best practices and security guidelines.".to_string(),
            ),
            tool_policy: ToolPolicy {
                allowed: vec![],
                denied: vec![],
                required: vec![],
                whitelist_mode: false, // Permite tudo por padrão
            },
            llm_config: ModeLlmConfig {
                temperature: 0.4,
                max_tokens: 8192,
                top_p: 0.9,
                ..Default::default()
            },
            limits: ModeLimits {
                max_tool_loops: 50,
                timeout_secs: 30,
                max_turns: 20,
            },
            tools_enabled: true,
            base_mode: None,
        }
    }

    fn default_ask() -> Self {
        Self {
            name: "ask".to_string(),
            description: "Apenas perguntas - modo padrão do Telegram".to_string(),
            system_prompt_template: Some(
                "You are a helpful assistant. Answer questions clearly and concisely. Use tools only when necessary to find information. Prefer providing text explanations over executing code.".to_string(),
            ),
            tool_policy: ToolPolicy {
                allowed: vec![],
                denied: vec!["file_write".to_string()],
                required: vec![],
                whitelist_mode: false,
            },
            llm_config: ModeLlmConfig {
                temperature: 0.7,
                max_tokens: 2048,
                top_p: 0.9,
                ..Default::default()
            },
            limits: ModeLimits {
                max_tool_loops: 5,
                timeout_secs: 10,
                max_turns: 3,
            },
            tools_enabled: true,
            base_mode: None,
        }
    }

    fn default_debug() -> Self {
        Self {
            name: "debug".to_string(),
            description: "Análise de erros, stack traces e logs".to_string(),
            system_prompt_template: Some(
                "You are a debugging assistant. Analyze errors, stack traces, and logs to find the root cause. Use search and read tools to investigate code. Explain findings clearly.".to_string(),
            ),
            tool_policy: ToolPolicy {
                allowed: vec![
                    "file_read".to_string(),
                    "repo_search".to_string(),
                    "list_dir".to_string(),
                    "bash".to_string(),
                ],
                denied: vec!["file_write".to_string()],
                required: vec![],
                whitelist_mode: true,
            },
            llm_config: ModeLlmConfig {
                temperature: 0.3,
                max_tokens: 4096,
                top_p: 0.8,
                ..Default::default()
            },
            limits: ModeLimits {
                max_tool_loops: 20,
                timeout_secs: 20,
                max_turns: 10,
            },
            tools_enabled: true,
            base_mode: None,
        }
    }

    fn default_orchestrator() -> Self {
        Self {
            name: "orchestrator".to_string(),
            description: "Execução multi-etapas com planejamento".to_string(),
            system_prompt_template: Some(
                "You are an orchestrator agent. Break down complex tasks into steps, execute them, validate results, and provide summaries. Coordinate multiple operations while maintaining security.\n\nAvailable tools: bash, file_read, file_write, repo_search, web_search, web_fetch.\n\nFor each task:\n1. Generate a plan with specific steps\n2. Execute steps sequentially\n3. Validate each result\n4. Retry if needed (max 2 retries per step)\n5. Provide final summary".to_string(),
            ),
            tool_policy: ToolPolicy {
                allowed: vec![
                    "bash".to_string(),
                    "file_read".to_string(),
                    "file_write".to_string(),
                    "repo_search".to_string(),
                    "web_search".to_string(),
                    "web_fetch".to_string(),
                ],
                denied: vec![],
                required: vec![],
                whitelist_mode: false,
            },
            llm_config: ModeLlmConfig {
                temperature: 0.5,
                max_tokens: 8192,
                top_p: 0.9,
                ..Default::default()
            },
            limits: ModeLimits {
                max_tool_loops: 100,
                timeout_secs: 60,
                max_turns: 30,
            },
            tools_enabled: true,
            base_mode: None,
        }
    }

    fn default_review() -> Self {
        Self {
            name: "review".to_string(),
            description: "Revisão de código e análise de changes".to_string(),
            system_prompt_template: Some(
                "You are a code review assistant. Analyze code changes, provide constructive feedback, and suggest improvements. Focus on code quality, security, and best practices.".to_string(),
            ),
            tool_policy: ToolPolicy {
                allowed: vec![
                    "file_read".to_string(),
                    "git_diff".to_string(),
                    "repo_search".to_string(),
                    "list_dir".to_string(),
                ],
                denied: vec![
                    "file_write".to_string(),
                    "bash".to_string(),
                ],
                required: vec![],
                whitelist_mode: true,
            },
            llm_config: ModeLlmConfig {
                temperature: 0.4,
                max_tokens: 4096,
                top_p: 0.85,
                ..Default::default()
            },
            limits: ModeLimits {
                max_tool_loops: 10,
                timeout_secs: 20,
                max_turns: 5,
            },
            tools_enabled: true,
            base_mode: None,
        }
    }

    fn default_edit() -> Self {
        Self {
            name: "edit".to_string(),
            description: "Edição pontual de arquivos".to_string(),
            system_prompt_template: Some(
                "You are an editing assistant. Make precise, targeted changes to files. Use search_and_replace for modifications. Always preserve existing code structure.".to_string(),
            ),
            tool_policy: ToolPolicy {
                allowed: vec![
                    "file_read".to_string(),
                    "file_write".to_string(),
                    "search_and_replace".to_string(),
                    "repo_search".to_string(),
                ],
                denied: vec![],
                required: vec![],
                whitelist_mode: true,
            },
            llm_config: ModeLlmConfig {
                temperature: 0.3,
                max_tokens: 2048,
                top_p: 0.8,
                ..Default::default()
            },
            limits: ModeLimits {
                max_tool_loops: 10,
                timeout_secs: 15,
                max_turns: 5,
            },
            tools_enabled: true,
            base_mode: None,
        }
    }
}

impl Default for ModeProfile {
    fn default() -> Self {
        Self::from_mode(AgentMode::Ask)
    }
}

/// Engine de modos que gerencia a resolução e aplicação de modos.
pub struct ModeEngine {
    /// Mapa de perfis de modo por nome
    profiles: HashMap<String, ModeProfile>,
    /// Modo padrão por canal
    channel_defaults: HashMap<String, String>,
    /// Flag para habilitar router LLM (P1)
    auto_router_llm_enabled: bool,
}

impl ModeEngine {
    /// Cria um novo ModeEngine com os modos padrão
    pub fn new() -> Self {
        let mut engine = Self {
            profiles: HashMap::new(),
            channel_defaults: HashMap::new(),
            auto_router_llm_enabled: false, // Desabilitado por padrão (P1)
        };

        // Registrar todos os modos padrão
        for mode in AgentMode::all_modes() {
            let profile = ModeProfile::from_mode(mode);
            engine.profiles.insert(profile.name.clone(), profile);
        }

        // Definir defaults por canal
        engine.channel_defaults.insert("telegram".to_string(), "ask".to_string());
        engine.channel_defaults.insert("web".to_string(), "auto".to_string());
        engine.channel_defaults.insert("vscode".to_string(), "auto".to_string());
        engine.channel_defaults.insert("discord".to_string(), "ask".to_string());
        engine.channel_defaults.insert("whatsapp".to_string(), "ask".to_string());

        engine
    }

    /// Get perfil de modo pelo nome
    pub fn get_profile(&self, mode_name: &str) -> Option<&ModeProfile> {
        self.profiles.get(mode_name.to_lowercase().as_str())
    }

    /// Lista todos os perfis de modo
    pub fn list_profiles(&self) -> Vec<&ModeProfile> {
        self.profiles.values().collect()
    }

    /// Resolve o modo efetivo baseado em múltiplas fontes de precedência:
    /// 1. Header (X-Agent-Mode)
    /// 2. Comando do chat (/mode)
    /// 3. Preferência por canal
    /// 4. Preferência por user_id (futuro)
    /// 5. Default
    pub fn resolve_mode(
        &self,
        header_mode: Option<&str>,
        session_mode: Option<&str>,
        channel: Option<&str>,
    ) -> AgentMode {
        // 1. Header tem maior precedência
        if let Some(mode_str) = header_mode {
            if let Some(mode) = AgentMode::from_str(mode_str) {
                return mode;
            }
        }

        // 2. Modo salvo na sessão (via comando /mode)
        if let Some(mode_str) = session_mode {
            if let Some(mode) = AgentMode::from_str(mode_str) {
                return mode;
            }
        }

        // 3. Default por canal
        if let Some(channel_str) = channel {
            if let Some(default_mode) = self.channel_defaults.get(channel_str.to_lowercase().as_str()) {
                if let Some(mode) = AgentMode::from_str(default_mode) {
                    return mode;
                }
            }
        }

        // 4. Default global
        AgentMode::default()
    }

    /// Resolve o modo automaticamente usando heurísticas (GAR-M3-1)
    pub fn resolve_auto_mode(&self, user_input: &str) -> AgentMode {
        let input_lower = user_input.to_lowercase();

        // Contains file path → search ou debug
        if input_lower.contains("c:\\") || input_lower.contains("g:\\") 
            || input_lower.contains("/home/") || input_lower.contains("src/") 
            || input_lower.contains("crates/") {
            // Dependendo do verbo
            if input_lower.contains("erro") || input_lower.contains("panic") 
                || input_lower.contains("stacktrace") || input_lower.contains("bug") {
                return AgentMode::Debug;
            }
            return AgentMode::Search;
        }

        // Palavras-chave de implementação → code
        let code_keywords = [
            "implementar", "criar arquivo", "refatorar", "escrever código",
            "implement", "create file", "refactor", "write code",
            "crie", "faça", "make", "build", "add", "fix",
        ];
        if code_keywords.iter().any(|kw| input_lower.contains(kw)) {
            return AgentMode::Code;
        }

        // Palavras-chave de perguntas → ask
        let ask_keywords = [
            "o que é", "explique", "como funciona", "defina",
            "what is", "explain", "how does", "define",
            "?", "qual é", "quais são",
        ];
        if ask_keywords.iter().any(|kw| input_lower.contains(kw)) {
            return AgentMode::Ask;
        }

        // Palavras-chave de debug → debug
        let debug_keywords = [
            "erro", "stacktrace", "panic", "exception", "falha",
            "error", "stack trace", "crash", "bug", "problema",
        ];
        if debug_keywords.iter().any(|kw| input_lower.contains(kw)) {
            return AgentMode::Debug;
        }

        // Palavras-chave de arquitetura → architect
        let arch_keywords = [
            "roadmap", "design", "arquitetura", "planejamento",
            "roadmap", "design", "architecture", "plan",
        ];
        if arch_keywords.iter().any(|kw| input_lower.contains(kw)) {
            return AgentMode::Architect;
        }

        // Palavras-chave de review → review
        let review_keywords = [
            "review", "revisar", "analisar diff", "analise",
            "revise", "analyze", "check",
        ];
        if review_keywords.iter().any(|kw| input_lower.contains(kw)) {
            return AgentMode::Review;
        }

        // Padrão: ask (mais seguro para Telegram)
        AgentMode::Ask
    }

    /// Registra um modo customizado
    pub fn register_custom_mode(&mut self, profile: ModeProfile) {
        self.profiles.insert(profile.name.clone(), profile);
    }

    /// Define default por canal
    pub fn set_channel_default(&mut self, channel: &str, mode: &str) {
        self.channel_defaults.insert(channel.to_lowercase(), mode.to_string());
    }

    /// Habilita/desabilita router LLM (P1)
    pub fn set_auto_router_llm(&mut self, enabled: bool) {
        self.auto_router_llm_enabled = enabled;
    }

    /// Verifica se ferramenta é permitida pelo policy
    pub fn is_tool_allowed(&self, mode: AgentMode, tool_name: &str) -> bool {
        let profile = match self.get_profile(mode.as_str()) {
            Some(p) => p,
            None => return true, // Se não encontrar perfil, permite
        };

        // Se whitelist_mode, nega tudo que não está na lista de allowed
        if profile.tool_policy.whitelist_mode {
            // Permite se está na lista de allowed OU se allowed está vazia
            profile.tool_policy.allowed.is_empty() 
                || profile.tool_policy.allowed.iter().any(|t| t == tool_name)
        } else {
            // Se não tem whitelist, nega apenas se está na lista de denied
            !profile.tool_policy.denied.iter().any(|t| t == tool_name)
        }
    }

    /// Retorna o system prompt para um modo (com possível override)
    pub fn get_system_prompt(&self, mode: AgentMode, custom_prompt: Option<&str>) -> String {
        if let Some(prompt) = custom_prompt {
            return prompt.to_string();
        }

        if let Some(profile) = self.get_profile(mode.as_str()) {
            if let Some(template) = &profile.system_prompt_template {
                return template.clone();
            }
        }

        // Fallback para ask
        "You are a helpful AI assistant.".to_string()
    }
}

impl Default for ModeEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Informações de modo para contexto de execução
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeContext {
    /// Modo atual
    pub mode: AgentMode,
    /// Nome do modo como string
    pub mode_name: String,
    /// Canal origem (telegram, web, etc)
    #[serde(default)]
    pub channel: Option<String>,
    /// Session ID para persistência
    #[serde(default)]
    pub session_id: Option<String>,
    /// Se tools estão habilitadas
    #[serde(default = "default_true")]
    pub tools_enabled: bool,
    /// tool_choice do request (auto/none/required)
    #[serde(default)]
    pub tool_choice: Option<String>,
}

fn default_true() -> bool {
    true
}

impl ModeContext {
    /// Cria contexto de modo com defaults
    pub fn new(mode: AgentMode) -> Self {
        Self {
            mode,
            mode_name: mode.as_str().to_string(),
            channel: None,
            session_id: None,
            tools_enabled: true,
            tool_choice: None,
        }
    }

    /// Resolve se tools estão habilitadas baseado no tool_choice
    pub fn resolve_tools_enabled(&self) -> bool {
        match self.tool_choice.as_deref() {
            Some("none") => false,
            _ => self.tools_enabled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_mode_from_str() {
        assert_eq!(AgentMode::from_str("code"), Some(AgentMode::Code));
        assert_eq!(AgentMode::from_str("CODE"), Some(AgentMode::Code));
        assert_eq!(AgentMode::from_str("ask"), Some(AgentMode::Ask));
        assert_eq!(AgentMode::from_str("invalid"), None);
    }

    #[test]
    fn test_mode_engine_resolve() {
        let engine = ModeEngine::new();

        // Header tem precedência
        let mode = engine.resolve_mode(Some("debug"), None, None);
        assert_eq!(mode, AgentMode::Debug);

        // Session override
        let mode = engine.resolve_mode(None, Some("code"), None);
        assert_eq!(mode, AgentMode::Code);

        // Canal
        let mode = engine.resolve_mode(None, None, Some("web"));
        assert_eq!(mode, AgentMode::Auto);

        // Default
        let mode = engine.resolve_mode(None, None, None);
        assert_eq!(mode, AgentMode::Ask); // Default
    }

    #[test]
    fn test_tool_policy_search() {
        let engine = ModeEngine::new();

        // Search mode: file_read permitida, file_write negada
        assert!(engine.is_tool_allowed(AgentMode::Search, "file_read"));
        assert!(!engine.is_tool_allowed(AgentMode::Search, "file_write"));
        assert!(!engine.is_tool_allowed(AgentMode::Search, "bash"));
    }

    #[test]
    fn test_tool_policy_code() {
        let engine = ModeEngine::new();

        // Code mode: permite tudo (não tem whitelist)
        assert!(engine.is_tool_allowed(AgentMode::Code, "file_write"));
        assert!(engine.is_tool_allowed(AgentMode::Code, "bash"));
    }

    #[test]
    fn test_auto_mode_heuristics() {
        let engine = ModeEngine::new();

        // Path → search
        let mode = engine.resolve_auto_mode("Veja o arquivo src/main.rs");
        assert_eq!(mode, AgentMode::Search);

        // Erro → debug
        let mode = engine.resolve_auto_mode("Getting error: panic in main");
        assert_eq!(mode, AgentMode::Debug);

        // Implementar → code
        let mode = engine.resolve_auto_mode("Implemente uma função");
        assert_eq!(mode, AgentMode::Code);

        // Pergunta → ask
        let mode = engine.resolve_auto_mode("O que é Rust?");
        assert_eq!(mode, AgentMode::Ask);
    }

    #[test]
    fn test_mode_profile_defaults() {
        let profile = ModeProfile::from_mode(AgentMode::Code);
        assert_eq!(profile.name, "code");
        assert!(profile.tools_enabled);

        let profile = ModeProfile::from_mode(AgentMode::Search);
        assert_eq!(profile.name, "search");
        assert!(profile.tool_policy.whitelist_mode);
    }
}
