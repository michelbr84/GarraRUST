//! Resolucao do provider de LLM para um nome explicito (`-p`/`provider` do
//! `garra_ask`), compartilhada pela CLI e pelo gateway.
//!
//! A cadeia de autodeteccao (`detect_provider`) continua na CLI: ela sonda o
//! Ollama local e pode perguntar no terminal antes de baixar um modelo, coisa
//! que um servidor HTTP nunca deve fazer.

use std::sync::Arc;

use anyhow::Result;
use garraia_agents::LlmProvider;
use garraia_config::AppConfig;

use crate::provider_binding;

/// GAR-576 — Resolve the model name for a given provider kind.
///
/// Lookup order:
///   1. `model_override` (the CLI `--model` flag, absolute precedence).
///   2. `config.llm[provider_kind].model` (key-match).
///   3. The first `config.llm[*]` entry whose `provider` field equals
///      `provider_kind` and whose `model` is `Some(non-empty)`.
///
/// Returns `None` only when no source supplies a usable model name; the
/// caller is then responsible for picking a hardcoded fallback.
pub fn resolve_provider_model(
    config: &AppConfig,
    provider_kind: &str,
    model_override: Option<&str>,
) -> Option<String> {
    if let Some(m) = model_override
        && !m.is_empty()
    {
        return Some(m.to_string());
    }
    if let Some(cfg) = config.llm.get(provider_kind)
        && let Some(m) = cfg.model.as_deref()
        && !m.is_empty()
    {
        return Some(m.to_string());
    }
    for cfg in config.llm.values() {
        if cfg.provider == provider_kind
            && let Some(m) = cfg.model.as_deref()
            && !m.is_empty()
        {
            return Some(m.to_string());
        }
    }
    None
}

/// GAR-576 — Last-resort fallback model name per provider kind, used
/// only when neither the CLI flag nor `config.llm` supplies one.
///
/// Single source of truth: `select_explicit_provider` and `detect_provider`
/// both route through here rather than repeating the literals inline, so the
/// two paths cannot disagree about what "the default" means.
pub fn hardcoded_default_model(provider_kind: &str) -> String {
    // The two project defaults (issue #1180) are keyed by the shared
    // constants rather than by a literal, so a rename in `crate::defaults`
    // cannot leave this table pointing at a provider kind that no longer
    // exists.
    match provider_kind {
        // The local *second* option — see `crate::defaults`.
        garraia_config::defaults::DEFAULT_LOCAL_PROVIDER => {
            garraia_config::defaults::DEFAULT_LOCAL_MODEL
        }
        // llama-server serves whatever model it was started with; the
        // OpenAI-compatible API accepts any string here — `default` matches
        // `garraia_agents::llama_cpp::DEFAULT_MODEL` byte-for-byte.
        "llamacpp" => "default",
        "anthropic" => "claude-sonnet-4-5-20250929",
        "openai" => "gpt-4o",
        // The official project default. Never `openrouter/auto`: `auto`
        // stays reachable only when the user passes it explicitly.
        garraia_config::defaults::DEFAULT_CLOUD_PROVIDER => {
            garraia_config::defaults::DEFAULT_CLOUD_MODEL
        }
        "echo" => "echo-stub",
        _ => "auto",
    }
    .to_string()
}

/// Model for an explicitly named provider: `--model` > the bound entry's own
/// model > `resolve_provider_model` (legacy scan by kind) > the per-kind
/// hardcoded default.
pub fn explicit_model(
    config: &AppConfig,
    binding: &provider_binding::ProviderBinding,
    model_override: Option<&str>,
) -> String {
    model_override
        .filter(|m| !m.is_empty())
        .map(str::to_string)
        .or_else(|| binding.model().map(str::to_string))
        .or_else(|| resolve_provider_model(config, binding.kind(), None))
        .unwrap_or_else(|| hardcoded_default_model(binding.kind()))
}

/// GAR-579 — Build a provider from an explicit `--provider <name>` flag.
///
/// Returns the same `(display_name, model, Arc<dyn LlmProvider>)` triple
/// that `detect_provider` returns. Honors `model_override` first, then the
/// bound entry's model, then `resolve_provider_model`, then a hardcoded
/// per-kind fallback. An unknown name is an error; a missing api_key for a
/// cloud provider is an error.
///
/// `name` is a provider kind (`openai`, `anthropic`, …) or an alias defined
/// under `llm:` (`lmstudio` with `provider: openai`) — the MCP policy already
/// accepted aliases, this is where they now resolve. Endpoint and
/// credential come from the same entry ([`provider_binding::bind_named`]):
/// `-p openai` used to read `llm.openai.api_key` and drop
/// `llm.openai.base_url`, sending the key of a custom OpenAI-compatible
/// endpoint to https://api.openai.com (v0.4.4 clean-install smoke).
///
/// `url_override` is the CLI `--url` flag. Only the keyless `llamacpp`
/// consumes it; on a keyed provider it would ship the entry's key to an
/// ad-hoc address.
///
/// Shared by `chat::run_chat`, `ask::run_ask` and the MCP tools so the
/// explicit-provider path lives in exactly one place.
pub fn select_explicit_provider(
    config: &AppConfig,
    name: &str,
    model_override: Option<&str>,
    url_override: Option<&str>,
) -> Result<(String, String, Arc<dyn LlmProvider>)> {
    select_explicit_provider_with_env(
        config,
        name,
        model_override,
        url_override,
        &provider_binding::process_env,
    )
}

/// [`select_explicit_provider`] with the environment injected, so tests pin
/// the env-var fallback without touching the process environment.
pub fn select_explicit_provider_with_env(
    config: &AppConfig,
    name: &str,
    model_override: Option<&str>,
    url_override: Option<&str>,
    env: provider_binding::Env<'_>,
) -> Result<(String, String, Arc<dyn LlmProvider>)> {
    let Some(binding) = provider_binding::bind_named(config, name, env) else {
        anyhow::bail!(
            "Provider desconhecido: {name}. Use: ollama, llamacpp, anthropic, openai, openrouter \
             (ou o nome de uma entrada em llm: no config.yml)"
        );
    };
    let model = explicit_model(config, &binding, model_override);
    // An OpenAI-compatible alias registers under its own name (GAR-582), so
    // a lookup by that name resolves. The other kinds cannot be renamed and
    // register as their kind; callers that look the provider up by name
    // (the MCP agent) therefore ask for `provider.provider_id()`, never for
    // the name that was typed.
    let provider_id = (!provider_binding::is_buildable_kind(name)).then_some(name);
    let provider = provider_binding::build_provider(&binding, &model, provider_id, url_override)?;
    Ok((name.to_string(), model, provider))
}
