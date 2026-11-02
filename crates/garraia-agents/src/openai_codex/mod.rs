//! Provider `openai-codex`: o plano do ChatGPT do usuario via **Sign in with
//! ChatGPT** para apps open-source (fluxo oficial da OpenAI).
//!
//! Documentacao seguida (a oficial vence qualquer exemplo de terceiro):
//! <https://developers.openai.com/siwc/token-sharing-open-source> e as
//! subpaginas `sign-in`, `models-and-inference`, `profiles-and-sessions`,
//! `token-reference`, `errors-and-recovery` e `preview-limitations`.
//!
//! O que o fluxo e, em uma frase: OAuth Authorization Code com PKCE S256 num
//! callback loopback em `127.0.0.1`, registro dinamico do cliente
//! (`dynamic_agent_client` → `oaiapp_...` emitido), escopo
//! `chatgpt.tokens.use.direct` para usar o plano, e inferencia no endpoint
//! **publico** `POST https://api.openai.com/v1/responses` com
//! `store: false` e `stream: true`.
//!
//! O que este modulo **nao** faz, de proposito:
//! - nao chama endpoints privados do ChatGPT (`backend-api`): a documentacao
//!   proibe;
//! - nao le credenciais de outras ferramentas (Codex CLI, Hermes): cada
//!   registro e do GarraIA, feito pelo usuario no navegador;
//! - nao troca de cobranca em silencio: erro de plano para a inferencia, e o
//!   texto diz o que fazer.
//!
//! Modulos:
//! - [`oauth`]: PKCE, URL de autorizacao, troca/renovacao/revogacao de
//!   token e validacao do ID token (RS256 contra o JWKS publicado).
//! - [`credenciais`]: o host id estavel, os registros 0600 por conta e a
//!   renovacao serializada entre processos.
//! - [`login`]: o callback loopback e o fluxo completo de login.
//! - `sse`: o leitor dos eventos da Responses API.
//! - `provedor`: o [`OpenAiCodexProvider`] (`LlmProvider`).

pub mod credenciais;
pub mod login;
pub mod oauth;
mod provedor;
mod sse;

pub use credenciais::{ArmazemDeCredenciais, ContaChatGpt, ErroDeConta, RegistroDeCredencial};
pub use login::{LoginEmAndamento, OpcoesDeLogin, ResultadoDoLogin};
pub use provedor::{ModeloDisponivel, OpenAiCodexProvider};

/// Id do provider no runtime e o `provider:` da entrada `llm.<nome>`.
pub const PROVIDER_ID: &str = "openai-codex";
/// Modelo preferencial quando a entrada nao declara outro.
pub const MODELO_PADRAO: &str = "gpt-6.1-sol";
/// `reasoning.effort` enviado quando a entrada nao declara outro.
pub const ESFORCO_PADRAO: &str = "high";
/// Valores de `reasoning.effort` que o modulo aceita na config. O conjunto
/// efetivo depende do modelo (o gpt-6.1-sol aceita low..max); um valor fora
/// do que o modelo aceita volta como erro da propria API, com `error.param`.
pub const ESFORCOS_ACEITOS: [&str; 7] =
    ["none", "minimal", "low", "medium", "high", "xhigh", "max"];

/// Escopos pedidos: identidade (`openid profile email`) mais os do plano
/// (`offline_access resource.invoke chatgpt.tokens.use.direct`).
pub const ESCOPOS: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
/// O escopo sem o qual nao ha inferencia pelo plano.
pub const ESCOPO_DO_PLANO: &str = "chatgpt.tokens.use.direct";
/// O `resource` do pedido de autorizacao, da troca e da renovacao.
pub const RECURSO: &str = "https://api.openai.com/v1";
/// Cliente da **primeira** autorizacao de uma conta (registro dinamico).
/// Nunca e salvo nem usado na troca do codigo.
pub const CLIENTE_DE_REGISTRO: &str = "dynamic_agent_client";
/// `agent_name_hint` do registro: o nome real do app, sempre o mesmo.
pub const NOME_DO_AGENTE: &str = "GarraIA";
/// Porta preferida do callback; se estiver ocupada, uma porta livre.
pub const PORTA_DE_CALLBACK: u16 = 1455;
/// Caminho fixo do callback (so a porta pode variar entre logins).
pub const CAMINHO_DE_CALLBACK: &str = "/auth/callback";
/// Pagina de uso do plano, para os erros de limite.
pub const PAGINA_DE_USO: &str = "https://chatgpt.com/#settings/Usage";

/// Os enderecos do fluxo. `Default` sao os oficiais; os testes apontam para
/// um servidor local.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    pub authorize: String,
    pub token: String,
    pub revoke: String,
    pub jwks: String,
    pub issuer: String,
    pub api_base: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            authorize: "https://auth.openai.com/api/accounts/authorize".into(),
            token: "https://auth.openai.com/api/accounts/oauth/token".into(),
            revoke: "https://auth.openai.com/api/accounts/oauth/revoke".into(),
            jwks: "https://auth.openai.com/.well-known/jwks.json".into(),
            issuer: "https://auth.openai.com".into(),
            api_base: "https://api.openai.com/v1".into(),
        }
    }
}

/// Normaliza um `reasoning_effort` vindo da config: minusculas, sem espacos.
/// `None` quando vazio ou fora de [`ESFORCOS_ACEITOS`].
pub fn esforco_normalizado(valor: &str) -> Option<String> {
    let v = valor.trim().to_ascii_lowercase();
    ESFORCOS_ACEITOS.contains(&v.as_str()).then_some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn esforco_aceita_os_niveis_e_recusa_o_resto() {
        assert_eq!(esforco_normalizado(" High ").as_deref(), Some("high"));
        assert_eq!(esforco_normalizado("max").as_deref(), Some("max"));
        assert_eq!(esforco_normalizado("turbo"), None);
        assert_eq!(esforco_normalizado(""), None);
    }

    #[test]
    fn endpoints_oficiais_sao_os_da_documentacao() {
        let e = Endpoints::default();
        assert_eq!(
            e.authorize,
            "https://auth.openai.com/api/accounts/authorize"
        );
        assert_eq!(e.token, "https://auth.openai.com/api/accounts/oauth/token");
        assert_eq!(e.api_base, "https://api.openai.com/v1");
        assert!(!e.api_base.contains("backend-api"));
        assert!(ESCOPOS.split(' ').any(|s| s == ESCOPO_DO_PLANO));
    }
}
