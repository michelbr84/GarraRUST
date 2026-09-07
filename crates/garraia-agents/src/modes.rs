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
use tracing::debug;

/// Enum de modos de execução do agente.
/// Cada modo define uma estratégia diferente de comportamento.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
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
    #[default]
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
    #[allow(clippy::should_implement_trait)]
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

/// Decide se uma ferramenta pode rodar, para um modo escolhido (#988).
///
/// # Sem modo escolhido, sem politica
///
/// [`ToolGate::sem_politica`] permite tudo, e e o que uma sessao sem `/mode`
/// recebe. **Nao** e o perfil de `Ask` — o default do enum e `Ask`, e `ask`
/// nega `file_write`, entao tratar "nao escolheu" como "escolheu Ask"
/// quebraria escrita por padrao em todo canal, CLI incluso. O criterio de
/// aceite da #988 pede que o comportamento padrao nao regrida, e o
/// comportamento padrao de hoje e nao ter politica.
///
/// # Ferramenta MCP nao e barrada por whitelist
///
/// Tool de servidor MCP se chama `{servidor}__{tool}`
/// (`mcp/tool_bridge.rs`). Os whitelists de `search`, `architect`, `debug`,
/// `review` e `edit` listam so nomes nativos, entao aplicar `whitelist_mode`
/// ao pe da letra **derrubaria toda integracao MCP** nesses cinco modos, em
/// silencio — o usuario veria a ferramenta sumir sem mensagem nenhuma.
///
/// Entao ferramenta MCP passa pelo whitelist e continua sujeita ao `denied`.
///
/// **A consequencia precisa ser dita**: um modo somente-leitura nao restringe
/// ferramenta MCP. Se o operador conectou um servidor MCP que escreve arquivo,
/// o modo `search` nao o impede. Isso e um limite conhecido desta versao, nao
/// um descuido — a alternativa era quebrar MCP para todo mundo hoje. Um
/// whitelist que entenda servidor MCP precisa ser desenhado com o dono.
#[derive(Debug, Clone)]
pub struct ToolGate {
    /// O perfil que vale neste turno, se algum vale.
    ///
    /// Guarda o perfil inteiro, e nao so a `ToolPolicy`, porque o mesmo perfil
    /// tambem decide os limites de execucao (`ModeLimits`) — o #979 pede que
    /// o `max_tool_loops` e o timeout do modo valham no lugar dos fixos, e
    /// carregar duas metades do mesmo perfil por caminhos diferentes e como
    /// elas divergem.
    profile: Option<ModeProfile>,
    /// O nome que aparece na recusa. Para `auto`, e o modo **deduzido**, que e
    /// o que explica a restricao para quem pediu a ferramenta.
    nome: Option<String>,
}

/// Separador que o `tool_bridge` usa entre servidor e ferramenta.
const SEPARADOR_MCP: &str = "__";

impl ToolGate {
    /// O portao aberto: nenhuma politica, tudo permitido.
    pub fn sem_politica() -> Self {
        Self {
            profile: None,
            nome: None,
        }
    }

    /// O portao do perfil de um modo.
    pub fn from_profile(profile: &ModeProfile) -> Self {
        Self {
            nome: Some(profile.name.clone()),
            profile: Some(profile.clone()),
        }
    }

    /// O portao para um contexto de execucao, **sem** olhar a mensagem.
    ///
    /// Sem modo escolhido, portao aberto. Ver o docblock do tipo. Com `auto`
    /// escolhido nao da para resolver aqui — `auto` depende do que a pessoa
    /// escreveu —, entao use [`Self::para_o_turno`] onde o texto existir.
    pub fn from_exec(exec: &crate::exec_context::ExecContext) -> Self {
        match exec.agent_mode.as_deref() {
            Some(nome) => Self::for_mode_name(nome),
            None => Self::sem_politica(),
        }
    }

