//! Módulo de Modos de Execução do GarraIA
//! 
//! Define o contrato de modo, perfis e resolução automática.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Modos de execução disponíveis no GarraIA
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentMode {
    /// Decide automaticamente baseado no contexto
    Auto,
    /// Busca e inspeção sem modificar
    Search,
    /// Análise de arquitetura e design
    Architect,
    /// Desenvolvimento e implementação
    Code,
    /// Consulta e explicação (padrão para Telegram)
    Ask,
    /// Debugging e análise de erros
    Debug,
    /// Execução multi-etapas com planos
    Orchestrator,
    /// Revisão de código e diffs
    Review,
    /// Edição focada
    Edit,
}

impl Default for AgentMode {
    fn default() -> Self {
        AgentMode::Ask // Padrão seguro para Telegram - não quebra comportamento atual
    }
}

impl std::fmt::Display for AgentMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentMode::Auto => write!(f, "auto"),
            AgentMode::Search => write!(f, "search"),
            AgentMode::Architect => write!(f, "architect"),
            AgentMode::Code => write!(f, "code"),
            AgentMode::Ask => write!(f, "ask"),
            AgentMode::Debug => write!(f, "debug"),
            AgentMode::Orchestrator => write!(f, "orchestrator"),
            AgentMode::Review => write!(f, "review"),
            AgentMode::Edit => write!(f, "edit"),
        }
    }
}

impl std::str::FromStr for AgentMode {
    type Err = String;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "auto" => Ok(AgentMode::Auto),
            "search" => Ok(AgentMode::Search),
            "architect" => Ok(AgentMode::Architect),
            "code" => Ok(AgentMode::Code),
            "ask" => Ok(AgentMode::Ask),
            "debug" => Ok(AgentMode::Debug),
            "orchestrator" => Ok(AgentMode::Orchestrator),
            "review" => Ok(AgentMode::Review),
            "edit" => Ok(AgentMode::Edit),
            _ => Err(format!("Modo desconhecido: {}", s)),
        }
    }
}

/// Políticas de tools por modo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPolicy {
    /// Tools explicitamente permitidas
    pub allowed: Vec<String>,
    /// Tools explicitamente negadas
    pub denied: Vec<String>,
    /// Tools em modo apenas leitura
    pub read_only: Vec<String>,
    /// Tool requerida por intenção (opcional)
    pub required: Option<String>,
}

impl Default for ToolPolicy {
    fn default() -> Self {
        Self {
            allowed: vec![],
            denied: vec![],
            read_only: vec![],
            required: None,
        }
    }
}

/// Parâmetros LLM padrão por modo
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct LlmDefaults {
    pub temperature: f32,
    pub max_tokens: u32,
    pub top_p: f32,
}

impl Default for LlmDefaults {
    fn default() -> Self {
        Self {
            temperature: 0.7,
            max_tokens: 4096,
            top_p: 1.0,
        }
    }
}

/// Limites de execução por modo
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ModeLimits {
    pub max_loops: u32,
    pub timeout_secs: u64,
}

impl Default for ModeLimits {
    fn default() -> Self {
        Self {
            max_loops: 10,
            timeout_secs: 120,
        }
    }
}

/// Perfil completo de um modo
#[derive(Debug, Clone)]
pub struct ModeProfile {
    pub mode: AgentMode,
    pub name: &'static str,
    pub description: &'static str,
    pub system_prompt_template: &'static str,
    pub tool_policy: ToolPolicy,
    pub llm_defaults: LlmDefaults,
    pub limits: ModeLimits,
}

/// Engine de Modos - gerencia perfis e resolução
pub struct ModeEngine {
    profiles: HashMap<AgentMode, ModeProfile>,
    channel_defaults: HashMap<String, AgentMode>,
}

