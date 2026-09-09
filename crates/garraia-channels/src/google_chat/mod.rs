//! Google Chat (Workspace) channel implementation for GarraIA.
//!
//! Provides a `GoogleChatChannel` struct that implements the `Channel` trait,
//! communicating via Google Chat webhooks and REST API.

pub mod auth;
pub mod config;
pub mod webhook;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use reqwest::Client;
use tokio::sync::mpsc;
use tracing::info;

use crate::traits::{Channel, ChannelStatus};
#[cfg(test)]
use garraia_common::{ChannelId, MessageDirection, SessionId, UserId};
use garraia_common::{Error, Message, MessageContent, Result};

pub use config::GoogleChatConfig;

/// Callback invoked when a Google Chat webhook event is received.
///
/// Arguments: `(space_id, user_id, user_name, text, delta_tx)`.
/// Return `Err("__blocked__")` to silently drop unauthorized messages.
pub type GoogleChatOnMessageFn = Arc<
    dyn Fn(
            String,
            String,
            String,
            String,
            Option<mpsc::Sender<String>>,
        ) -> Pin<Box<dyn Future<Output = std::result::Result<String, String>> + Send>>
        + Send
        + Sync,
>;

/// Google Chat channel implementation.
///
/// Uses Google Chat REST API and incoming webhooks for message handling.
pub struct GoogleChatChannel {
    config: GoogleChatConfig,
    client: Client,
    status: ChannelStatus,
    on_message: GoogleChatOnMessageFn,
    /// Chaves publicas do Google, buscadas sob demanda e renovadas sozinhas.
    /// Uma por canal: sao baratas e evitam estado global.
    jwks: crate::jwks::JwksCache,
}

impl GoogleChatChannel {
    /// Create a new `GoogleChatChannel` from config and callback.
    ///
    /// Recusa a construcao quando `audience` esta vazia (#1050). Sem ela nao
    /// da para distinguir um webhook desta app de um webhook legitimo de
    /// **outra** app do Google Chat: todos os tokens sao assinados pela mesma
    /// chave, e o `aud` e a unica coisa que diz para quem o token foi
    /// emitido. Um canal que nao consegue fazer essa distincao aceitaria um
    /// token encaminhado por qualquer outra app — e nao deve existir nem por
    /// engano.
    ///
    /// E aqui, e nao no bootstrap, pelo mesmo motivo do `LineChannel::new`:
    /// assim nenhum wiring futuro consegue pular a checagem, porque o
    /// compilador obriga a tratar o erro.
    pub fn new(config: GoogleChatConfig, on_message: GoogleChatOnMessageFn) -> Result<Self> {
        if config.audience.trim().is_empty() {
            return Err(Error::Channel(
                "google chat: audience e obrigatoria para verificar o token do webhook".into(),
            ));
        }
        Ok(Self {
            config,
            client: Client::new(),
            status: ChannelStatus::Disconnected,
            on_message,
            jwks: crate::jwks::JwksCache::new(auth::JWK_URL),
        })
    }

    /// Nome da secao de config que originou este canal, para log.
    pub fn name(&self) -> &str {
        &self.config.name
    }

    /// Verifica um token de webhook contra a `audience` deste canal.
    ///
    /// O motivo da recusa serve para log; a resposta HTTP e 401 em todos os
    /// casos, sem distinguir qual deles ocorreu.
    pub async fn verificar_token(&self, token: &str) -> std::result::Result<(), auth::AuthError> {
        auth::verificar_token(&self.jwks, token, &self.config.audience).await
    }

    /// Access the current config.
    pub fn config(&self) -> &GoogleChatConfig {
        &self.config
    }

    /// Process an incoming webhook event from Google Chat.
    pub async fn handle_incoming(
        &self,
        space_id: &str,
        user_id: &str,
        user_name: &str,
        text: &str,
    ) -> std::result::Result<String, String> {
        (self.on_message)(
            space_id.to_string(),
            user_id.to_string(),
            user_name.to_string(),
            text.to_string(),
            None,
        )
        .await
    }

