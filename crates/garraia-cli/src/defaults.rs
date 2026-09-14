//! Issue #1180 — the CLI's view of the project-wide default LLM identity.
//!
//! The constants themselves live in [`garraia_config::defaults`]: the
//! gateway needs them too (`bootstrap/mod.rs` used to carry its own
//! `openai/gpt-4o` literal for a keyless `openrouter` block), and a
//! `pub(crate)` module inside the `garraia` binary is not reachable from
//! another crate. `garraia-config` is the crate both sides already depend
//! on, so it is where the single source of truth sits.
//!
//! This module re-exports them unchanged so every `crate::defaults::…`
//! import in `chat.rs`, `wizard/`, `mcp_server.rs` and `mcp_agent.rs`
//! keeps working, and adds the one lock that only the CLI can hold: the
//! Desktop's `config.default.yml`, a resource file that sits next to this
//! crate and cannot read a Rust constant at all.

pub(crate) use garraia_config::defaults::{
    DEFAULT_CLOUD_MODEL, DEFAULT_CLOUD_PROVIDER, DEFAULT_LOCAL_MODEL, DEFAULT_LOCAL_PROVIDER,
};

#[cfg(test)]
mod tests {
    use super::*;

    /// Issue #1180 — the Desktop is the one surface that *cannot* read the
    /// shared constants: `config.default.yml` is a resource file copied
    /// verbatim into the user's config dir on first run, so it has to repeat
    /// the four literals. Every other surface is protected by the shared
    /// constant; this file is protected only by this test.
    ///
    /// Both ADR 0022 and the acceptance criteria of issue #1180 name Desktop
    /// explicitly in the promise that a clean install ends on
    /// `agent.default_provider: openrouter` + `z-ai/glm-5.3-flash`
    /// "verificavel por teste". Without this test that promise is unverified
    /// for Desktop: the YAML could drift back to `lmstudio` (its pre-#1180
    /// value) or to `openrouter/auto` and every other #1180 lock would still
    /// pass green.
    ///
    /// `include_str!` rather than a runtime read on purpose: moving or
    /// deleting the resource then breaks the build instead of silently
    /// skipping the check.
    #[test]
    fn desktop_default_config_matches_the_shared_constants() {
        const YAML: &str =
            include_str!("../../garraia-desktop/src-tauri/resources/config.default.yml");
        let doc: serde_yaml::Value =
            serde_yaml::from_str(YAML).expect("config.default.yml must be valid YAML");

        let llm = doc.get("llm").expect("llm section present");

        let cloud = llm
            .get(DEFAULT_CLOUD_PROVIDER)
            .unwrap_or_else(|| panic!("llm.{DEFAULT_CLOUD_PROVIDER} block present"));
        assert_eq!(
            cloud.get("provider").and_then(|v| v.as_str()),
            Some(DEFAULT_CLOUD_PROVIDER),
            "the Desktop cloud block must be the project default provider"
        );
        assert_eq!(
            cloud.get("model").and_then(|v| v.as_str()),
            Some(DEFAULT_CLOUD_MODEL),
            "the Desktop cloud model must be the project default model"
        );

        let local = llm
            .get(DEFAULT_LOCAL_PROVIDER)
            .unwrap_or_else(|| panic!("llm.{DEFAULT_LOCAL_PROVIDER} block present"));
        assert_eq!(
            local.get("provider").and_then(|v| v.as_str()),
            Some(DEFAULT_LOCAL_PROVIDER),
            "the Desktop local block must be the project fallback provider"
        );
        assert_eq!(
            local.get("model").and_then(|v| v.as_str()),
            Some(DEFAULT_LOCAL_MODEL),
            "the Desktop local model must be the project fallback model"
        );

        let agent = doc.get("agent").expect("agent section present");
        assert_eq!(
            agent.get("default_provider").and_then(|v| v.as_str()),
            Some(DEFAULT_CLOUD_PROVIDER),
            "a fresh Desktop install must boot on the cloud default, not on local"
        );

        let fallbacks: Vec<&str> = agent
            .get("fallback_providers")
            .and_then(|v| v.as_sequence())
            .expect("agent.fallback_providers present")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(
            fallbacks,
            vec![DEFAULT_LOCAL_PROVIDER],
            "local stays the SECOND option, in agent.fallback_providers"
        );
    }
}