    /// O portao do turno: igual ao [`Self::from_exec`], exceto em `auto`.
    ///
    /// # Escolher `auto` e consentir com a delegacao
    ///
    /// O #988 estabeleceu que modo **deduzido** nao liga politica: quem nunca
    /// digitou `/mode` nao pode perder `file_write` porque uma heuristica achou
    /// que a pergunta parecia busca. `auto` nao contradiz isso — e o caso em
    /// que a pessoa escolheu, explicitamente, deixar o roteador decidir. A
    /// escolha esta em ter digitado `/mode auto`; o que a heuristica faz depois
    /// e executar essa escolha, nao substitui-la.
    ///
    /// Ate aqui `auto` resolvia para `ModeProfile::default_auto()`, cuja
    /// `ToolPolicy` e a vazia — ou seja, escolher `auto` nao mudava nada. Era o
    /// buraco do #979.
    ///
    /// # So a heuristica, aqui
    ///
    /// O estagio de LLM do roteador precisa de config (`auto_router_llm_enabled`,
    /// `auto_router_model`) que o runtime nao tem e nao deveria ter. Quem tem a
    /// config — hoje o gateway — pode resolver `auto` para um modo concreto
    /// antes de chamar e passar esse nome no `ExecContext`; este caminho e o
    /// piso, e vale para a CLI, que monta o proprio runtime.
    pub fn para_o_turno(exec: &crate::exec_context::ExecContext, user_text: &str) -> Self {
        match exec.agent_mode.as_deref() {
            Some(nome) if AgentMode::from_str(nome) == Some(AgentMode::Auto) => {
                match crate::auto_router::classify_heuristic(user_text) {
                    Some(modo) => {
                        let perfil = ModeProfile::from_mode(modo);
                        debug!(
                            modo_deduzido = %perfil.name,
                            "auto: perfil do turno resolvido pela mensagem"
                        );
                        Self::from_profile(&perfil)
                    }
                    // Nao deu para dizer qual: portao aberto, que e o
                    // comportamento que `auto` sempre teve. Fechar por duvida
                    // bloquearia ferramenta que a pessoa podia usar.
                    None => Self::sem_politica(),
                }
            }
            Some(nome) => Self::for_mode_name(nome),
            None => Self::sem_politica(),
        }
    }

    /// Resolve o modo pelo nome; nome desconhecido vira portao aberto, e nao
    /// portao fechado — recusar tudo porque alguem digitou errado seria pior
    /// que ignorar o modo.
    pub fn for_mode_name(nome: &str) -> Self {
        match AgentMode::from_str(nome) {
            Some(modo) => Self::from_profile(&ModeProfile::from_mode(modo)),
            None => Self::sem_politica(),
        }
    }

    /// Os limites do modo que vale neste turno, se algum vale (#979).
    pub fn limites(&self) -> Option<&ModeLimits> {
        self.profile.as_ref().map(|p| &p.limits)
    }

    /// O nome do modo que esta valendo — o deduzido, quando o escolhido foi
    /// `auto`. E o que a recusa precisa dizer para a explicacao fazer sentido.
    pub fn nome_do_modo(&self) -> Option<&str> {
        self.nome.as_deref()
    }

    /// A ferramenta pode rodar?
    pub fn permite(&self, tool_name: &str) -> bool {
        let Some(p) = self.profile.as_ref().map(|p| &p.tool_policy) else {
            return true;
        };

        // `denied` vale sempre, inclusive para MCP.
        if p.denied.iter().any(|t| t == tool_name) {
            return false;
        }

        if p.whitelist_mode {
            // Whitelist vazia nao quer dizer "nada permitido" — quer dizer que
            // o perfil nao restringiu.
            if p.allowed.is_empty() {
                return true;
            }
            if tool_name.contains(SEPARADOR_MCP) {
                return true;
            }
            return p.allowed.iter().any(|t| t == tool_name);
        }

        true
    }