    /// Send a text message to a Google Chat space via REST API.
    ///
    /// `space_name` vem do corpo do webhook (`message.space.name`). O corpo
    /// ja passou pela autenticacao, mas quem manda a mensagem ainda controla
    /// o valor — entao ele e validado antes de entrar na URL. Ver
    /// [`space_name_valido`].
    pub async fn send_to_space(&self, space_name: &str, text: &str) -> Result<()> {
        if !space_name_valido(space_name) {
            return Err(Error::Channel(format!(
                "google chat: space name recusado, fora da forma 'spaces/<id>': {space_name:?}"
            )));
        }
        let url = format!("https://chat.googleapis.com/v1/{}/messages", space_name);

        let body = serde_json::json!({
            "text": text,
        });

        let resp = self
            .client
            .post(&url)
            .bearer_auth(&self.config.service_account_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Channel(format!("google chat send failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Channel(format!(
                "google chat API error {status}: {body}"
            )));
        }

        Ok(())
    }

    /// Send a message via the configured webhook URL (simple integration).
    pub async fn send_via_webhook(&self, text: &str) -> Result<()> {
        let url = self
            .config
            .webhook_url
            .as_deref()
            .ok_or_else(|| Error::Channel("google chat webhook_url not configured".into()))?;

        let body = serde_json::json!({
            "text": text,
        });

        let resp = self
            .client
            .post(url)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Channel(format!("google chat webhook send failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Channel(format!(
                "google chat webhook error {status}: {body}"
            )));
        }

        Ok(())
    }
}

