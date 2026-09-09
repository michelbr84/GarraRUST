//! Autenticacao das requisicoes que o Bot Framework manda ao webhook.
//!
//! Mesma familia do Google Chat (`google_chat::auth`): JWT RS256 no
//! `Authorization`, chaves publicas num JWK Set remoto, cache compartilhado
//! em [`crate::jwks`]. As constantes e uma das claims mudam.
//!
//! ## O `serviceurl` nao e um detalhe — e o que impede exfiltrar o token
//!
//! Esta e a diferenca que importa em relacao ao Google Chat. La a resposta
//! vai para `chat.googleapis.com`, host constante. Aqui a resposta vai para
//! o `serviceUrl` **que veio no corpo da atividade** — host incluido — com o
//! bearer do bot anexado (`TeamsChannel::send_activity`).
//!
//! Sem amarrar esse valor a algo assinado, quem consegue mandar uma
//! atividade escolhe para qual host o gateway faz POST **com a credencial
//! do bot junto**. Nao e so SSRF: e entregar o token do bot a um servidor
//! escolhido pelo atacante.
//!
//! O Bot Framework resolve isso pondo o `serviceurl` como **claim assinada**
//! no proprio token. [`verificar_token`] devolve a claim, e o handler exige
//! que ela case com o `serviceUrl` do corpo. Como a claim e assinada pela
//! Microsoft, forjar um `serviceUrl` exigiria forjar o token.
//!
//! A verificacao de URL de saida (`garraia_common::ssrf`) fica **por baixo**
//! disso, nao no lugar disso: nem um `serviceUrl` legitimamente assinado
//! deveria conseguir apontar para `169.254.169.254`.

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;

use crate::jwks::{JwksCache, JwksError};

/// Emissor de todo token do Bot Framework.
pub const ISSUER: &str = "https://api.botframework.com";

/// JWK Set com as chaves publicas do Bot Framework.
///
/// E o `jwks_uri` publicado em
/// `https://login.botframework.com/v1/.well-known/openidconfiguration`. Fica
/// como constante, e nao descoberto em tempo de execucao, para o boot nao
/// depender de duas requisicoes encadeadas — mas **o documento de metadados
/// e a fonte de verdade**: se a Microsoft mover o endpoint, e la que o novo
/// valor aparece, e esta constante e o que muda.
pub const JWK_URL: &str = "https://login.botframework.com/v1/.well-known/keys";

/// Uma `serviceUrl` que veio de uma **claim assinada** e ja foi conferida
/// contra o corpo.
///
/// Existe para tornar a invariante impossivel de violar em vez de apenas
/// documentada. So [`verificar_token`] constroi este tipo, e
/// [`super::TeamsChannel::send_activity`] so aceita ele — entao nao ha como
/// mandar a resposta (com o bearer do bot junto) para uma URL que veio do
/// corpo sem passar pela verificacao. Com um `&str` no lugar, bastava alguem
/// ler `serviceUrl` do payload e passar adiante; foi exatamente o que a
/// implementacao de `Channel::send_message` fazia.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceUrlVerificada(String);

impl ServiceUrlVerificada {
    /// A URL. Deliberadamente sem `From<String>` e sem construtor publico:
    /// o unico jeito de obter uma e verificando um token.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// So para teste. Fora de `cfg(test)` nao existe construtor nenhum
    /// alem do interno de [`verificar_token`].
    #[cfg(test)]
    pub fn para_teste(url: impl Into<String>) -> Self {
        Self(url.into())
    }
}

impl std::fmt::Display for ServiceUrlVerificada {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Por que um token foi recusado.
///
/// Para log do operador. **Nao** vai para a resposta HTTP: de fora, todos os
/// casos sao o mesmo 401.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// Nao veio `Authorization`, ou nao veio como `Bearer <token>`.
    HeaderAusente,
    /// O header do JWT nao decodifica, ou nao traz `kid`.
    HeaderMalformado,
    /// O `alg` declarado nao e RS256. Recusado antes de qualquer tentativa
    /// de verificar — e o caminho da confusao de algoritmo.
    AlgoritmoErrado,
    /// A chave do `kid` nao esta disponivel.
    ChaveIndisponivel(JwksError),
    /// Assinatura invalida, `iss` errado, `aud` errado ou token expirado.
    TokenInvalido,
    /// O canal esta configurado sem `app_id`. Fail-closed: o `app_id` e a
    /// audiencia esperada, e sem ela nao ha o que verificar.
    SemAppId,
    /// O token nao traz a claim `serviceurl`. Sem ela nao ha nada assinado
    /// para comparar com o `serviceUrl` do corpo, e responder para um host
    /// nao verificado entrega o bearer do bot a quem o escolheu.
    SemServiceUrl,
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HeaderAusente => f.write_str("header Authorization: Bearer ausente"),
            Self::HeaderMalformado => f.write_str("header do JWT ilegivel ou sem kid"),
            Self::AlgoritmoErrado => f.write_str("alg do token nao e RS256"),
            Self::ChaveIndisponivel(e) => write!(f, "chave publica indisponivel: {e}"),
            Self::TokenInvalido => {
                f.write_str("token nao confere (assinatura, iss, aud ou expiracao)")
            }
            Self::SemAppId => f.write_str("app_id nao configurado para este canal"),
            Self::SemServiceUrl => f.write_str("token sem a claim serviceurl"),
        }
    }
}