impl ModeEngine {
    /// Cria um novo ModeEngine com perfis padrão
    pub fn new() -> Self {
        let mut profiles = HashMap::new();
        
        // Perfil AUTO
        profiles.insert(AgentMode::Auto, ModeProfile {
            mode: AgentMode::Auto,
            name: "Auto",
            description: "Decide automaticamente baseado no contexto",
            system_prompt_template: "Você é o GarraIA, um assistente de IA inteligente.",
            tool_policy: ToolPolicy::default(),
            llm_defaults: LlmDefaults::default(),
            limits: ModeLimits::default(),
        });
        
        // Perfil SEARCH
        profiles.insert(AgentMode::Search, ModeProfile {
            mode: AgentMode::Search,
            name: "Search",
            description: "Busca e inspeção sem modificar",
            system_prompt_template: "Você está em modo de busca. Analise o repositório e forneça informações relevantes.",
            tool_policy: ToolPolicy {
                allowed: vec!["file_read".to_string(), "repo_search".to_string()],
                denied: vec!["file_write".to_string()],
                read_only: vec!["bash".to_string()],
                required: None,
            },
            llm_defaults: LlmDefaults {
                temperature: 0.3,
                max_tokens: 2048,
                top_p: 0.9,
            },
            limits: ModeLimits {
                max_loops: 5,
                timeout_secs: 60,
            },
        });
        
        // Perfil ARCHITECT
        profiles.insert(AgentMode::Architect, ModeProfile {
            mode: AgentMode::Architect,
            name: "Architect",
            description: "Análise de arquitetura e design",
            system_prompt_template: "Você é um arquiteto de software. Analise a arquitetura e sugira melhorias.",
            tool_policy: ToolPolicy {
                allowed: vec!["file_read".to_string(), "repo_search".to_string()],
                denied: vec!["file_write".to_string(), "bash".to_string()],
                read_only: vec![],
                required: None,
            },
            llm_defaults: LlmDefaults {
                temperature: 0.5,
                max_tokens: 4096,
                top_p: 0.9,
            },
            limits: ModeLimits {
                max_loops: 3,
                timeout_secs: 60,
            },
        });
        
        // Perfil CODE
        profiles.insert(AgentMode::Code, ModeProfile {
            mode: AgentMode::Code,
            name: "Code",
            description: "Desenvolvimento e implementação",
            system_prompt_template: "Você é um desenvolvedor. Escreva código limpo, eficiente e bem documentado.",
            tool_policy: ToolPolicy {
                allowed: vec!["file_read".to_string(), "file_write".to_string(), "bash".to_string()],
                denied: vec![],
                read_only: vec![],
                required: None,
            },
            llm_defaults: LlmDefaults {
                temperature: 0.4,
                max_tokens: 8192,
                top_p: 0.95,
            },
            limits: ModeLimits {
                max_loops: 15,
                timeout_secs: 300,
            },
        });
        
        // Perfil ASK (padrão para Telegram)
        profiles.insert(AgentMode::Ask, ModeProfile {
            mode: AgentMode::Ask,
            name: "Ask",
            description: "Consulta e explicação",
            system_prompt_template: "Você é o GarraIA, um assistente prestativo. Forneça explicações claras.",
            tool_policy: ToolPolicy {
                allowed: vec![],
                denied: vec![],
                read_only: vec![],
                required: None,
            },
            llm_defaults: LlmDefaults {
                temperature: 0.7,
                max_tokens: 2048,
                top_p: 1.0,
            },
            limits: ModeLimits {
                max_loops: 1,
                timeout_secs: 30,
            },
        });
        
        // Perfil DEBUG
        profiles.insert(AgentMode::Debug, ModeProfile {
            mode: AgentMode::Debug,
            name: "Debug",
            description: "Debugging e análise de erros",
            system_prompt_template: "Você é um especialista em debugging. Analise erros e forneça soluções.",
            tool_policy: ToolPolicy {
                allowed: vec!["file_read".to_string(), "bash".to_string(), "repo_search".to_string()],
                denied: vec!["file_write".to_string()],
                read_only: vec![],
                required: None,
            },
            llm_defaults: LlmDefaults {
                temperature: 0.2,
                max_tokens: 4096,
                top_p: 0.9,
            },
            limits: ModeLimits {
                max_loops: 10,
                timeout_secs: 120,
            },
        });
        
        // Perfil ORCHESTRATOR
        profiles.insert(AgentMode::Orchestrator, ModeProfile {
            mode: AgentMode::Orchestrator,
            name: "Orchestrator",
            description: "Execução multi-etapas com planos",
            system_prompt_template: "Você é um orquestrador de tarefas. Planeje, execute e valide múltiplas etapas.",
            tool_policy: ToolPolicy::default(),
            llm_defaults: LlmDefaults {
                temperature: 0.5,
                max_tokens: 8192,
                top_p: 0.95,
            },
            limits: ModeLimits {
                max_loops: 20,
                timeout_secs: 600,
            },
        });
        
        // Perfil REVIEW
        profiles.insert(AgentMode::Review, ModeProfile {
            mode: AgentMode::Review,
            name: "Review",
            description: "Revisão de código e diffs",
            system_prompt_template: "Você é um revisor de código. Analise mudanças e forneça feedback construtivo.",
            tool_policy: ToolPolicy {
                allowed: vec!["file_read".to_string(), "git_diff".to_string()],
                denied: vec!["file_write".to_string(), "bash".to_string()],
                read_only: vec![],
                required: None,
            },
            llm_defaults: LlmDefaults {
                temperature: 0.3,
                max_tokens: 4096,
                top_p: 0.9,
            },
            limits: ModeLimits {
                max_loops: 5,
                timeout_secs: 60,
            },
        });
        
        // Perfil EDIT
        profiles.insert(AgentMode::Edit, ModeProfile {
            mode: AgentMode::Edit,
            name: "Edit",
            description: "Edição focada",
            system_prompt_template: "Você está em modo de edição. Faça modificações precisas e eficientes.",
            tool_policy: ToolPolicy {
                allowed: vec!["file_read".to_string(), "file_write".to_string()],
                denied: vec!["bash".to_string()],
                read_only: vec![],
                required: None,
            },
            llm_defaults: LlmDefaults {
                temperature: 0.3,
                max_tokens: 4096,
                top_p: 0.9,
            },
            limits: ModeLimits {
                max_loops: 5,
                timeout_secs: 60,
            },
        });
        
        // Channel defaults - Telegram deve manter Ask para compatibilidade
        let mut channel_defaults = HashMap::new();
        channel_defaults.insert("telegram".to_string(), AgentMode::Ask);
        channel_defaults.insert("discord".to_string(), AgentMode::Ask);
        channel_defaults.insert("whatsapp".to_string(), AgentMode::Ask);
        channel_defaults.insert("web".to_string(), AgentMode::Auto);
        channel_defaults.insert("api".to_string(), AgentMode::Auto);
        channel_defaults.insert("continue".to_string(), AgentMode::Auto);
        
        Self {
            profiles,
            channel_defaults,
        }
    }
    
