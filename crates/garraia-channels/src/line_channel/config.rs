//! Configuration for LINE Messaging API channel.

use serde::{Deserialize, Serialize};

/// LINE channel configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineConfig {
    /// Channel access token from LINE Developers Console.
    pub channel_access_token: String,

    /// Channel secret for webhook signature verification.
    pub channel_secret: String,

    /// Nome da secao `[channels.<nome>]` que originou este canal.
    ///
    /// Existe so para log. `Channel::display_name` devolve a constante
    /// `"LINE"` para todo canal, entao com dois canais LINE configurados o
    /// log do webhook nao dizia de qual deles se tratava — e o webhook e
    /// exatamente o ponto onde a assinatura escolhe entre eles (#1050).
    #[serde(default = "nome_padrao")]
    pub name: String,
}

fn nome_padrao() -> String {
    "line".to_string()
}