    /// A mensagem que o modelo recebe quando pede uma ferramenta barrada.
    ///
    /// Diz **por que**, e nao so que falhou: sem isso o modelo tende a tentar
    /// de novo a mesma coisa, gastando o orcamento de chamadas.
    ///
    /// Nao manda o modelo trocar de modo. Trocar de modo e do usuario — o
    /// modelo nao executa `/mode`, e a versao anterior desta frase sugeria
    /// `/mode code` mesmo quando `code` nao era o modo que liberava a
    /// ferramenta.
    pub fn recusa(tool_name: &str, modo: &str) -> String {
        format!(
            "A ferramenta `{tool_name}` nao e permitida no modo `{modo}`. \
             Siga sem ela, ou peca ao usuario para trocar de modo."
        )
    }
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
        engine
            .channel_defaults
            .insert("telegram".to_string(), "ask".to_string());
        engine
            .channel_defaults
            .insert("web".to_string(), "auto".to_string());
        engine
            .channel_defaults
            .insert("vscode".to_string(), "auto".to_string());
        engine
            .channel_defaults
            .insert("discord".to_string(), "ask".to_string());
        engine
            .channel_defaults
            .insert("whatsapp".to_string(), "ask".to_string());

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
        if let Some(mode_str) = header_mode
            && let Some(mode) = AgentMode::from_str(mode_str)
        {
            return mode;
        }

        // 2. Modo salvo na sessão (via comando /mode)
        if let Some(mode_str) = session_mode
            && let Some(mode) = AgentMode::from_str(mode_str)
        {
            return mode;
        }

        // 3. Default por canal
        if let Some(channel_str) = channel
            && let Some(default_mode) = self
                .channel_defaults
                .get(channel_str.to_lowercase().as_str())
            && let Some(mode) = AgentMode::from_str(default_mode)
        {
            return mode;
        }

