use serde::{Deserialize, Serialize};

/// Configuration for the OpenClaw bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenClawConfig {
    /// Whether the OpenClaw bridge is enabled.
    #[serde(default)]
    pub enabled: bool,

    /// WebSocket URL of the OpenClaw daemon.
    #[serde(default = "default_ws_url")]
    pub ws_url: String,

    /// Which OpenClaw sub-channels to bridge (e.g. ["whatsapp", "signal"]).
    /// Empty means bridge all available channels.
    #[serde(default)]
    pub channels: Vec<String>,

    /// Seconds between reconnection attempts.
    #[serde(default = "default_reconnect_interval")]
    pub reconnect_interval_secs: u64,

    /// Expose GarraIA tools to OpenClaw and vice versa.
    #[serde(default)]
    pub tool_sharing: bool,

    /// Bridge voice features (TTS/STT) through OpenClaw.
    #[serde(default)]
    pub voice_bridge: bool,
}

fn default_ws_url() -> String {
    "ws://127.0.0.1:18789".to_string()
}

fn default_reconnect_interval() -> u64 {
    5
}

impl Default for OpenClawConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            ws_url: default_ws_url(),
            channels: Vec::new(),
            reconnect_interval_secs: default_reconnect_interval(),
            tool_sharing: false,
            voice_bridge: false,
        }
    }
}
