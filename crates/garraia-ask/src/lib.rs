//! #1612 — o núcleo one-shot de `garra_ask`, compartilhado por quem chama um
//! LLM sem ferramentas: a CLI (`garra ask`, e o servidor stdio `garra_ask`) e
//! a ponte MCP HTTP do gateway (`POST /mcp`).
//!
//! Mora num crate próprio porque a ponte HTTP não pode depender de `garraia`
//! (a CLI): a dependência iria no sentido errado. O que fica aqui é o que
//! as duas superfícies têm em comum — resolver um provider explícito, chamar o
//! LLM com timeout, e devolver o envelope `garra.ask.v1`. A autodetecção do
//! provider (que sonda o Ollama e pode perguntar no terminal) continua na CLI.

pub mod ask;
pub mod provider;
pub mod provider_binding;

pub use ask::{
    ARG_MESSAGE_MAX_BYTES, ARG_SYSTEM_PROMPT_MAX_BYTES, ARG_TIMEOUT_SECS_DEFAULT,
    ARG_TIMEOUT_SECS_MAX, ARG_TIMEOUT_SECS_MIN, AskError, AskOptions, AskOutcome, PROVEDORES_ASK,
    Provedor, ask_oneshot, error_envelope, executar, sanitize_provider_error, success_envelope,
    validar_argumentos,
};
pub use provider::{hardcoded_default_model, resolve_provider_model, select_explicit_provider};