        // 4. Default global
        AgentMode::default()
    }

    /// Resolve o modo automaticamente usando heurísticas (GAR-M3-1)
    pub fn resolve_auto_mode(&self, user_input: &str) -> AgentMode {
        let input_lower = user_input.to_lowercase();

        // Contains file path → search ou debug
        if input_lower.contains("c:\\")
            || input_lower.contains("g:\\")
            || input_lower.contains("/home/")
            || input_lower.contains("src/")
            || input_lower.contains("crates/")
        {
            // Dependendo do verbo
            if input_lower.contains("erro")
                || input_lower.contains("panic")
                || input_lower.contains("stacktrace")
                || input_lower.contains("bug")
            {
                return AgentMode::Debug;
            }
            return AgentMode::Search;
        }

        // Palavras-chave de implementação → code
        let code_keywords = [
            "implementar",
            "criar arquivo",
            "refatorar",
            "escrever código",
            "implement",
            "create file",
            "refactor",
            "write code",
            "crie",
            "faça",
            "make",
            "build",
            "add",
            "fix",
        ];
        if code_keywords.iter().any(|kw| input_lower.contains(kw)) {
            return AgentMode::Code;
        }

        // Palavras-chave de perguntas → ask
        let ask_keywords = [
            "o que é",
            "explique",
            "como funciona",
            "defina",
            "what is",
            "explain",
            "how does",
            "define",
            "?",
            "qual é",
            "quais são",
        ];
        if ask_keywords.iter().any(|kw| input_lower.contains(kw)) {
            return AgentMode::Ask;
        }

        // Palavras-chave de debug → debug
        let debug_keywords = [
            "erro",
            "stacktrace",
            "panic",
            "exception",
            "falha",
            "error",
            "stack trace",
            "crash",
            "bug",
            "problema",
        ];
        if debug_keywords.iter().any(|kw| input_lower.contains(kw)) {
            return AgentMode::Debug;
        }

        // Palavras-chave de arquitetura → architect
        let arch_keywords = [
            "roadmap",
            "design",
            "arquitetura",
            "planejamento",
            "roadmap",
            "design",
            "architecture",
            "plan",
        ];
        if arch_keywords.iter().any(|kw| input_lower.contains(kw)) {
            return AgentMode::Architect;
        }

        // Palavras-chave de review → review
        let review_keywords = [
            "review",
            "revisar",
            "analisar diff",
            "analise",
            "revise",
            "analyze",
            "check",
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
        self.channel_defaults
            .insert(channel.to_lowercase(), mode.to_string());
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

        if let Some(profile) = self.get_profile(mode.as_str())
            && let Some(template) = &profile.system_prompt_template
        {
            return template.clone();
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
    use super::ToolGate;

    /// A regressao mais provavel do lote, e a razao de `None` existir.
    ///
    /// O default do enum e `Ask`, e `ask` nega `file_write`. Se sessao sem
    /// modo resolvesse para `Ask`, ligar a politica quebraria escrita por
    /// padrao em todo canal — o CLI nunca seta modo.
    #[test]
    fn sem_modo_escolhido_permite_tudo() {
        let g = ToolGate::sem_politica();
        assert!(g.permite("file_write"));
        assert!(g.permite("bash"));
        assert!(g.permite("qualquer_coisa"));
    }

    /// E o modo `ask`, quando **escolhido**, nega mesmo.
    #[test]
    fn modo_ask_escolhido_nega_escrita() {
        let g = ToolGate::for_mode_name("ask");
        assert!(!g.permite("file_write"), "ask deveria negar file_write");
        assert!(g.permite("file_read"));
    }

    /// Modo somente-leitura barra escrita e bash.
    #[test]
    fn modo_search_barra_escrita_e_bash() {
        let g = ToolGate::for_mode_name("search");
        assert!(!g.permite("file_write"));
        assert!(!g.permite("bash"));
        assert!(g.permite("file_read"));
        assert!(g.permite("repo_search"));
    }

    /// Modo `code` continua podendo tudo — o criterio de aceite da #988 sobre
    /// nao regredir o caminho de escrita.
    #[test]
    fn modo_code_nao_e_restringido() {
        let g = ToolGate::for_mode_name("code");
        assert!(g.permite("file_write"));
        assert!(g.permite("bash"));
    }

    /// Ferramenta MCP passa pelo whitelist.
    ///
    /// Os whitelists listam so nomes nativos; aplicar ao pe da letra
    /// derrubaria toda integracao MCP em cinco dos nove modos, em silencio.
    #[test]
    fn ferramenta_mcp_nao_e_barrada_por_whitelist() {
        for modo in ["search", "architect", "debug", "review", "edit"] {
            let g = ToolGate::for_mode_name(modo);
            assert!(
                g.permite("meu_servidor__consulta"),
                "{modo} barrou ferramenta MCP"
            );
        }
    }

    /// Mas `denied` vale para MCP tambem — a lista explicita ganha.
    #[test]
    fn denied_vale_inclusive_para_mcp() {
        let g = ToolGate::from_profile(&ModeProfile {
            tool_policy: ToolPolicy {
                allowed: vec![],
                denied: vec!["servidor__perigosa".to_string()],
                required: vec![],
                whitelist_mode: true,
            },
            ..ModeProfile::from_mode(AgentMode::Code)
        });
        assert!(!g.permite("servidor__perigosa"));
        assert!(g.permite("servidor__inofensiva"));
    }

    /// Nome de modo desconhecido abre o portao, em vez de fechar. Recusar tudo
    /// porque alguem digitou errado seria pior que ignorar o modo.
    #[test]
    fn modo_desconhecido_nao_fecha_o_portao() {
        let g = ToolGate::for_mode_name("modo_que_nao_existe");
        assert!(g.permite("file_write"));
    }

    /// Whitelist vazia quer dizer "nao restringiu", e nao "nada permitido".
    #[test]
    fn whitelist_vazia_nao_e_negacao_total() {
        let g = ToolGate::from_profile(&ModeProfile {
            tool_policy: ToolPolicy {
                allowed: vec![],
                denied: vec![],
                required: vec![],
                whitelist_mode: true,
            },
            ..ModeProfile::from_mode(AgentMode::Code)
        });
        assert!(g.permite("file_write"));
    }

    /// `/mode auto` passa a restringir de verdade (#979).
    ///
    /// Ate aqui `auto` resolvia para `default_auto()`, cuja `ToolPolicy` e a
    /// vazia: escolher `auto` nao mudava nada. Agora o perfil sai da mensagem.
    #[test]
    fn auto_escolhido_resolve_o_perfil_pela_mensagem() {
        let exec = crate::exec_context::ExecContext::with_mode(Some("auto".into()));

        // Mensagem de busca -> perfil `search`, que nega escrita.
        let g = ToolGate::para_o_turno(
            &exec,
            "qual arquivo tem o handler de login? procura por ele",
        );
        assert_eq!(g.nome_do_modo(), Some("search"));
        assert!(!g.permite("file_write"), "search precisa negar escrita");

        // Mensagem de codigo -> perfil `code`, que permite.
        let g = ToolGate::para_o_turno(&exec, "escreve uma funcao que soma dois numeros");
        assert_eq!(g.nome_do_modo(), Some("code"));
        assert!(g.permite("file_write"));
    }

    /// Sem conseguir deduzir, `auto` fica aberto — nao fechado.
    ///
    /// Fechar por duvida bloquearia ferramenta que a pessoa podia usar, e o
    /// comportamento que `auto` sempre teve foi permitir tudo.
    #[test]
    fn auto_sem_deducao_clara_fica_aberto() {
        let exec = crate::exec_context::ExecContext::with_mode(Some("auto".into()));
        let g = ToolGate::para_o_turno(&exec, "oi");
        assert_eq!(g.nome_do_modo(), None);
        assert!(g.permite("file_write"));
        assert!(g.permite("bash"));
    }

    /// E nao escolher modo nenhum continua sem politica (#988).
    ///
    /// Esta e a linha que o #979 nao pode cruzar: deduzir para quem **escolheu
    /// `auto`** e executar a escolha; deduzir para quem nao escolheu nada seria
    /// restringir sem consentimento. A mensagem aqui e a mesma que resolveria
    /// para `search` no teste acima.
    #[test]
    fn sem_modo_escolhido_a_mensagem_nao_restringe() {
        let exec = crate::exec_context::ExecContext::default();
        let g = ToolGate::para_o_turno(
            &exec,
            "qual arquivo tem o handler de login? procura por ele",
        );
        assert_eq!(g.nome_do_modo(), None);
        assert!(
            g.permite("file_write"),
            "quem nao escolheu modo nao pode perder file_write por heuristica"
        );
    }

    /// Modo concreto escolhido ignora a mensagem, como sempre.
    #[test]
    fn modo_concreto_nao_e_reinterpretado_pela_mensagem() {
        let exec = crate::exec_context::ExecContext::with_mode(Some("search".into()));
        let g = ToolGate::para_o_turno(&exec, "escreve uma funcao que soma dois numeros");
        assert_eq!(g.nome_do_modo(), Some("search"));
        assert!(!g.permite("file_write"), "a escolha do usuario manda");
    }

    /// Os limites do modo chegam ao portao (#979 item 4).
    #[test]
    fn o_portao_carrega_os_limites_do_modo() {
        let g = ToolGate::for_mode_name("code");
        let limites = g.limites().expect("modo concreto tem limites");
        assert_eq!(
            limites.max_tool_loops,
            ModeProfile::from_mode(AgentMode::Code)
                .limits
                .max_tool_loops
        );

        assert!(
            ToolGate::sem_politica().limites().is_none(),
            "sem modo nao ha limites de modo"
        );
    }

    /// A recusa diz por que, e o que fazer. Sem isso o modelo tende a repetir
    /// a mesma chamada e gastar o orcamento.
    #[test]
    fn a_recusa_diz_o_motivo_e_a_saida() {
        let m = ToolGate::recusa("file_write", "search");
        assert!(m.contains("file_write"), "diz qual ferramenta");
        assert!(m.contains("search"), "diz qual modo barrou");
        assert!(
            m.contains("Siga sem ela") && m.contains("usuario"),
            "diz o que fazer, e que a troca de modo e do usuario"
        );
        // O modelo nao executa `/mode`, e `code` nao e necessariamente o modo
        // que libera a ferramenta barrada.
        assert!(
            !m.contains("/mode code"),
            "nao prescreve um modo especifico"
        );
    }

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
