//! `GET /api/capabilities` — what this gateway can currently do, computed
//! live and secret-free.
//!
//! Split out of `health.rs` in v0.4.0, when Garra Mobile started negotiating
//! its home tiles against the feature list (ADR 0016 amendment 2026-09-07):
//! the pure `feature_flags` and the test that locks its contract with the app
//! live here, so the health module stays about health checks.

use axum::Json;
use axum::extract::State;
use serde::Serialize;

use crate::state::{AppState, SharedState};

/// What the Web Console + future remote clients can rely on. Each list is
/// computed live from the running gateway (`AgentRuntime`,
/// `ChannelRegistry`, `CommandRegistry`). Secret-free.
#[derive(Debug, Clone, Serialize)]
pub struct CapabilitiesResponse {
    /// Cargo-feature-flag-shaped capability flags.
    pub features: Vec<String>,
    /// Provider IDs currently registered (sorted, deduped).
    pub providers: Vec<String>,
    /// Per-provider configured model — emitted as `provider/model`.
    pub models: Vec<String>,
    /// Live channel registry list.
    pub channels: Vec<String>,
    /// Slash-command registry — names only, no descriptions or aliases that
    /// might leak admin paths.
    pub commands: Vec<String>,
    /// Skin / theme presets the front-end can offer.
    pub skins: Vec<String>,
    /// Forward-compat hook for `--experimental-*` flags. Empty for now.
    pub experimental_flags: Vec<String>,
    /// Gateway binary version, mirroring `/api/health`.
    pub version: &'static str,
}

/// What the runtime has wired, reduced to booleans so the feature list is a
/// pure function (testable without a `SharedState`).
#[derive(Debug, Clone, Copy, Default)]
pub struct FeatureInputs {
    pub tts: bool,
    pub stt: bool,
    pub mcp: bool,
    pub openclaw: bool,
    pub auth_v1: bool,
    pub memory: bool,
}

/// What the runtime has wired, read once from the live `AppState`.
///
/// Shared by the HTTP handler and by the agent's own `garra_status` tool, so
/// the console and the model can never disagree about what this gateway can
/// do — one reading, one truth.
pub fn feature_inputs(state: &AppState) -> FeatureInputs {
    FeatureInputs {
        tts: state.voice_client.is_some(),
        stt: state.stt_client.is_some(),
        mcp: state.mcp_manager_arc.is_some(),
        openclaw: state.openclaw_client.is_some(),
        auth_v1: state.auth_provider.is_some(),
        memory: state.agents.memory_provider().is_some(),
    }
}

/// Feature flags advertised by `GET /api/capabilities`.
///
/// Garra Mobile v0.4.0 negotiates its home tiles against this list (ADR 0016
/// amendment 2026-09-07): a tile whose feature is absent renders as
/// "unavailable on this runtime" instead of a dead screen. The entries are
/// therefore a contract with `apps/garraia-mobile/lib/runtime/models.dart`
/// (`GarraFeature`) — additions are fine, renames and removals are breaking.
///
/// Always-on entries (`learning-skills`, `projects`, `modes`) reflect handlers
/// that read files / built-ins and need no wired state; `memory` follows the
/// memory provider like `/api/memory/*` does. `automations` is deliberately
/// absent: the gateway exposes no scheduling API yet, and the mobile tile
/// says so rather than pretending.
pub fn feature_flags(inputs: &FeatureInputs) -> Vec<String> {
    let mut features: Vec<String> = vec![
        "chat".into(),
        "websocket".into(),
        "multi-channel".into(),
        "learning-skills".into(),
        "projects".into(),
        "modes".into(),
    ];
    if inputs.memory {
        features.push("memory".into());
    }
    if inputs.tts {
        features.push("tts".into());
    }
    if inputs.stt {
        features.push("stt".into());
    }
    if inputs.mcp {
        features.push("mcp".into());
    }
    if inputs.openclaw {
        features.push("openclaw".into());
    }
    if inputs.auth_v1 {
        features.push("auth-v1".into());
    }
    features
}

/// GET /api/capabilities — read-only snapshot of what the gateway can
/// currently do. Renders the Dashboard "Arquitetura" card + drives the
/// Skins page enumeration without hardcoded JS lists.
pub async fn capabilities_handler(State(state): State<SharedState>) -> Json<CapabilitiesResponse> {
    let features = feature_flags(&feature_inputs(&state));

    let mut providers: Vec<String> = state.agents.provider_ids().to_vec();
    providers.sort();
    providers.dedup();

    let models: Vec<String> = providers
        .iter()
        .filter_map(|pid| {
            state
                .agents
                .get_provider(pid)
                .and_then(|p| p.configured_model().map(|m| format!("{}/{}", pid, m)))
        })
        .collect();

    let channels: Vec<String> = state
        .channels
        .read()
        .await
        .list()
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    let commands: Vec<String> = state
        .command_registry
        .read()
        .map(|r| r.list().into_iter().map(|(n, _d)| n.to_string()).collect())
        .unwrap_or_default();

    // Plan 0117 lists four canonical skins. The Settings Registry (PR-8)
    // can later persist user-defined skins server-side.
    let skins: Vec<String> = vec![
        "garra-blue".into(),
        "aurora-admin".into(),
        "editorial".into(),
        "cyber-garra".into(),
    ];

    Json(CapabilitiesResponse {
        features,
        providers,
        models,
        channels,
        commands,
        skins,
        experimental_flags: Vec::new(),
        version: env!("CARGO_PKG_VERSION"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Contrato com o Garra Mobile (`GarraFeature` em
    /// `apps/garraia-mobile/lib/runtime/models.dart`): os nomes que a home
    /// negocia precisam continuar existindo, e `automations` precisa continuar
    /// ausente enquanto o gateway não expõe scheduling.
    #[test]
    fn feature_flags_keep_the_mobile_contract() {
        let all = feature_flags(&FeatureInputs {
            tts: true,
            stt: true,
            mcp: true,
            openclaw: true,
            auth_v1: true,
            memory: true,
        });
        for required in [
            "chat",
            "websocket",
            "multi-channel",
            "learning-skills",
            "projects",
            "modes",
            "memory",
            "tts",
            "stt",
            "mcp",
            "openclaw",
            "auth-v1",
        ] {
            assert!(all.iter().any(|f| f == required), "{required} ausente");
        }
        assert!(
            !all.iter().any(|f| f == "automations"),
            "automations so entra quando existir API de scheduling"
        );

        // Nada opcional vaza quando nada esta wired; os always-on ficam.
        let none = feature_flags(&FeatureInputs::default());
        assert_eq!(
            none,
            vec![
                "chat",
                "websocket",
                "multi-channel",
                "learning-skills",
                "projects",
                "modes"
            ]
        );
    }
}
