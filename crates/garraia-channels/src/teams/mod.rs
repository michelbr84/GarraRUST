//! Microsoft Teams (Graph API) channel implementation for GarraIA.
//!
//! Provides a `TeamsChannel` struct that implements the `Channel` trait,
//! communicating via Microsoft Bot Framework and Graph API.

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
use garraia_common::{Error, Message, Result};

pub use config::TeamsConfig;

/// Callback invoked when a Teams Bot Framework webhook event is received.
///
/// Arguments: `(conversation_id, user_id, user_name, text, delta_tx)`.
/// Return `Err("__blocked__")` to silently drop unauthorized messages.
pub type TeamsOnMessageFn = Arc<
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

/// Microsoft Teams channel implementation.
///
/// Uses the Bot Framework webhook model for receiving messages and
/// the Graph API for sending replies.
pub struct TeamsChannel {
    config: TeamsConfig,
    client: Client,
    status: ChannelStatus,
    on_message: TeamsOnMessageFn,
    /// OAuth2 access token for Graph API calls, obtained at runtime.
    access_token: Option<String>,
    /// Chaves publicas do Bot Framework, buscadas sob demanda e renovadas
    /// sozinhas. Uma por canal: sao baratas e evitam estado global.
    jwks: crate::jwks::JwksCache,
}

impl TeamsChannel {
    /// Create a new `TeamsChannel` from config and callback.
    ///
    /// Recusa a construcao quando `app_id` esta vazio (#1050). O `app_id` e
    /// a audiencia esperada no token do webhook: sem ela nao da para
    /// distinguir um token emitido para **este** bot de um emitido para
    /// qualquer outro bot do Bot Framework, e todos sao assinados pelas
    /// mesmas chaves. E no construtor, e nao no bootstrap, para que nenhum
    /// wiring futuro consiga pular a checagem.
    pub fn new(config: TeamsConfig, on_message: TeamsOnMessageFn) -> Result<Self> {
        if config.app_id.trim().is_empty() {
            return Err(Error::Channel(
                "teams: app_id e obrigatorio para verificar o token do webhook".into(),
            ));
        }
        Ok(Self {
            config,
            client: Client::new(),
            status: ChannelStatus::Disconnected,
            on_message,
            access_token: None,
            jwks: crate::jwks::JwksCache::new(auth::JWK_URL),
        })
    }

    /// Nome da secao de config que originou este canal, para log.
    pub fn name(&self) -> &str {
        &self.config.name
    }

    /// Verifica um token de webhook e devolve o `serviceurl` **assinado**.
    ///
    /// O retorno nao e `()` de proposito: quem chama precisa comparar esse
    /// valor com o `serviceUrl` do corpo antes de responder. Ver
    /// `teams::auth`.
    pub async fn verificar_token(
        &self,
        token: &str,
    ) -> std::result::Result<auth::ServiceUrlVerificada, auth::AuthError> {
        auth::verificar_token(&self.jwks, token, &self.config.app_id).await
    }

    /// Access the current config.
    pub fn config(&self) -> &TeamsConfig {
        &self.config
    }

    /// Process an incoming Bot Framework activity.
    pub async fn handle_incoming(
        &self,
        conversation_id: &str,
        user_id: &str,
        user_name: &str,
        text: &str,
    ) -> std::result::Result<String, String> {
        (self.on_message)(
            conversation_id.to_string(),
            user_id.to_string(),
            user_name.to_string(),
            text.to_string(),
            None,
        )
        .await
    }