/// As claims que interessam.
#[derive(Debug, Deserialize)]
struct Claims {
    /// Para onde a resposta pode ir. **Assinada pela Microsoft** — e o que
    /// torna o `serviceUrl` do corpo confiavel, depois de comparado com
    /// esta.
    serviceurl: Option<String>,
}

/// Extrai o token de um header `Authorization`.
///
/// Esquema case-insensitive (RFC 7235 §2.1); o token continua exato.
pub fn extrair_bearer(header: Option<&str>) -> Option<&str> {
    let valor = header?.trim();
    let (esquema, token) = valor.split_once(' ')?;
    if !esquema.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    if token.is_empty() { None } else { Some(token) }
}

/// Verifica um token do Bot Framework e devolve o `serviceurl` assinado.
///
/// `app_id` e o Microsoft App ID do bot, que e a audiencia esperada.
///
/// O retorno **nao** e `()`: e a claim `serviceurl`, porque o chamador
/// precisa dela para decidir se o `serviceUrl` do corpo pode ser usado.
/// Devolver `()` e deixar o handler ler a claim por conta propria seria o
/// jeito de alguem, um dia, esquecer de compara-la.
pub async fn verificar_token(
    jwks: &JwksCache,
    token: &str,
    app_id: &str,
) -> Result<ServiceUrlVerificada, AuthError> {
    if app_id.trim().is_empty() {
        return Err(AuthError::SemAppId);
    }

    let header = decode_header(token).map_err(|_| AuthError::HeaderMalformado)?;

    // Antes de tocar em chave: o `alg` vem do proprio token.
    if header.alg != Algorithm::RS256 {
        return Err(AuthError::AlgoritmoErrado);
    }

    let kid = header.kid.ok_or(AuthError::HeaderMalformado)?;
    let chave = jwks
        .chave_para(&kid)
        .await
        .map_err(AuthError::ChaveIndisponivel)?;

    verificar_com_chave(token, &chave, app_id)
}

/// Monta a politica de validacao.
///
/// A linha do `set_required_spec_claims` e a mesma armadilha documentada em
/// `google_chat::auth`: `set_issuer`/`set_audience` sozinhos so exigem que o
/// claim bata **quando ele existe** — no `jsonwebtoken` 11 o braco
/// `(TryParse::NotPresent, Some(esperado))` cai no `_ => {}`, e
/// `Validation::new` poe so `exp` em `required_spec_claims`. Sem esta linha,
/// um token sem `aud` seria aceito por qualquer bot.
///
/// O metodo **substitui** o conjunto, entao `exp` precisa ser repetido.
fn validacao_para(app_id: &str) -> Validation {
    let mut validation = Validation::new(Algorithm::RS256);
    validation.algorithms = vec![Algorithm::RS256];
    validation.set_issuer(&[ISSUER]);
    validation.set_audience(&[app_id]);
    validation.set_required_spec_claims(&["exp", "iss", "aud"]);
    validation.validate_exp = true;
    validation.validate_nbf = true;
    // Explicito, e nao herdado: 60s e o default do `jsonwebtoken`, e sem
    // esta linha nao se sabe se a janela foi escolhida ou esquecida.
    validation.leeway = 60;
    validation
}

fn verificar_com_chave(
    token: &str,
    chave: &DecodingKey,
    app_id: &str,
) -> Result<ServiceUrlVerificada, AuthError> {
    let dados = decode::<Claims>(token, chave, &validacao_para(app_id))
        .map_err(|_| AuthError::TokenInvalido)?;

    dados
        .claims
        .serviceurl
        .filter(|s| !s.trim().is_empty())
        .map(ServiceUrlVerificada)
        .ok_or(AuthError::SemServiceUrl)
}

