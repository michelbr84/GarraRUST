//! Configuration for Google Chat channel.

use serde::{Deserialize, Serialize};

/// Google Chat channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleChatConfig {
    /// Webhook URL for incoming messages (simple integration).
    pub webhook_url: Option<String>,

    /// Path to the service account JSON key file.
    pub service_account_key_path: Option<String>,

    /// OAuth2 bearer token obtained from the service account key.
    /// Populated at runtime after authentication.
    #[serde(skip)]
    pub service_account_token: String,
}

impl Default for GoogleChatConfig {
    fn default() -> Self {
        Self {
            webhook_url: None,
            service_account_key_path: None,
            service_account_token: String::new(),
        }
    }
}
