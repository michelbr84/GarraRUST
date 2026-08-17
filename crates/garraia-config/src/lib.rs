pub mod auth;
pub mod check;
pub mod loader;
pub mod model;
pub mod provider_keys;
pub mod watcher;

pub use auth::{AuthConfig, AuthConfigError};
pub use check::{ConfigCheck, ConfigSummary, Finding, Severity, SourceReport, run_check};
pub use loader::{ConfigLoader, harden_secret_file};
pub use model::{
    AUTH_ACCESS_TTL_MAX_SECS, AUTH_ACCESS_TTL_MIN_SECS, AUTH_REFRESH_TTL_MAX_SECS,
    AUTH_REFRESH_TTL_MIN_SECS, AUTH_SUPPORTED_JWT_ALGORITHMS, AgentConfig, AppConfig, AuthSection,
    ChannelConfig, EmbeddingProviderConfig, GatewayConfig, LlmProviderConfig, MAX_PATCH_BYTES_MAX,
    MAX_PATCH_BYTES_MIN, McpServerConfig, MemoryConfig, NamedAgentConfig, S3StorageConfig,
    StorageBackend, StorageConfig, TimeoutConfig, TypeTimeout, VoiceConfig,
};
pub use provider_keys::{
    KeySource, default_vault_path, provider_key_env, resolve_api_key, resolve_api_key_source,
    resolve_provider_key_source, vault_present_but_locked,
};
pub use watcher::ConfigWatcher;