    /// Obtain an OAuth2 access token from Azure AD using client credentials.
    ///
    /// Sem `garraia_common::ssrf` de proposito: o host e constante
    /// (`login.microsoftonline.com`) e so o `tenant_id`, que vem da config
    /// estatica, entra no caminho — nenhum dos tres casos da regra 14
    /// (request, config editavel por request, tool call de LLM).
    ///
    /// **Isso muda se o `PATCH /api/settings` passar a persistir config de
    /// canal (plan 0121a).** Nesse dia o `tenant_id` vira "config editavel
    /// por request" e este call site precisa de `vet_url` + `pinned_client`
    /// com `host_allowlist: &["login.microsoftonline.com"]`.
    pub async fn authenticate(&mut self) -> Result<()> {
        let url = format!(
            "https://login.microsoftonline.com/{}/oauth2/v2.0/token",
            self.config.tenant_id
        );

        let params = [
            ("client_id", self.config.app_id.as_str()),
            ("client_secret", self.config.app_secret.as_str()),
            ("scope", "https://graph.microsoft.com/.default"),
            ("grant_type", "client_credentials"),
        ];

        let resp = self
            .client
            .post(&url)
            .form(&params)
            .send()
            .await
            .map_err(|e| Error::Channel(format!("teams auth failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Channel(format!("teams auth error {status}: {body}")));
        }

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| Error::Channel(format!("teams auth parse failed: {e}")))?;

        let token = body
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::Channel("teams: missing access_token in auth response".into()))?
            .to_string();

        self.access_token = Some(token);
        Ok(())
    }

    /// Send a reply to a Teams conversation via Bot Framework REST API.
    ///
    /// `service_url` decide o **host** do POST, e o POST leva o bearer do
    /// bot. Ele vem do corpo da atividade, ou seja de quem manda a
    /// requisicao, entao passa por duas barreiras independentes:
    ///
    /// 1. **a claim assinada.** O chamador so chega aqui depois de o
    ///    `serviceUrl` do corpo casar com a claim `serviceurl` do token —
    ///    ver `teams::webhook`. E o que torna o valor confiavel: forjar o
    ///    host exigiria forjar o token da Microsoft.
    /// 2. **`garraia_common::ssrf` (regra 14), aqui.** Nao no lugar da
    ///    primeira, **por baixo** dela: nem um `serviceUrl` legitimamente
    ///    assinado deveria conseguir apontar para `169.254.169.254`, e o
    ///    guard tambem pina os IPs vetados, o que fecha a janela de
    ///    DNS-rebinding entre a checagem e o connect.
    ///
    /// `conversation_id` tambem entra no caminho e tambem vem do corpo;
    /// por isso o `Url::join`, que percent-encoda, em vez de `format!`.
    pub async fn send_activity(
        &self,
        service_url: &auth::ServiceUrlVerificada,
        conversation_id: &str,
        text: &str,
    ) -> Result<()> {
        let token = self.access_token.as_deref().ok_or_else(|| {
            Error::Channel("teams: not authenticated, call authenticate() first".into())
        })?;

        let base = format!("{}/", service_url.as_str().trim_end_matches('/'));
        let base = url::Url::parse(&base)
            .map_err(|e| Error::Channel(format!("teams: serviceUrl invalida: {e}")))?;
        // `join` percent-encoda o segmento, entao um `conversation_id` com
        // `../` nao sobe no caminho nem sai para outro endpoint.
        let url = base
            .join(&format!(
                "v3/conversations/{}/activities",
                utf8_percent_encode_segmento(conversation_id)
            ))
            .map_err(|e| Error::Channel(format!("teams: URL de atividade invalida: {e}")))?;

        let vetted = garraia_common::ssrf::vet_url(url.as_str(), &SSRF_POLICY)
            .map_err(|e| Error::Channel(format!("teams: serviceUrl recusada: {e}")))?;
        let client = garraia_common::ssrf::pinned_client(&vetted, &SSRF_POLICY)
            .map_err(|e| Error::Channel(format!("teams: cliente HTTP recusado: {e}")))?;

        let body = serde_json::json!({
            "type": "message",
            "text": text,
        });

        let resp = client
            .post(vetted.url.clone())
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .map_err(|e| Error::Channel(format!("teams send failed: {e}")))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Channel(format!("teams send error {status}: {body}")));
        }

        Ok(())
    }
}

