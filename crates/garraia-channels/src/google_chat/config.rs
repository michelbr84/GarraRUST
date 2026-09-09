//! Configuration for Google Chat channel.

use serde::{Deserialize, Serialize};

/// Google Chat channel configuration.
///
/// `Default` e derivado, nao escrito a mao: a versao manual so repetia os
/// valores do derive e o clippy a marcava com `derivable_impls` — invisivel
/// ate a feature `google_chat` entrar no CI (#1050).
///
/// Cuidado com o `Default`: ele produz `audience` vazia, que
/// [`super::GoogleChatChannel::new`] **recusa**. E de proposito — serve para
/// testes montarem a struct campo a campo, nao para representar um canal
/// utilizavel.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GoogleChatConfig {
    /// Webhook URL for incoming messages (simple integration).
    pub webhook_url: Option<String>,

    /// Path to the service account JSON key file.
    pub service_account_key_path: Option<String>,

    /// OAuth2 bearer token obtained from the service account key.
    /// Populated at runtime after authentication.
    #[serde(skip)]
    pub service_account_token: String,

    /// Audiencia esperada no JWT que o Google Chat manda em cada requisicao
    /// do webhook — o numero do projeto no Google Cloud, ou a URL da app,
    /// conforme configurado no console da Chat API.
    ///
    /// **E o que impede que o webhook de outra pessoa valha aqui.** Todo
    /// token do Chat e assinado pela mesma chave do Google
    /// (`chat@system.gserviceaccount.com`), entao a assinatura sozinha nao
    /// distingue a app de ninguem: verificar so a assinatura aceitaria um
    /// token legitimo emitido para *qualquer outra* app do Chat. O `aud` e
    /// a unica coisa no token que diz "este webhook e seu".
    #[serde(default)]
    pub audience: String,

    /// Nome da secao `[channels.<nome>]` que originou este canal. So para
    /// log — `Channel::display_name` e a constante `"Google Chat"` para
    /// todos.
    #[serde(default = "nome_padrao")]
    pub name: String,
}

fn nome_padrao() -> String {
    "google_chat".to_string()
}
