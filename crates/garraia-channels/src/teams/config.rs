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

    /// Nome da secao `[channels.<nome>]` que originou este canal. So para
    /// log — `Channel::display_name` e a constante `"Microsoft Teams"` para
    /// todos.
    #[serde(default = "nome_padrao")]
    pub name: String,
}

fn nome_padrao() -> String {
    "teams".to_string()
}