/// O `space_name` tem a forma `spaces/<id>` e nada mais.
///
/// Ele e interpolado no caminho de uma URL da API do Google, e vem do corpo
/// do webhook — autenticado, mas escrito por quem manda a mensagem. O host
/// e fixo (`chat.googleapis.com`), entao nao ha como alcancar a rede interna
/// por aqui; o que um valor torto alcanca sao **outros endpoints do Google**,
/// com o token da conta de servico junto: `spaces/../../v1/admin/...` sobe um
/// nivel de caminho, e `spaces/x?alt=media#` injeta query e fragmento.
///
/// Por isso uma allow-list de forma, e nao uma deny-list de caracteres ruins:
/// tudo que nao for exatamente `spaces/` seguido de um identificador
/// alfanumerico (com `-` e `_`) e recusado. O threat model (§5.6) lista este
/// call site como lacuna do `garraia_common::ssrf`; como o host e constante,
/// a validacao de forma cobre o que sobra.
fn space_name_valido(space_name: &str) -> bool {
    let Some(id) = space_name.strip_prefix("spaces/") else {
        return false;
    };
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[async_trait]
impl Channel for GoogleChatChannel {
    fn channel_type(&self) -> &str {
        "google_chat"
    }

    fn display_name(&self) -> &str {
        "Google Chat"
    }

    async fn connect(&mut self) -> Result<()> {
        // Google Chat is webhook-driven — no persistent connection needed.
        self.status = ChannelStatus::Connected;
        info!("google chat channel connected (webhook mode)");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        self.status = ChannelStatus::Disconnected;
        info!("google chat channel disconnected");
        Ok(())
    }

    async fn send_message(&self, message: &Message) -> Result<()> {
        let space_name = message
            .metadata
            .get("google_chat_space")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Channel("missing google_chat_space in metadata".into()))?;

        let text = match &message.content {
            MessageContent::Text(t) => t.clone(),
            _ => {
                return Err(Error::Channel(
                    "only text messages are supported for google chat send".into(),
                ));
            }
        };

        self.send_to_space(space_name, &text).await
    }

    fn status(&self) -> ChannelStatus {
        self.status.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn space_name_bem_formado_e_aceito() {
        for bom in [
            "spaces/AAAA",
            "spaces/AAAAQQ_-1234",
            "spaces/a",
            "spaces/ABCdef123",
        ] {
            assert!(space_name_valido(bom), "{bom} deveria passar");
        }
    }

    /// O `space_name` vem do corpo do webhook. Ele e interpolado no caminho
    /// de uma URL do Google **com o token da conta de servico junto**, entao
    /// um valor torto alcanca endpoints que ninguem pretendeu — nao a rede
    /// interna (o host e constante), mas outras APIs do Google.
    #[test]
    fn space_name_torto_e_recusado() {
        for ruim in [
            // Sobe niveis de caminho.
            "spaces/../../v1/admin",
            "spaces/..",
            "spaces/a/../../b",
            // Injeta query e fragmento.
            "spaces/x?alt=media",
            "spaces/x#frag",
            // Tenta trocar o caminho inteiro.
            "spaces/x/messages/../../../v2/other",
            // Sem o prefixo nao e space nenhum.
            "AAAA",
            "/spaces/AAAA",
            "https://evil.example/spaces/AAAA",
            // Prefixo sem id.
            "spaces/",
            "",
            // Barra a mais quebra a forma `spaces/<id>`.
            "spaces/a/b",
            // Espaco vira %20 ou pior, dependendo do cliente.
            "spaces/a b",
        ] {
            assert!(!space_name_valido(ruim), "{ruim:?} nao deveria passar");
        }
    }

    #[tokio::test]
    async fn send_to_space_recusa_space_name_torto_antes_de_qualquer_requisicao() {
        let on_msg: GoogleChatOnMessageFn = Arc::new(|_space, _uid, _user, _text, _delta_tx| {
            Box::pin(async { Ok("test".to_string()) })
        });
        let config = GoogleChatConfig {
            webhook_url: None,
            service_account_key_path: None,
            service_account_token: String::new(),
            audience: "1234567890".into(),
            name: "gchat-teste".into(),
        };
        let channel = GoogleChatChannel::new(config, on_msg).expect("audience nao vazia");
        let erro = channel
            .send_to_space("spaces/../../v1/admin", "oi")
            .await
            .expect_err("space name torto tem de ser recusado");
        assert!(
            erro.to_string().contains("space name recusado"),
            "erro inesperado: {erro}"
        );
    }

    #[test]
    fn channel_type_is_google_chat() {
        let on_msg: GoogleChatOnMessageFn = Arc::new(|_space, _uid, _user, _text, _delta_tx| {
            Box::pin(async { Ok("test".to_string()) })
        });
        let config = GoogleChatConfig {
            webhook_url: Some(
                "https://chat.googleapis.com/v1/spaces/test/messages?key=test".into(),
            ),
            service_account_key_path: None,
            service_account_token: String::new(),
            audience: "1234567890".into(),
            name: "gchat-teste".into(),
        };
        let channel = GoogleChatChannel::new(config, on_msg).expect("audience nao vazia");
        assert_eq!(channel.channel_type(), "google_chat");
        assert_eq!(channel.display_name(), "Google Chat");
        assert_eq!(channel.status(), ChannelStatus::Disconnected);
    }

    #[tokio::test]
    async fn send_message_missing_space_metadata() {
        let on_msg: GoogleChatOnMessageFn = Arc::new(|_space, _uid, _user, _text, _delta_tx| {
            Box::pin(async { Ok("test".to_string()) })
        });
        let config = GoogleChatConfig {
            webhook_url: Some("https://example.com/webhook".into()),
            service_account_key_path: None,
            service_account_token: String::new(),
            audience: "1234567890".into(),
            name: "gchat-teste".into(),
        };
        let channel = GoogleChatChannel::new(config, on_msg).expect("audience nao vazia");
        let msg = Message::text(
            SessionId::from_string("s"),
            ChannelId::from_string("c"),
            UserId::from_string("u"),
            MessageDirection::Outgoing,
            "hello",
        );
        let result = channel.send_message(&msg).await;
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("google_chat_space")
        );
    }

    #[test]
    fn send_via_webhook_requires_url() {
        let on_msg: GoogleChatOnMessageFn = Arc::new(|_space, _uid, _user, _text, _delta_tx| {
            Box::pin(async { Ok("test".to_string()) })
        });
        let config = GoogleChatConfig {
            webhook_url: None,
            service_account_key_path: None,
            service_account_token: String::new(),
            audience: "1234567890".into(),
            name: "gchat-teste".into(),
        };
        let channel = GoogleChatChannel::new(config, on_msg).expect("audience nao vazia");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(channel.send_via_webhook("test"));
        assert!(result.is_err());
    }

    #[test]
    fn initial_status_is_disconnected() {
        let on_msg: GoogleChatOnMessageFn = Arc::new(|_space, _uid, _user, _text, _delta_tx| {
            Box::pin(async { Ok("test".to_string()) })
        });
        let config = GoogleChatConfig {
            webhook_url: Some("https://example.com".into()),
            service_account_key_path: None,
            service_account_token: String::new(),
            audience: "1234567890".into(),
            name: "gchat-teste".into(),
        };
        let channel = GoogleChatChannel::new(config, on_msg).expect("audience nao vazia");
        assert_eq!(channel.status(), ChannelStatus::Disconnected);
    }
}
