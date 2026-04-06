//! Configuration for LINE Messaging API channel.

use serde::{Deserialize, Serialize};

/// LINE channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineConfig {
    /// Channel access token from LINE Developers Console.
    pub channel_access_token: String,

    /// Channel secret for webhook signature verification.
    pub channel_secret: String,
}
