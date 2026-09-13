//! Issue #1180 — the one place that says which LLM GarraIA uses when the
//! user has not chosen anything.
//!
//! Before this module the answer depended on which door the user came
//! through: `garra chat` said `openrouter/auto`, the wizard said
//! `openrouter/auto`, `garra mcp-server` said `openrouter/free`, and the
//! Desktop config shipped `default_provider: "lmstudio"`. Four answers to
//! one question. The owner's decision (issue #1180, ADR 0021) collapses
//! them into one:
//!
//! 1. [`DEFAULT_CLOUD_PROVIDER`] + [`DEFAULT_CLOUD_MODEL`] is the official
//!    default of every fresh installation.
//! 2. Local (Ollama, [`DEFAULT_LOCAL_MODEL`]) is always the **second**
//!    option — the fallback, offered when the machine can host it or when
//!    the user asks for it, never preselected over the cloud default.
//! 3. `openrouter/auto` and `openrouter/free` stop being anyone's default.
//!    They stay reachable as an explicit user choice; no code path picks
//!    them on its own.
//!
//! Every surface (`chat.rs`, `wizard/mod.rs`, `mcp_server.rs`, and through
//! it `mcp_agent.rs`) reads the constants below instead of repeating a
//! literal, so the four defaults cannot drift apart again.
//!
//! The slug `z-ai/glm-5.3-flash` could not be verified against
//! `https://openrouter.ai/api/v1/models` from the build environment
//! (egress to `openrouter.ai` is blocked). It is the slug this repository
//! already uses as its canonical example (see `garraia-gateway`'s
//! `anthropic_api.rs`, `config_cmd.rs`, `main.rs` and
//! `docs/cli-mcp-server.md`), which is evidence of real use but not a
//! formal validation.

/// Provider key of the official default: OpenRouter, one key fronting many
/// models. Doubles as the `llm:` map key and the `provider:` type string.
pub(crate) const DEFAULT_CLOUD_PROVIDER: &str = "openrouter";

/// The official default model of every GarraIA installation.
///
/// Cost note (inherited from the `openrouter/free` era): the MCP surface
/// used to default to `openrouter/free` purely as a spend guardrail, so
/// that an agent host calling `garra_ask` in a loop could not run up a
/// bill. That intent is preserved deliberately, not by accident —
/// `z-ai/glm-5.3-flash` is a flash-tier model priced low enough to be the
/// unattended default, while actually being good enough for real work
/// (which `openrouter/free` was not). Anything more expensive must stay an
/// explicit caller choice, and operators who want a hard ceiling still
/// have `GARRAIA_MCP_MODEL_ALLOWLIST`.
pub(crate) const DEFAULT_CLOUD_MODEL: &str = "z-ai/glm-5.3-flash";

/// Provider key of the **second** option: a local Ollama daemon. Present
/// as the fallback in `agent.fallback_providers`, never as the primary
/// unless the user explicitly asks for a local-first install.
pub(crate) const DEFAULT_LOCAL_PROVIDER: &str = "ollama";

/// Local fallback model. `qwen3.8:latest` == `qwen3.8:27b` (Q4_K_M,
/// ~18 GB, 262 144-token context, vision + tools). Kept byte-identical to
/// `garraia_agents::ollama::DEFAULT_MODEL` — `chat.rs` has a test that
/// asserts the two agree.
pub(crate) const DEFAULT_LOCAL_MODEL: &str = "qwen3.8:latest";

#[cfg(test)]
mod tests {
    use super::*;

    /// Spec-lock for issue #1180. These four strings are the project's
    /// answer to "which LLM runs when nobody chose one?" — every surface
    /// reads them, so changing a row here changes `garra chat`, the
    /// wizard, the MCP server and the Desktop default config at once.
    /// That is the point, and it is why the change must be deliberate:
    /// update the docs, `config.default.yml` and the ADR in the same
    /// commit.
    #[test]
    fn default_llm_identity_is_locked() {
        assert_eq!(DEFAULT_CLOUD_PROVIDER, "openrouter");
        assert_eq!(DEFAULT_CLOUD_MODEL, "z-ai/glm-5.3-flash");
        assert_eq!(DEFAULT_LOCAL_PROVIDER, "ollama");
        assert_eq!(DEFAULT_LOCAL_MODEL, "qwen3.8:latest");
    }

    /// Issue #1180 decision 3 — no default may be `openrouter/auto` or
    /// `openrouter/free` again. They remain valid values a user can pass
    /// explicitly; they may never be what a code path picks by itself.
    #[test]
    fn no_default_is_openrouter_auto_or_free() {
        for value in [
            DEFAULT_CLOUD_MODEL,
            DEFAULT_LOCAL_MODEL,
            DEFAULT_CLOUD_PROVIDER,
            DEFAULT_LOCAL_PROVIDER,
        ] {
            assert_ne!(value, "openrouter/auto");
            assert_ne!(value, "openrouter/free");
        }
    }
}
