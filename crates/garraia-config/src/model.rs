use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub gateway: GatewayConfig,

    #[serde(default)]
    pub channels: HashMap<String, ChannelConfig>,

    #[serde(default)]
    pub llm: HashMap<String, LlmProviderConfig>,

    #[serde(default)]
    pub embeddings: HashMap<String, EmbeddingProviderConfig>,

    #[serde(default)]
    pub memory: MemoryConfig,

    #[serde(default)]
    pub agent: AgentConfig,

    #[serde(default)]
    pub data_dir: Option<PathBuf>,

    #[serde(default)]
    pub log_level: Option<String>,

    #[serde(default)]
    pub mcp: HashMap<String, McpServerConfig>,

    /// Named agent configurations for multi-agent routing.
    /// If empty, the single `agent:` block is used as "default".
    #[serde(default)]
    pub agents: HashMap<String, NamedAgentConfig>,

    /// Voice (TTS/STT) configuration.
    #[serde(default)]
    pub voice: VoiceConfig,

    /// Per-type timeout configuration.
    #[serde(default)]
    pub timeouts: TimeoutConfig,

    /// GAR-261: File-system glob and ignore configuration.
    #[serde(default)]
    pub fs: FsConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            gateway: GatewayConfig::default(),
            channels: HashMap::new(),
            llm: HashMap::new(),
            embeddings: HashMap::new(),
            memory: MemoryConfig::default(),
            agent: AgentConfig::default(),
            data_dir: None,
            log_level: Some("info".to_string()),
            mcp: HashMap::new(),
            agents: HashMap::new(),
            voice: VoiceConfig::default(),
            timeouts: TimeoutConfig::default(),
            fs: FsConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    #[serde(default = "default_host")]
    pub host: String,

    #[serde(default = "default_port")]
    pub port: u16,

    #[serde(default)]
    pub api_key: Option<String>,

    #[serde(default)]
    pub rate_limit: RateLimitConfig,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            api_key: None,
            rate_limit: RateLimitConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    #[serde(default = "default_rate_per_second")]
    pub per_second: u64,

    #[serde(default = "default_rate_burst_size")]
    pub burst_size: u32,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            per_second: default_rate_per_second(),
            burst_size: default_rate_burst_size(),
        }
    }
}

fn default_rate_per_second() -> u64 {
    1
}

fn default_rate_burst_size() -> u32 {
    60
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    3888
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    #[serde(rename = "type")]
    pub channel_type: String,

    pub enabled: Option<bool>,

    #[serde(flatten)]
    pub settings: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmProviderConfig {
    pub provider: String,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,

    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingProviderConfig {
    pub provider: String,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub dimensions: Option<usize>,

    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    #[serde(default = "default_memory_enabled")]
    pub enabled: bool,

    pub embedding_provider: Option<String>,

    #[serde(default)]
    pub shared_continuity: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: default_memory_enabled(),
            embedding_provider: None,
            shared_continuity: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentConfig {
    pub system_prompt: Option<String>,
    pub default_provider: Option<String>,
    pub max_tokens: Option<u32>,
    pub max_context_tokens: Option<usize>,
    /// Maximum total tool calls per task (default: 50).
    /// Increase for complex agentic workflows.
    pub max_tool_calls: Option<usize>,
    /// GAR-210: Ordered list of fallback provider keys to try when the primary fails
    /// with a retryable error (429, 502, 503). Example: ["openrouter", "anthropic"]
    #[serde(default)]
    pub fallback_providers: Vec<String>,
    /// GAR-187: When true, bash commands matching CONFIRM_LIST require user approval
    /// before execution. The agent loop pauses and waits for "sim"/"yes" before proceeding.
    /// Default: false (opt-in).
    #[serde(default)]
    pub tool_confirmation_enabled: bool,
}

/// A named agent configuration for multi-agent routing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedAgentConfig {
    /// Which LLM provider key (from `llm:` section) to use.
    pub provider: Option<String>,
    /// Override model name (otherwise uses the provider's default).
    pub model: Option<String>,
    /// Custom system prompt for this agent.
    pub system_prompt: Option<String>,
    /// Max output tokens.
    pub max_tokens: Option<u32>,
    /// Max context window tokens.
    pub max_context_tokens: Option<usize>,
    /// Restrict which tools this agent can use (empty = all tools).
    #[serde(default)]
    pub tools: Vec<String>,
}

fn default_memory_enabled() -> bool {
    true
}

// ─── Timeout Config ────────────────────────────────────────────────────────

fn default_llm_timeout() -> u64 {
    120
}
fn default_tts_timeout() -> u64 {
    120
}
fn default_stt_timeout() -> u64 {
    30
}
fn default_mcp_timeout() -> u64 {
    60
}
fn default_health_timeout() -> u64 {
    5
}

/// Per-type timeout configuration.
///
/// ```yaml
/// timeouts:
///   llm:
///     default_secs: 120  # LLM responses podem demorar; 30s era curto demais
///   tts:
///     default_secs: 120
///   stt:
///     default_secs: 30
///   mcp:
///     default_secs: 60
///   health:
///     default_secs: 5
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutConfig {
    #[serde(default)]
    pub llm: TypeTimeout,
    #[serde(default)]
    pub tts: TypeTimeout,
    #[serde(default)]
    pub stt: TypeTimeout,
    #[serde(default)]
    pub mcp: TypeTimeout,
    #[serde(default)]
    pub health: TypeTimeout,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            llm: TypeTimeout {
                default_secs: default_llm_timeout(),
            },
            tts: TypeTimeout {
                default_secs: default_tts_timeout(),
            },
            stt: TypeTimeout {
                default_secs: default_stt_timeout(),
            },
            mcp: TypeTimeout {
                default_secs: default_mcp_timeout(),
            },
            health: TypeTimeout {
                default_secs: default_health_timeout(),
            },
        }
    }
}