    /// Obtém o perfil de um modo
    pub fn get_profile(&self, mode: AgentMode) -> &ModeProfile {
        self.profiles.get(&mode).expect("Modo não encontrado")
    }
    
    /// Lista todos os modos disponíveis
    pub fn list_modes(&self) -> Vec<(AgentMode, &'static str, &'static str)> {
        self.profiles.values()
            .map(|p| (p.mode, p.name, p.description))
            .collect()
    }
    
    /// Resolve o modo final considerando precedência
    /// Precedência: Header > Comando > Canal > Usuário > Default
    pub fn resolve_mode(
        &self,
        header_mode: Option<&str>,
        command_mode: Option<&str>,
        channel: &str,
    ) -> AgentMode {
        // 1. Header HTTP tem maior precedência
        if let Some(mode_str) = header_mode {
            if let Ok(mode) = mode_str.parse::<AgentMode>() {
                tracing::debug!("Modo resolvido por header: {:?}", mode);
                return mode;
            }
        }
        
        // 2. Comando do chat
        if let Some(mode_str) = command_mode {
            if let Ok(mode) = mode_str.parse::<AgentMode>() {
                tracing::debug!("Modo resolvido por comando: {:?}", mode);
                return mode;
            }
        }
        
        // 3. Default por canal
        if let Some(&default) = self.channel_defaults.get(channel) {
            tracing::debug!("Modo resolvido por canal {}: {:?}", channel, default);
            return default;
        }
        
        // 4. Padrão global
        tracing::debug!("Modo usando padrão: Ask");
        AgentMode::Ask
    }
    
    /// Obtém o default do canal
    pub fn get_channel_default(&self, channel: &str) -> AgentMode {
        self.channel_defaults.get(channel).copied().unwrap_or(AgentMode::Ask)
    }
    
