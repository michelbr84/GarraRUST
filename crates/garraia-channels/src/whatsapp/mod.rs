pub mod api;
pub mod signature;
pub mod webhook;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use reqwest::Client;
use tokio::sync::mpsc;
use tracing::info;

use crate::traits::{Channel, ChannelStatus};
use garraia_common::{Message, MessageContent, Result};

/// Callback invoked when the bot receives a text message from WhatsApp.
///
/// Arguments: `(from_number, user_name, text, delta_tx)`.
/// `delta_tx` is always `None` for WhatsApp (no streaming support).
/// Return `Err("__blocked__")` to silently drop the message (unauthorized user).
pub type WhatsAppOnMessageFn = Arc<
    dyn Fn(
            String,
            String,
            String,
            Option<mpsc::Sender<String>>,
        ) -> Pin<Box<dyn Future<Output = std::result::Result<String, String>> + Send>>
        + Send
        + Sync,
>;

pub struct WhatsAppChannel {
    client: Client,
    access_token: String,
    phone_number_id: String,
    verify_token: String,
    /// App secret da app da Meta, usado para verificar `X-Hub-Signature-256`
    /// em cada POST do webhook (#1070). E **outro** valor que o
    /// `verify_token`, que so serve ao handshake unico do `GET`.
    app_secret: String,
    display: String,
    status: ChannelStatus,
    on_message: WhatsAppOnMessageFn,
}

impl WhatsAppChannel {
    /// Constroi o canal, **recusando** quando falta o `app_secret`.
    ///
    /// Mesma decisao do [`crate::line_channel::LineChannel::new`] (#1051):
    /// sem o segredo nao ha como verificar a assinatura do webhook, e um
    /// canal que nao consegue fazer essa distincao nao deve existir nem por
    /// engano. Estar no construtor — e nao no wiring — e o que impede
    /// qualquer call-site futuro de montar o canal em modo degradado.
    pub fn new(
        access_token: String,
        phone_number_id: String,
        verify_token: String,
        app_secret: String,
        on_message: WhatsAppOnMessageFn,
    ) -> Result<Self> {
        if app_secret.trim().is_empty() {
            return Err(garraia_common::Error::Channel(
                "whatsapp: app_secret e obrigatorio para verificar a assinatura do webhook".into(),
            ));
        }
        Ok(Self {
            client: Client::new(),
            access_token,
            phone_number_id,
            verify_token,
            app_secret,
            display: "WhatsApp".to_string(),
            status: ChannelStatus::Disconnected,
            on_message,
        })
    }

    /// Access token for the WhatsApp Cloud API.
    pub fn access_token(&self) -> &str {
        &self.access_token
    }

    /// Phone number ID for this WhatsApp Business account.
    pub fn phone_number_id(&self) -> &str {
        &self.phone_number_id
    }

    /// Verify token used for webhook verification.
    pub fn verify_token(&self) -> &str {
        &self.verify_token
    }

    /// App secret usado para verificar `X-Hub-Signature-256` (#1070).
    pub fn app_secret(&self) -> &str {
        &self.app_secret
    }

    /// HTTP client shared across requests.
    pub fn client(&self) -> &Client {
        &self.client
    }

    /// Process an incoming message from the webhook. Returns the response text.
    pub async fn handle_incoming(
        &self,
        from: &str,
        user_name: &str,
        text: &str,
    ) -> std::result::Result<String, String> {
        (self.on_message)(
            from.to_string(),
            user_name.to_string(),
            text.to_string(),
            None, // No streaming for WhatsApp
        )
        .await
    }
}

#[async_trait]
impl Channel for WhatsAppChannel {
    fn channel_type(&self) -> &str {
        "whatsapp"
    }

    fn display_name(&self) -> &str {
        &self.display
    }

    async fn connect(&mut self) -> Result<()> {
        // WhatsApp is webhook-driven — no persistent connection needed.
        self.status = ChannelStatus::Connected;
        info!("whatsapp channel connected (webhook mode)");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        self.status = ChannelStatus::Disconnected;
        info!("whatsapp channel disconnected");
        Ok(())
    }

    async fn send_message(&self, message: &Message) -> Result<()> {
        let to = message
            .metadata
            .get("whatsapp_from")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                garraia_common::Error::Channel("missing whatsapp_from in metadata".into())
            })?;

        let text = match &message.content {
            MessageContent::Text(t) => t.clone(),
            _ => {
                return Err(garraia_common::Error::Channel(
                    "only text messages are supported for whatsapp send".into(),
                ));
            }
        };

        api::send_text_message(
            &self.client,
            &self.access_token,
            &self.phone_number_id,
            to,
            &text,
        )
        .await
        .map_err(|e| garraia_common::Error::Channel(format!("whatsapp send failed: {e}")))?;

        Ok(())
    }

    fn status(&self) -> ChannelStatus {
        self.status.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_type_is_whatsapp() {
        let on_msg: WhatsAppOnMessageFn =
            Arc::new(|_from, _user, _text, _delta_tx| Box::pin(async { Ok("test".to_string()) }));
        let channel = WhatsAppChannel::new(
            "fake-token".to_string(),
            "123456".to_string(),
            "verify-me".to_string(),
            "app-secret".to_string(),
            on_msg,
        )
        .expect("app_secret presente");
        assert_eq!(channel.channel_type(), "whatsapp");
        assert_eq!(channel.display_name(), "WhatsApp");
        assert_eq!(channel.status(), ChannelStatus::Disconnected);
    }

    /// #1070: sem `app_secret` o canal nao existe. Se este teste cair, o
    /// wiring voltou a poder montar WhatsApp em modo degradado — que e
    /// exatamente o estado que deixava `POST /webhooks/whatsapp` aberto.
    #[test]
    fn sem_app_secret_o_canal_e_recusado_no_construtor() {
        for secret in ["", "   ", "\t"] {
            let on_msg: WhatsAppOnMessageFn =
                Arc::new(|_f, _u, _t, _d| Box::pin(async { Ok("test".to_string()) }));
            let r = WhatsAppChannel::new(
                "fake-token".to_string(),
                "123456".to_string(),
                "verify-me".to_string(),
                secret.to_string(),
                on_msg,
            );
            assert!(r.is_err(), "app_secret {secret:?} devia ser recusado");
        }
    }
}