/// Timeout for a specific type of operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypeTimeout {
    #[serde(default = "default_llm_timeout")]
    pub default_secs: u64,
}

impl Default for TypeTimeout {
    fn default() -> Self {
        Self { default_secs: default_llm_timeout() }
    }
}

// ─── FsConfig — GAR-261 ────────────────────────────────────────────────────

/// File-system glob and ignore configuration.
///
/// ```yaml
/// fs:
///   glob:
///     mode: picomatch   # picomatch | bash
///     dot: false        # match dotfiles with * ?
///   ignore:
///     use_gitignore: true
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsConfig {
    #[serde(default)]
    pub glob: FsGlobConfig,
    #[serde(default)]
    pub ignore: FsIgnoreConfig,
}

impl Default for FsConfig {
    fn default() -> Self {
        Self {
            glob: FsGlobConfig::default(),
            ignore: FsIgnoreConfig::default(),
        }
    }
}

/// Glob matching configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsGlobConfig {
    /// Matching mode: `"picomatch"` (default) or `"bash"`.
    #[serde(default = "default_glob_mode")]
    pub mode: String,
    /// If `true`, `*` and `?` will match dotfiles. Default: `false`.
    #[serde(default)]
    pub dot: bool,
}

fn default_glob_mode() -> String {
    "picomatch".to_string()
}

impl Default for FsGlobConfig {
    fn default() -> Self {
        Self { mode: default_glob_mode(), dot: false }
    }
}

/// Ignore-file configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FsIgnoreConfig {
    /// Respect `.gitignore` files during traversal. Default: `true`.
    #[serde(default = "default_true")]
    pub use_gitignore: bool,
}

fn default_true() -> bool { true }

impl Default for FsIgnoreConfig {
    fn default() -> Self {
        Self { use_gitignore: true }
    }
}

// ─── Voice Config ──────────────────────────────────────────────────────────

fn default_tts_endpoint() -> String {
    "http://127.0.0.1:7860".to_string()
}

fn default_stt_endpoint() -> String {
    "http://127.0.0.1:9090".to_string()
}

fn default_voice_language() -> String {
    "pt".to_string()
}

/// Voice / TTS/STT configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    /// Whether voice mode is enabled (set at runtime via `--with-voice`).
    #[serde(default)]
    pub enabled: bool,

    /// Base URL of the Chatterbox Multilingual TTS server.
    #[serde(default = "default_tts_endpoint")]
    pub tts_endpoint: String,

    /// Base URL of the Whisper STT server.
    #[serde(default = "default_stt_endpoint")]
    pub stt_endpoint: String,

    /// Base URL of the Hibiki TTS server (alternative to Chatterbox).
    #[serde(default = "default_tts_endpoint")]
    pub hibiki_endpoint: String,

    /// Default language for TTS synthesis.
    #[serde(default = "default_voice_language")]
    pub language: String,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            tts_endpoint: default_tts_endpoint(),
            stt_endpoint: default_stt_endpoint(),
            hibiki_endpoint: default_tts_endpoint(),
            language: default_voice_language(),
        }
    }
}

fn default_transport() -> String {
    "stdio".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub command: String,

    #[serde(default)]
    pub args: Vec<String>,

    #[serde(default)]
    pub env: HashMap<String, String>,

    #[serde(default = "default_transport")]
    pub transport: String,

    /// Future: HTTP transport URL
    pub url: Option<String>,

    /// Whether this server is enabled (default: true)
    pub enabled: Option<bool>,

    /// Connection timeout in seconds (default: 30)
    pub timeout: Option<u64>,

    /// GAR-190: Tool allowlist — only these tool names are registered into the agent runtime.
    /// If empty (default), all tools discovered from this server are available.
    /// Use this to restrict which tools an LLM can invoke from a given MCP server.
    ///
    /// Example:
    /// ```yaml
    /// mcp:
    ///   my-server:
    ///     allowed_tools: [web_search, read_file]
    /// ```
    #[serde(default)]
    pub allowed_tools: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::AppConfig;

    #[test]
    fn app_config_defaults_include_memory_block() {
        let config = AppConfig::default();
        assert!(config.memory.enabled);
        assert!(!config.memory.shared_continuity);
        assert!(config.embeddings.is_empty());
    }

    #[test]
    fn parses_memory_and_embedding_config() {
        let raw = r#"
gateway:
  host: "127.0.0.1"
  port: 3888
memory:
  enabled: true
  embedding_provider: "cohere-main"
  shared_continuity: true
embeddings:
  cohere-main:
    provider: cohere
    model: embed-english-v3.0
    api_key: test-key
    base_url: https://api.cohere.com
    dimensions: 1024
"#;

        let config: AppConfig = serde_yaml::from_str(raw).expect("yaml should parse");
        assert!(config.memory.enabled);
        assert_eq!(
            config.memory.embedding_provider.as_deref(),
            Some("cohere-main")
        );
        assert!(config.memory.shared_continuity);

        let cohere = config
            .embeddings
            .get("cohere-main")
            .expect("cohere embedding provider should exist");
        assert_eq!(cohere.provider, "cohere");
        assert_eq!(cohere.model.as_deref(), Some("embed-english-v3.0"));
        assert_eq!(cohere.dimensions, Some(1024));
    }
}
