//! Autenticacao das requisicoes que o Google Chat manda ao webhook.
//!
//! O Google Chat nao assina o corpo com um segredo compartilhado, como o
//! LINE faz (`line_channel::signature`). Ele manda um **JWT RS256** no
//! header `Authorization: Bearer <token>`, assinado com a chave privada da
//! conta de servico `chat@system.gserviceaccount.com`, e publica a chave
//! publica correspondente num JWK Set.
//!
//! ## As tres coisas que precisam ser verificadas, e por que nenhuma sobra
//!
//! **Assinatura.** Sem ela qualquer um forja um token. Obvio, e a unica que
//! costuma ser lembrada.
//!
//! **`iss`.** Fixado em [`ISSUER`]. Sem isso, um token assinado por outra
//! conta de servico do Google — e existem milhares — passaria, desde que a
//! chave dela estivesse no conjunto que buscamos. Fixar o emissor e o que
//! amarra a verificacao a *esta* origem.
//!
//! **`aud`.** Esta e a que se perde com mais facilidade, e a que mais
//! importa. **Todo** webhook do Google Chat, de **toda** app do mundo, e
//! assinado pela mesma chave. Uma verificacao que confira so assinatura e
//! `iss` aceitaria um token legitimo emitido para a app de outra pessoa —
//! e essa pessoa pode simplesmente encaminhar o token dela para o nosso
//! endpoint. O `aud` (numero do projeto, ou URL da app, conforme o console
//! da Chat API) e a unica coisa no token que diz "isto e para voce".
//!
//! O `exp` e conferido pelo proprio `jsonwebtoken` quando `validate_exp`
//! esta ligado, que e o default.
//!
//! ## Confusao de algoritmo
//!
//! [`Validation::algorithms`] recebe **exatamente** `[RS256]`. Sem essa
//! restricao, um atacante trocaria o `alg` do header por `none` ou por um
//! HMAC cuja "chave" e a chave publica — que e publica. E a mesma defesa
//! que `garraia_auth::jwt` aplica ao HS256.

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode, decode_header};
use serde::Deserialize;

use crate::jwks::{JwksCache, JwksError};

/// Emissor de todo token de webhook do Google Chat.
pub const ISSUER: &str = "chat@system.gserviceaccount.com";

/// JWK Set com as chaves publicas do emissor.
///
/// O formato JWK (e nao o X.509 irmao) porque o `jsonwebtoken` consome JWK
/// direto via `DecodingKey::from_jwk`, sem precisar da feature `use_pem` —
/// que o workspace desliga (`Cargo.toml` raiz).
pub const JWK_URL: &str =
    "https://www.googleapis.com/service_accounts/v1/jwk/chat@system.gserviceaccount.com";

/// Por que um token foi recusado.
///
/// Para log do operador. **Nao** vai para a resposta HTTP: de fora, todos
/// os casos sao o mesmo 401, senao o erro vira um oraculo que diz o que
/// ajustar na proxima tentativa. Mesma regra do `line_channel::signature`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// Nao veio `Authorization`, ou nao veio no formato `Bearer <token>`.
    HeaderAusente,
    /// O header do JWT nao decodifica, ou nao traz `kid`. Sem `kid` nao ha
    /// como escolher a chave — e aceitar "qualquer chave do conjunto"
    /// enfraqueceria a verificacao sem ganho nenhum.
    HeaderMalformado,
    /// O `alg` declarado nao e RS256. Recusado **antes** de qualquer
    /// tentativa de verificar: e o caminho da confusao de algoritmo.
    AlgoritmoErrado,
    /// A chave do `kid` nao esta disponivel.
    ChaveIndisponivel(JwksError),
    /// Assinatura invalida, `iss` errado, `aud` errado ou token expirado.
    /// Um caso so de proposito: sao todos "este token nao vale aqui", e
    /// separa-los no log tentaria o proximo leitor a separa-los na
    /// resposta.
    TokenInvalido,
    /// O canal esta configurado sem `audience`. Fail-closed: sem saber para
    /// quem o token deveria ter sido emitido, nao ha o que verificar.
    SemAudience,
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
            Self::SemAudience => f.write_str("audience nao configurada para este canal"),
        }
    }
}