/// Politica de saida para o `serviceUrl` do Bot Framework.
///
/// `PublicOnly` e https-only: o Bot Framework e um servico da internet
/// publica, e nenhuma resposta legitima vai para a LAN ou para o loopback —
/// ao contrario do Ollama ou de um MCP self-hosted, que sao os casos que
/// justificam `AllowPrivate` em outros pontos do repo.
///
/// Sem `host_allowlist`: os `serviceUrl` legitimos se espalham por varios
/// dominios da Microsoft (`smba.trafficmanager.net`, `*.botframework.com`, e
/// os regionais), e uma lista fixa quebraria implantacoes reais na primeira
/// mudanca de roteamento deles. Quem restringe o host aqui e a claim
/// assinada, nao a allow-list; esta politica restringe **faixas de
/// endereco**, que e o que a claim nao consegue restringir.
const SSRF_POLICY: garraia_common::ssrf::UrlPolicy = garraia_common::ssrf::UrlPolicy::https_public(
    std::time::Duration::from_secs(30),
    "GarraIA-Teams/1.0",
);

/// Percent-encoda um segmento de caminho.
///
/// O `conversation_id` vem do corpo da atividade. Sem encoding, um `/` nele
/// acrescenta segmentos ao caminho e um `..` sobe niveis — dois jeitos de
/// alcancar um endpoint diferente do pretendido com o bearer do bot junto.
fn utf8_percent_encode_segmento(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            outro => format!("%{outro:02X}"),
        })
        .collect()
}

#[async_trait]
impl Channel for TeamsChannel {
    fn channel_type(&self) -> &str {
        "teams"
    }

    fn display_name(&self) -> &str {
        "Microsoft Teams"
    }

    async fn connect(&mut self) -> Result<()> {
        // Authenticate with Azure AD to get an access token.
        self.authenticate().await?;
        self.status = ChannelStatus::Connected;
        info!("teams channel connected (bot framework mode)");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        self.access_token = None;
        self.status = ChannelStatus::Disconnected;
        info!("teams channel disconnected");
        Ok(())
    }

    /// **Nao suportado, e de proposito.**
    ///
    /// Esta implementacao lia `teams_service_url` do `metadata` e o passava
    /// para `send_activity` sem confronta-lo com a claim assinada do token —
    /// ou seja, pulava a primeira das duas barreiras que protegem o bearer
    /// do bot, deixando so o guard SSRF. O caminho era inalcancavel hoje (o
    /// Teams e canal push e nao entra no `ChannelRegistry`), mas era um vetor
    /// de regressao esperando qualquer wiring futuro.
    ///
    /// Agora o tipo impede: `send_activity` so aceita
    /// [`auth::ServiceUrlVerificada`], que so `verificar_token` produz. Uma
    /// resposta do Teams **tem** de nascer de um webhook verificado, entao o
    /// caminho generico do `Channel` nao tem como ser correto aqui.
    async fn send_message(&self, _message: &Message) -> Result<()> {
        Err(Error::Channel(
            "teams: send_message nao e suportado — a resposta so pode ir para a \
             serviceUrl assinada do token que originou o turno; use o fluxo do \
             webhook (`teams::webhook`), que chama `send_activity` com ela"
                .into(),
        ))
    }

