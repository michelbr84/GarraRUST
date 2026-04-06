//! Configuration for Microsoft Teams channel.

use serde::{Deserialize, Serialize};

/// Microsoft Teams channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamsConfig {
    /// Azure AD application (client) ID.
    pub app_id: String,

    /// Azure AD application secret.
    pub app_secret: String,

    /// Azure AD tenant ID.
    pub tenant_id: String,
}