/// As claims que interessam. O Google manda mais; o que nao esta aqui e
/// ignorado de proposito.
#[derive(Debug, Deserialize)]
struct Claims {
    /// Presente para o serde nao reclamar e para deixar explicito que o
    /// `aud` e conferido — pela `Validation`, nao a mao.
    #[allow(dead_code)]
    aud: String,
}

/// Extrai o token de um header `Authorization`.
///
/// O esquema e comparado sem diferenciar maiusculas porque a RFC 7235 §2.1
/// diz que ele e case-insensitive — e um cliente que mande `bearer` nao
/// esta atacando ninguem. O **token** em si continua exato.
pub fn extrair_bearer(header: Option<&str>) -> Option<&str> {
    let valor = header?.trim();
    let (esquema, token) = valor.split_once(' ')?;
    if !esquema.eq_ignore_ascii_case("bearer") {
        return None;
    }
    let token = token.trim();
    if token.is_empty() { None } else { Some(token) }
}

/// Verifica um token de webhook do Google Chat.
///
/// `audience` e o valor configurado no canal — numero do projeto ou URL da
/// app. Vazio e recusa imediata: ver [`AuthError::SemAudience`].
pub async fn verificar_token(
    jwks: &JwksCache,
    token: &str,
    audience: &str,
) -> Result<(), AuthError> {
    if audience.trim().is_empty() {
        return Err(AuthError::SemAudience);
    }

    let header = decode_header(token).map_err(|_| AuthError::HeaderMalformado)?;

    // Antes de tocar em chave: o `alg` vem do proprio token, ou seja de
    // quem manda a requisicao. Recusar aqui fecha a confusao de algoritmo
    // no ponto mais cedo possivel.
    if header.alg != Algorithm::RS256 {
        return Err(AuthError::AlgoritmoErrado);
    }

    let kid = header.kid.ok_or(AuthError::HeaderMalformado)?;
    let chave = jwks
        .chave_para(&kid)
        .await
        .map_err(AuthError::ChaveIndisponivel)?;

    verificar_com_chave(token, &chave, audience)
}

/// Monta a politica de validacao.
///
/// Separada e nomeada porque **a garantia deste modulo esta toda aqui**, e a
/// linha do `set_required_spec_claims` nao e obvia.
///
/// ## Por que ela nao e redundante com `set_issuer`/`set_audience`
///
/// Sozinhos, os dois setters so dizem "se o claim vier, tem de bater". No
/// `jsonwebtoken` 11 o casamento e por `match`, e o braco
/// `(TryParse::NotPresent, Some(esperado))` cai no `_ => {}` — ou seja, um
/// token **sem** `aud` passa por `set_audience`, e um token **sem** `iss`
/// passa por `set_issuer`. E `Validation::new` poe **so** `exp` em
/// `required_spec_claims`.
///
/// Sem esta linha a defesa central some sem barulho: um JWT assinado pela
/// chave certa do Google, com `exp` valido e **nenhum** `aud`, seria aceito
/// por todos os canais configurados — exatamente o vetor que a `audience`
/// existe para fechar. Nenhum token legitimo do Chat cai nesse caso, o que
/// piora a situacao: a falha nao aparece em teste manual, so em ataque.
///
/// `set_required_spec_claims` **substitui** o conjunto, entao `exp` precisa
/// ser repetido aqui — sem isso a expiracao deixaria de ser exigida.
fn validacao_para(audience: &str) -> Validation {
    let mut validation = Validation::new(Algorithm::RS256);
    // Exatamente um algoritmo aceito. `Validation::new` ja faz isso, mas
    // deixar explicito impede que um `insert` futuro passe despercebido.
    validation.algorithms = vec![Algorithm::RS256];
    validation.set_issuer(&[ISSUER]);
    validation.set_audience(&[audience]);
    validation.set_required_spec_claims(&["exp", "iss", "aud"]);
    validation.validate_exp = true;
    // Token emitido "para o futuro" nao vale. O Google Chat emite com
    // `nbf == iat`, entao na pratica nunca dispara — e defesa em
    // profundidade, e custa uma linha.
    validation.validate_nbf = true;
    // Explicito, e nao herdado: 60s e o default do `jsonwebtoken`, e sem
    // esta linha o proximo leitor nao sabe se a janela foi escolhida ou
    // esquecida. Cobre relogio dessincronizado sem abrir nada relevante —
    // um token expirado ha um minuto ainda exige a chave privada do Google.
    validation.leeway = 60;
    validation
}