    fn status(&self) -> ChannelStatus {
        self.status.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canal_autenticado() -> TeamsChannel {
        let on_msg: TeamsOnMessageFn = Arc::new(|_conv, _uid, _user, _text, _delta_tx| {
            Box::pin(async { Ok("test".to_string()) })
        });
        let mut ch = TeamsChannel::new(
            TeamsConfig {
                app_id: "app-id".into(),
                app_secret: "segredo".into(),
                tenant_id: "tenant".into(),
                name: "teams-teste".into(),
            },
            on_msg,
        )
        .expect("app_id nao vazio");
        // Sem isto o `send_activity` sai no "not authenticated" antes de
        // chegar ao guard, e os testes abaixo nao provariam nada.
        ch.access_token = Some("token-do-bot".into());
        ch
    }

    /// **O achado da auditoria.** `send_message` lia a `serviceUrl` do
    /// `metadata` e a passava adiante sem confronta-la com a claim assinada,
    /// pulando a primeira das duas barreiras. Era inalcancavel hoje, mas
    /// bastava alguem registrar o canal no `ChannelRegistry` para virar
    /// exfiltracao do bearer do bot.
    ///
    /// A defesa real nao e este teste, e o tipo: `send_activity` so aceita
    /// `ServiceUrlVerificada`, que so `verificar_token` produz. Este teste
    /// afirma o que sobra — que o caminho generico recusa em vez de tentar
    /// adivinhar.
    #[tokio::test]
    async fn send_message_recusa_porque_nao_ha_url_verificada() {
        let ch = canal_autenticado();
        let msg = Message::text(
            SessionId::from_string("s"),
            ChannelId::from_string("c"),
            UserId::from_string("u"),
            MessageDirection::Outgoing,
            "oi",
        );
        let erro = ch
            .send_message(&msg)
            .await
            .expect_err("o caminho generico nao pode responder pelo Teams");
        assert!(
            erro.to_string().contains("nao e suportado"),
            "o erro tem de dizer por que, veio: {erro}"
        );
    }

    /// **O ponto do PR, do lado da saida.** O `serviceUrl` decide o host do
    /// POST e o POST leva o bearer do bot. Um valor apontando para a rede
    /// interna nao pode ser aceito nem quando chega ate aqui — a claim
    /// assinada e a primeira barreira, esta e a segunda.
    #[tokio::test]
    async fn service_url_para_rede_interna_e_recusada() {
        let ch = canal_autenticado();
        for interna in [
            "http://169.254.169.254", // metadados de instancia
            "https://169.254.169.254",
            "http://127.0.0.1:8080",
            "https://localhost",
            "http://10.0.0.1",
            "https://192.168.1.1",
            "http://[::1]",
        ] {
            let erro = ch
                .send_activity(
                    &auth::ServiceUrlVerificada::para_teste(interna),
                    "conv-1",
                    "oi",
                )
                .await
                .expect_err("{interna} tinha de ser recusada");
            let msg = erro.to_string();
            assert!(
                msg.contains("recusada") || msg.contains("recusado"),
                "{interna} devia ser barrada pelo guard, veio: {msg}"
            );
        }
    }

    /// Esquema fora de https nao passa: o Bot Framework e https, e um
    /// `file://` ou `gopher://` aqui so serviria para alcancar outra coisa.
    #[tokio::test]
    async fn esquema_nao_https_e_recusado() {
        let ch = canal_autenticado();
        for torta in [
            "http://smba.trafficmanager.net",
            "file:///etc/passwd",
            "gopher://exemplo.invalido",
            "nao-e-url",
            "",
        ] {
            assert!(
                ch.send_activity(
                    &auth::ServiceUrlVerificada::para_teste(torta),
                    "conv-1",
                    "oi"
                )
                .await
                .is_err(),
                "{torta:?} nao deveria passar"
            );
        }
    }

    /// O `conversation_id` tambem vem do corpo. Sem encoding, um `..` nele
    /// sobe niveis de caminho e alcanca outro endpoint com o bearer junto.
    #[test]
    fn conversation_id_e_percent_encodado() {
        assert_eq!(utf8_percent_encode_segmento("conv-1"), "conv-1");
        assert_eq!(utf8_percent_encode_segmento("a/b"), "a%2Fb");
        assert_eq!(utf8_percent_encode_segmento(".."), "..");
        assert_eq!(utf8_percent_encode_segmento("a/../b"), "a%2F..%2Fb");
        assert_eq!(utf8_percent_encode_segmento("a?b#c"), "a%3Fb%23c");
        assert_eq!(utf8_percent_encode_segmento("a b"), "a%20b");
    }

    /// `..` sozinho sobrevive ao encoding (e um segmento valido), entao a
    /// prova de que ele nao escapa e o `Url::join`, nao o encoder: um id de
    /// `..` some no caminho em vez de subir para fora do `serviceUrl`.
    /// O que o encoding sozinho **nao** cobre, e por que `Url::join` importa.
    ///
    /// `.` esta na lista de caracteres seguros da RFC 3986, entao `..` passa
    /// pelo encoder intacto — `"../../../v3/other"` vira
    /// `"..%2F..%2Fv3%2Fother"`, sem separador, mas um `conversation_id` de
    /// literalmente `".."` continua sendo um segmento de travessia. Quem
    /// barra esse caso e o `Url::join`, que resolve a travessia **dentro** da
    /// origem: o caminho pode acabar num endpoint errado do mesmo servidor,
    /// e o host nunca muda (RFC 3986 §5.2). Endpoint errado devolve erro do
    /// Bot Framework; host errado seria exfiltracao do bearer. So o segundo
    /// importa, e e o que este teste fixa.
    #[test]
    fn join_nao_deixa_o_caminho_escapar_do_service_url() {
        let base = url::Url::parse("https://smba.trafficmanager.net/amer/").expect("base");
        for id in [
            "../../../v3/other",
            "..",
            ".",
            "../..",
            "%2e%2e",
            "/etc/passwd",
        ] {
            let alvo = base
                .join(&format!(
                    "v3/conversations/{}/activities",
                    utf8_percent_encode_segmento(id)
                ))
                .expect("join");
            assert_eq!(
                alvo.host_str(),
                Some("smba.trafficmanager.net"),
                "o host nunca pode mudar, id={id:?} deu {alvo}"
            );
            assert_eq!(
                alvo.scheme(),
                "https",
                "o esquema nunca pode mudar, id={id:?}"
            );
        }
    }

    #[test]
    fn channel_type_is_teams() {
        let on_msg: TeamsOnMessageFn = Arc::new(|_conv, _uid, _user, _text, _delta_tx| {
            Box::pin(async { Ok("test".to_string()) })
        });
        let config = TeamsConfig {
            app_id: "test-app-id".into(),
            app_secret: "test-secret".into(),
            tenant_id: "test-tenant".into(),
            name: "teams-teste".into(),
        };
        let channel = TeamsChannel::new(config, on_msg).expect("app_id nao vazio");
        assert_eq!(channel.channel_type(), "teams");
        assert_eq!(channel.display_name(), "Microsoft Teams");
        assert_eq!(channel.status(), ChannelStatus::Disconnected);
    }

    #[tokio::test]
    async fn send_message_missing_metadata() {
        let on_msg: TeamsOnMessageFn = Arc::new(|_conv, _uid, _user, _text, _delta_tx| {
            Box::pin(async { Ok("test".to_string()) })
        });
        let config = TeamsConfig {
            app_id: "test-app".into(),
            app_secret: "test-secret".into(),
            tenant_id: "test-tenant".into(),
            name: "teams-teste".into(),
        };
        let channel = TeamsChannel::new(config, on_msg).expect("app_id nao vazio");
        let msg = Message::text(
            SessionId::from_string("s"),
            ChannelId::from_string("c"),
            UserId::from_string("u"),
            MessageDirection::Outgoing,
            "hello",
        );
        let result = channel.send_message(&msg).await;
        assert!(result.is_err());
    }

    #[test]
    fn initial_status_is_disconnected() {
        let on_msg: TeamsOnMessageFn = Arc::new(|_conv, _uid, _user, _text, _delta_tx| {
            Box::pin(async { Ok("test".to_string()) })
        });
        let config = TeamsConfig {
            app_id: "test".into(),
            app_secret: "secret".into(),
            tenant_id: "tenant".into(),
            name: "teams-teste".into(),
        };
        let channel = TeamsChannel::new(config, on_msg).expect("app_id nao vazio");
        assert_eq!(channel.status(), ChannelStatus::Disconnected);
    }

    #[test]
    fn display_name_is_microsoft_teams() {
        let on_msg: TeamsOnMessageFn = Arc::new(|_conv, _uid, _user, _text, _delta_tx| {
            Box::pin(async { Ok("test".to_string()) })
        });
        let config = TeamsConfig {
            app_id: "a".into(),
            app_secret: "b".into(),
            tenant_id: "c".into(),
            name: "teams-teste".into(),
        };
        let channel = TeamsChannel::new(config, on_msg).expect("app_id nao vazio");
        assert_eq!(channel.display_name(), "Microsoft Teams");
    }
}