    /// Resolve o modo automaticamente baseado no conteúdo da mensagem
    /// Usa heurísticas determinísticas para determinar o modo apropriado
    pub fn resolve_auto_mode(&self, message: &str) -> AgentMode {
        let msg_lower = message.to_lowercase();
        
        // Debug patterns - erros, stacktrace, panic
        if Self::matches_any(&msg_lower, &[
            "erro", "error", "bug", "panic", "stacktrace", "exception",
            "falha", "não funciona", "não está funcionando", "não roda",
            "debug", "problema", "issue", "crash", "travou", "trava"
        ]) {
            tracing::debug!("Auto mode resolved: debug (error patterns)");
            return AgentMode::Debug;
        }
        
        // Code patterns - criar, implementar, refatorar
        if Self::matches_any(&msg_lower, &[
            "criar", "implementar", "refatorar", "escrever código", "make new",
            "add function", "add method", "create file", "criar arquivo",
            "modificar", "alterar código", "update code", "fix code",
            "build", "compile", "rust", "typescript", "javascript",
            ".rs", "Cargo.toml", "package.json", "main.py"
        ]) {
            tracing::debug!("Auto mode resolved: code (code patterns)");
            return AgentMode::Code;
        }
        
        // Search patterns - buscar, encontrar, onde está
        if Self::matches_any(&msg_lower, &[
            "buscar", "procurar", "encontrar", "onde está", "onde fica",
            "search", "find", "look for", "show me", "list files",
            "liste", "mostrar arquivos", "pesquisar"
        ]) && !Self::matches_any(&msg_lower, &["criar", "implementar"]) {
            tracing::debug!("Auto mode resolved: search (search patterns)");
            return AgentMode::Search;
        }
        
        // Architect patterns - arquitetura, design, roadmap
        if Self::matches_any(&msg_lower, &[
            "arquitetura", "design", "roadmap", "planejamento", "estrutura",
            "architecture", "design", "plan", "structure", "refactor",
            "melhorar o design", "sugestão de arquitetura"
        ]) {
            tracing::debug!("Auto mode resolved: architect (architecture patterns)");
            return AgentMode::Architect;
        }
        
        // Review patterns - revisar, review, diff, analisar
        if Self::matches_any(&msg_lower, &[
            "revisar", "review", "analisar código", "check code",
            "diff", "pull request", "merge", "analisar changes",
            "code review", "revisão", "verificar código"
        ]) {
            tracing::debug!("Auto mode resolved: review (review patterns)");
            return AgentMode::Review;
        }
        
        // Edit patterns - editar, modificar arquivo específico
        if Self::matches_any(&msg_lower, &[
            "editar", "alterar arquivo", "modificar arquivo", "update file",
            "change file", "corrigir arquivo", "fix in"
        ]) {
            tracing::debug!("Auto mode resolved: edit (edit patterns)");
            return AgentMode::Edit;
        }
        
        // Ask patterns (default) - explicar, o que é, como funciona
        if Self::matches_any(&msg_lower, &[
            "o que é", "como funciona", "explique", "explicar",
            "what is", "how does", "explain", "tell me about",
            "definição", "conceito", "entender", "difference between"
        ]) {
            tracing::debug!("Auto mode resolved: ask (question patterns)");
            return AgentMode::Ask;
        }
        
        // Orchestrator patterns - múltiplas tarefas
        if Self::matches_any(&msg_lower, &[
            "faça isso e aquilo", "faça múltiplas", "várias tarefas",
            "execute isso e", "complete workflow", "pipeline",
            "atualize tudo", "refatore e teste", "crie e configure"
        ]) {
            tracing::debug!("Auto mode resolved: orchestrator (multi-task patterns)");
            return AgentMode::Orchestrator;
        }
        
        // Default para Ask (compatível com Telegram)
        tracing::debug!("Auto mode resolved: ask (default)");
        AgentMode::Ask
    }
    
    /// Helper para verificar múltiplos padrões
    fn matches_any(text: &str, patterns: &[&str]) -> bool {
        patterns.iter().any(|p| text.contains(p))
    }
}

impl Default for ModeEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Obtém o engine de modos global
pub fn get_mode_engine() -> ModeEngine {
    ModeEngine::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_mode_parsing() {
        assert_eq!("code".parse::<AgentMode>().unwrap(), AgentMode::Code);
        assert_eq!("ask".parse::<AgentMode>().unwrap(), AgentMode::Ask);
        assert_eq!("DEBUG".parse::<AgentMode>().unwrap(), AgentMode::Debug);
        assert!("invalid".parse::<AgentMode>().is_err());
    }
    
    #[test]
    fn test_mode_engine() {
        let engine = ModeEngine::new();
        
        // Test list modes
        let modes = engine.list_modes();
        assert_eq!(modes.len(), 9);
        
        // Test get profile
        let profile = engine.get_profile(AgentMode::Code);
        assert_eq!(profile.name, "Code");
        
        // Test resolve mode - header takes precedence
        let mode = engine.resolve_mode(Some("debug"), None, "telegram");
        assert_eq!(mode, AgentMode::Debug);
        
        // Test resolve mode - channel default
        let mode = engine.resolve_mode(None, None, "telegram");
        assert_eq!(mode, AgentMode::Ask);
        
        // Test resolve mode - web defaults to auto
        let mode = engine.resolve_mode(None, None, "web");
        assert_eq!(mode, AgentMode::Auto);
    }
}
