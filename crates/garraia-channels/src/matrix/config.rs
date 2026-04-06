//! Configuration for Matrix channel.

use serde::{Deserialize, Serialize};

/// Matrix channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixConfig {
    /// Matrix homeserver URL (e.g., "https://matrix.org").
    pub homeserver_url: String,

    /// Access token for the Matrix bot account.
    pub access_token: String,

    /// List of room IDs to listen in. Empty means all joined rooms.
    #[serde(default)]
    pub room_ids: Vec<String>,
}