/// Duas URLs do Bot Framework designam o mesmo destino quando diferem so
/// pela barra final.
///
/// O `serviceUrl` do corpo e a claim do token vem da mesma origem, mas nao
/// ha garantia de normalizacao identica entre elas — e uma comparacao de
/// string crua rejeitaria atividades legitimas por causa de um `/`. A
/// comparacao e case-insensitive porque esquema e host sao case-insensitive
/// por definicao (RFC 3986 §3.1, §3.2.2).
///
/// Nada alem disso e normalizado de proposito: resolver `..`, decodificar
/// percent-encoding ou ignorar porta transformaria a comparacao numa
/// heuristica, e o ponto dela e ser exata.
pub fn mesma_service_url(a: &str, b: &str) -> bool {
    a.trim_end_matches('/')
        .eq_ignore_ascii_case(b.trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_bem_formado_e_extraido() {
        assert_eq!(
            extrair_bearer(Some("Bearer abc.def.ghi")),
            Some("abc.def.ghi")
        );
        for h in ["bearer t", "BEARER t"] {
            assert_eq!(extrair_bearer(Some(h)), Some("t"), "{h}");
        }
    }

    #[test]
    fn header_sem_bearer_nao_extrai_nada() {
        for h in [
            "",
            "t",
            "Basic dXNlcjpwYXNz",
            "Bearer",
            "Bearer   ",
            "Token t",
        ] {
            assert_eq!(extrair_bearer(Some(h)), None, "{h:?}");
        }
        assert_eq!(extrair_bearer(None), None);
    }

    /// A mesma armadilha do Google Chat, e a razao de ela estar testada duas
    /// vezes: sem `aud` e `iss` em `required_spec_claims`, um token **sem**
    /// esses claims passa — e aqui isso significaria aceitar um token
    /// emitido para outro bot.
    #[test]
    fn a_validacao_exige_aud_e_iss_e_nao_so_os_confere_quando_existem() {
        let v = validacao_para("app-id-do-bot");
        for claim in ["aud", "iss", "exp"] {
            assert!(
                v.required_spec_claims.contains(claim),
                "{claim} tem de ser exigido: {:?}",
                v.required_spec_claims
            );
        }
    }

    #[test]
    fn a_politica_de_validacao_e_a_esperada() {
        let v = validacao_para("app-id-do-bot");
        assert_eq!(v.algorithms, vec![Algorithm::RS256]);
        assert!(v.validate_exp);
        assert!(v.validate_nbf);
        assert_eq!(v.leeway, 60);
        assert!(v.iss.as_ref().is_some_and(|s| s.contains(ISSUER)));
        assert!(v.aud.as_ref().is_some_and(|s| s.contains("app-id-do-bot")));
    }

    #[tokio::test]
    async fn sem_app_id_recusa_antes_de_qualquer_coisa() {
        let jwks = JwksCache::new("http://127.0.0.1:1/nunca-vai-ser-chamado");
        for vazio in ["", "   ", "\t"] {
            assert_eq!(
                verificar_token(&jwks, "token.qualquer.aqui", vazio).await,
                Err(AuthError::SemAppId)
            );
        }
    }

    #[tokio::test]
    async fn alg_errado_recusa_sem_buscar_chave() {
        let jwks = JwksCache::new("http://127.0.0.1:1/nunca-vai-ser-chamado");
        let hs256 = crate::jwks::jwt_de_teste(r#"{"alg":"HS256","typ":"JWT","kid":"k"}"#, "x");
        assert_eq!(
            verificar_token(&jwks, &hs256, "app-id").await,
            Err(AuthError::AlgoritmoErrado)
        );
        // `none` nem existe como variante de `Algorithm`, entao cai antes,
        // no decode do header.
        let none = crate::jwks::jwt_de_teste(r#"{"alg":"none","typ":"JWT","kid":"k"}"#, "");
        assert_eq!(
            verificar_token(&jwks, &none, "app-id").await,
            Err(AuthError::HeaderMalformado)
        );
    }

    #[tokio::test]
    async fn rs256_sem_kid_e_recusado() {
        let jwks = JwksCache::new("http://127.0.0.1:1/nunca-vai-ser-chamado");
        let token = crate::jwks::jwt_de_teste(r#"{"alg":"RS256","typ":"JWT"}"#, "x");
        assert_eq!(
            verificar_token(&jwks, &token, "app-id").await,
            Err(AuthError::HeaderMalformado)
        );
    }

    #[test]
    fn as_constantes_sao_as_do_bot_framework() {
        assert_eq!(ISSUER, "https://api.botframework.com");
        assert!(
            JWK_URL.starts_with("https://login.botframework.com/"),
            "o JWK Set e o do Bot Framework: {JWK_URL}"
        );
    }

    /// So a barra final e o caixa podem diferir. Qualquer outra diferenca e
    /// um host diferente, e comparar frouxo aqui seria devolver ao atacante
    /// a escolha que a claim assinada existe para tirar dele.
    #[test]
    fn service_url_igual_a_menos_de_barra_e_caixa() {
        assert!(mesma_service_url(
            "https://smba.trafficmanager.net/amer/",
            "https://smba.trafficmanager.net/amer"
        ));
        assert!(mesma_service_url(
            "https://SMBA.trafficmanager.net/amer",
            "https://smba.trafficmanager.net/amer"
        ));
        assert!(mesma_service_url(
            "https://a.example/",
            "https://a.example/"
        ));
    }

    #[test]
    fn service_url_de_outro_host_nao_casa() {
        let legitima = "https://smba.trafficmanager.net/amer";
        for impostora in [
            "https://evil.example/amer",
            "https://smba.trafficmanager.net.evil.example/amer",
            "http://smba.trafficmanager.net/amer",
            "https://smba.trafficmanager.net:8443/amer",
            "https://smba.trafficmanager.net/emea",
            "https://smba.trafficmanager.net/amer/../../x",
            "",
        ] {
            assert!(
                !mesma_service_url(legitima, impostora),
                "{impostora:?} nao pode casar com {legitima:?}"
            );
        }
    }
}
