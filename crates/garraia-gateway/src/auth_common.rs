//! Peças compartilhadas entre os guardas de token do gateway (#1045).
//!
//! Nasceram privadas em [`crate::metrics_auth`], que por um tempo foi o único
//! lugar do gateway a comparar um token de portador. Com o gate de
//! `gateway.api_key` em `/api/*` passaram a ser duas — e três contando o
//! `/ws`, que até então tinha comparação própria com saída antecipada por
//! comprimento. Um lugar só evita que os guardas divirjam em silêncio.

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use ring::digest::{SHA256, digest};
use subtle::ConstantTimeEq;

/// RFC 7235 §4.1 exige `WWW-Authenticate` num 401, para o cliente saber que
/// deve repetir com um token. O `realm` diz **qual** credencial falta, e o
/// corpo é constante — nada do que veio no pedido é ecoado de volta.
pub(crate) fn deny_unauthorized(realm: &str, body: &'static str) -> Response {
    let mut resp = (StatusCode::UNAUTHORIZED, body).into_response();
    let valor = format!(r#"Bearer realm="{realm}""#);
    resp.headers_mut().insert(
        header::WWW_AUTHENTICATE,
        HeaderValue::from_str(&valor).unwrap_or(HeaderValue::from_static("Bearer")),
    );
    resp
}

/// Compara dois tokens em tempo constante **também no comprimento**.
///
/// `subtle::ConstantTimeEq::ct_eq` sobre `[u8]` cru devolve `Choice::zero()`
/// na hora quando os tamanhos diferem — essa saída antecipada entrega um
/// oráculo de comprimento a quem consegue medir o tempo de resposta
/// (auditoria M-1, plan 0024). Passar os dois por SHA-256 antes tira a
/// dependência do tamanho; o custo, dois digests de menos de 64 bytes, some
/// diante do próprio round trip HTTP.
pub(crate) fn constant_time_token_eq(a: &[u8], b: &[u8]) -> bool {
    let a_hash = digest(&SHA256, a);
    let b_hash = digest(&SHA256, b);
    a_hash.as_ref().ct_eq(b_hash.as_ref()).unwrap_u8() == 1
}

/// Extrai o token de um header `Authorization`. O esquema é comparado com
/// distinção de maiúsculas — tanto os scrapers do Prometheus quanto o app
/// escrevem `Bearer`.
pub(crate) fn extract_bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let rest = value.strip_prefix("Bearer ")?;
    let trimmed = rest.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_iguais_e_diferentes() {
        assert!(constant_time_token_eq(b"segredo", b"segredo"));
        assert!(!constant_time_token_eq(b"segredo", b"outro"));
        // Comprimentos diferentes tambem passam pelo caminho completo.
        assert!(!constant_time_token_eq(b"a", b"aaaaaaaaaaaaaaaa"));
        assert!(constant_time_token_eq(b"", b""));
    }

    #[test]
    fn bearer_so_com_o_esquema_certo() {
        let mut h = HeaderMap::new();
        h.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer k1"));
        assert_eq!(extract_bearer(&h), Some("k1"));

        for ruim in ["bearer k1", "Basic k1", "Bearer", "Bearer    ", "k1"] {
            let mut h = HeaderMap::new();
            h.insert(header::AUTHORIZATION, HeaderValue::from_str(ruim).unwrap());
            assert_eq!(extract_bearer(&h), None, "aceitou {ruim:?}");
        }
    }

    #[test]
    fn o_401_nomeia_o_realm_e_nao_ecoa_nada() {
        let resp = deny_unauthorized("garraia", "gateway: invalid or missing api key");
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            resp.headers().get(header::WWW_AUTHENTICATE).unwrap(),
            r#"Bearer realm="garraia""#
        );
    }

    /// Um `realm` com aspas quebraria o header; o fallback mantem a resposta
    /// bem formada em vez de entrar em panico num caminho de erro.
    #[test]
    fn realm_invalido_cai_no_fallback_em_vez_de_entrar_em_panico() {
        let resp = deny_unauthorized("com\nquebra", "nao importa");
        assert_eq!(
            resp.headers().get(header::WWW_AUTHENTICATE).unwrap(),
            "Bearer"
        );
    }
}