/// A parte pura da verificacao, separada para poder ser testada com uma
/// chave montada no teste, sem rede.
fn verificar_com_chave(token: &str, chave: &DecodingKey, audience: &str) -> Result<(), AuthError> {
    decode::<Claims>(token, chave, &validacao_para(audience))
        .map(|_| ())
        .map_err(|_| AuthError::TokenInvalido)
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
    }

    /// O esquema e case-insensitive pela RFC 7235 §2.1.
    #[test]
    fn o_esquema_e_case_insensitive() {
        for h in ["bearer t", "BEARER t", "BeArEr t"] {
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
            assert_eq!(extrair_bearer(Some(h)), None, "{h:?} nao deveria extrair");
        }
        assert_eq!(extrair_bearer(None), None);
    }

    /// Fail-closed: sem `audience` configurada nao ha o que verificar, e a
    /// recusa vem antes de qualquer trabalho — inclusive antes de tocar no
    /// JWKS, que faria trafego de saida por um canal mal configurado.
    #[tokio::test]
    async fn sem_audience_recusa_antes_de_qualquer_coisa() {
        let jwks = JwksCache::new("http://127.0.0.1:1/nunca-vai-ser-chamado");
        for vazia in ["", "   ", "\t"] {
            assert_eq!(
                verificar_token(&jwks, "token.qualquer.aqui", vazia).await,
                Err(AuthError::SemAudience),
                "audience {vazia:?} tinha de recusar"
            );
        }
    }

    /// Um token que nem parece JWT nao chega ao JWKS.
    #[tokio::test]
    async fn header_ilegivel_recusa_sem_buscar_chave() {
        let jwks = JwksCache::new("http://127.0.0.1:1/nunca-vai-ser-chamado");
        assert_eq!(
            verificar_token(&jwks, "isto nao e um jwt", "1234").await,
            Err(AuthError::HeaderMalformado)
        );
    }

    /// Confusao de algoritmo: `alg: none` e a forma classica.
    ///
    /// Neste `jsonwebtoken` a defesa e ainda mais forte do que o guard de
    /// `alg` deste modulo: `none` **nao existe** como variante de
    /// [`Algorithm`], entao o token nem chega a virar um header — cai em
    /// [`AuthError::HeaderMalformado`], antes de qualquer chave. O que se
    /// afirma aqui e o resultado (recusado, sem tocar no JWKS), nao por qual
    /// dos dois caminhos ele foi recusado: uma atualizacao da biblioteca que
    /// passasse a representar `none` deslocaria o erro para
    /// `AlgoritmoErrado` sem afrouxar nada, e este teste continuaria certo.
    #[tokio::test]
    async fn alg_none_e_recusado_sem_buscar_chave() {
        let jwks = JwksCache::new("http://127.0.0.1:1/nunca-vai-ser-chamado");
        let token = crate::jwks::jwt_de_teste(r#"{"alg":"none","typ":"JWT","kid":"k"}"#, "");
        let erro = verificar_token(&jwks, &token, "1234").await.unwrap_err();
        assert!(
            matches!(
                erro,
                AuthError::HeaderMalformado | AuthError::AlgoritmoErrado
            ),
            "alg=none tem de ser recusado antes da chave, foi: {erro}"
        );
    }

    /// HS256 assinado com a chave publica (que e publica) e a outra metade
    /// da confusao de algoritmo.
    #[tokio::test]
    async fn hs256_e_recusado_mesmo_com_kid_valido() {
        let jwks = JwksCache::new("http://127.0.0.1:1/nunca-vai-ser-chamado");
        let token =
            crate::jwks::jwt_de_teste(r#"{"alg":"HS256","typ":"JWT","kid":"chave-1"}"#, "x");
        assert_eq!(
            verificar_token(&jwks, &token, "1234").await,
            Err(AuthError::AlgoritmoErrado),
            "HS256 tem de cair no guard de algoritmo, nao na busca de chave"
        );
    }

    /// RS256 sem `kid` nao da para verificar: escolher "qualquer chave do
    /// conjunto" enfraqueceria a checagem sem ganho nenhum.
    #[tokio::test]
    async fn rs256_sem_kid_e_recusado() {
        let jwks = JwksCache::new("http://127.0.0.1:1/nunca-vai-ser-chamado");
        let token = crate::jwks::jwt_de_teste(r#"{"alg":"RS256","typ":"JWT"}"#, "x");
        assert_eq!(
            verificar_token(&jwks, &token, "1234").await,
            Err(AuthError::HeaderMalformado)
        );
    }

    /// **O teste do achado HIGH.** `set_issuer` e `set_audience` sozinhos so
    /// exigem que o claim bata **quando ele existe**: no `jsonwebtoken` 11 o
    /// braco `(TryParse::NotPresent, Some(esperado))` cai no `_ => {}`. Sem
    /// `aud` e `iss` em `required_spec_claims`, um JWT assinado pela chave
    /// certa do Google, com `exp` valido e sem esses dois claims, passaria em
    /// todos os canais — o vetor que a `audience` existe para fechar.
    ///
    /// Nenhum token legitimo do Chat cai nesse caso, o que torna a falha
    /// invisivel em teste manual. Este teste e o unico lugar que a percebe.
    #[test]
    fn a_validacao_exige_aud_e_iss_e_nao_so_os_confere_quando_existem() {
        let v = validacao_para("1234567890");
        for claim in ["aud", "iss", "exp"] {
            assert!(
                v.required_spec_claims.contains(claim),
                "{claim} tem de ser exigido, senao um token sem ele passa: {:?}",
                v.required_spec_claims
            );
        }
    }

    /// O resto da politica, junto, para a mudanca de um campo so nao passar
    /// despercebida.
    #[test]
    fn a_politica_de_validacao_e_a_esperada() {
        let v = validacao_para("1234567890");
        assert_eq!(
            v.algorithms,
            vec![Algorithm::RS256],
            "so RS256: qualquer outro abre confusao de algoritmo"
        );
        assert!(v.validate_exp, "token expirado nao vale");
        assert!(v.validate_nbf, "token emitido para o futuro nao vale");
        assert_eq!(v.leeway, 60, "a janela de relogio e escolhida, nao herdada");
        assert!(
            v.iss
                .as_ref()
                .is_some_and(|s| s.len() == 1 && s.contains(ISSUER)),
            "um emissor so, e o do Chat"
        );
        assert_eq!(
            v.aud.as_ref().map(|s| s.len()),
            Some(1),
            "uma audiencia so: a deste canal"
        );
    }

    /// O `iss` esta fixado na constante e nao pode virar configuravel por
    /// acidente: todo token do Chat vem desta conta de servico, e aceitar
    /// outra abriria a porta para qualquer conta de servico do Google cuja
    /// chave estivesse no conjunto.
    #[test]
    fn o_issuer_e_a_conta_de_servico_do_chat() {
        assert_eq!(ISSUER, "chat@system.gserviceaccount.com");
        assert!(
            JWK_URL.starts_with("https://www.googleapis.com/service_accounts/v1/jwk/"),
            "a URL tem de ser o endpoint JWK do Google: {JWK_URL}"
        );
        assert!(JWK_URL.ends_with(ISSUER), "o JWK Set e o do proprio issuer");
    }
}
