//! Configuration for Signal channel.

use serde::{Deserialize, Serialize};

/// Signal channel configuration (via signal-cli REST API).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalConfig {
    /// Base URL of the signal-cli REST API daemon.
    /// Example: "http://localhost:8080"
    pub signal_cli_url: String,

    /// Phone number registered with Signal (E.164 format).
    /// Example: "+1234567890"
    pub phone_number: String,
}
