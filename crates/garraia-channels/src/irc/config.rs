//! Configuration for IRC channel.

use serde::{Deserialize, Serialize};

/// IRC channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IrcConfig {
    /// IRC server hostname.
    pub server: String,

    /// IRC server port (typically 6667 for plain, 6697 for TLS).
    pub port: u16,

    /// Bot nickname.
    pub nick: String,

    /// IRC channels to join (e.g., ["#garraia", "#dev"]).
    pub channels: Vec<String>,

    /// Whether to use TLS for the connection.
    #[serde(default)]
    pub use_tls: bool,
}

impl Default for IrcConfig {
    fn default() -> Self {
        Self {
            server: "irc.libera.chat".to_string(),
            port: 6667,
            nick: "garrabot".to_string(),
            channels: vec![],
            use_tls: false,
        }
    }
}
