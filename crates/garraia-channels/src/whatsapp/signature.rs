//! Verificacao da assinatura do webhook do WhatsApp (HMAC-SHA256 + hex).
//!
//! Ate a #1070 nao havia verificacao nenhuma: `whatsapp_webhook` tinha dois
//! extratores e nenhum autenticava. Diferente do LINE (#1051), este canal
//! **estava ligado** — feature no `Cargo.toml` do gateway, call-site no
//! `server.rs`, rota montada no `router.rs`. Quem descobrisse a URL podia
//! mandar mensagem como qualquer numero, gastar token do provider a cada
//! POST, forjar um `from` da allowlist, e numa instalacao nova cair no ramo
//! `needs_owner` do bootstrap e reivindicar o papel de owner.
//!
//! O contrato da Cloud API ("Validating Payloads"):
//!
//! `sha256=` + `hex( HMAC-SHA256( app_secret, raw_request_body ) )`
//!
//! comparado com o header `X-Hub-Signature-256`.
//!
//! **Hex com prefixo, nao Base64.** O LINE usa Base64 puro; a Meta usa hex
//! com o prefixo literal `sha256=`. Um digest de 32 bytes vira 44 chars em
//! Base64 e 64 em hex, entao herdar o formato do LINE rejeitaria toda
//! requisicao legitima. Os vetores de teste deste modulo fixam o formato.
//!
//! **O segredo e o app secret da app da Meta**, nao o `verify_token`. Sao
//! coisas diferentes: o `verify_token` e o handshake unico do registro da
//! URL (o `GET`), e o app secret assina cada `POST` que chega depois. Usar
//! um no lugar do outro nao verifica nada.
//!
//! **O corpo tem de ser o byte a byte recebido.** Re-serializar o JSON ja
//! parseado muda espacos e ordem de chaves e quebra o HMAC; por isso
//! [`verify_signature`] recebe `&[u8]` e o handler le `axum::body::Bytes`
//! como ultimo extrator.

use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// Prefixo obrigatorio do header, definido pela Meta.
const PREFIX: &str = "sha256=";

/// Tamanho de um digest HMAC-SHA256, em bytes.
const DIGEST_LEN: usize = 32;

/// Por que uma assinatura foi recusada.
///
/// O motivo serve para log do operador. **Nao** deve ir para a resposta
/// HTTP: para quem chama de fora todos os casos sao um 403 igual, senao o
/// proprio erro vira um oraculo que diz o que ajustar na proxima tentativa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureError {
    /// O canal foi construido sem `app_secret`. Fail-closed: sem segredo
    /// nao ha o que verificar, entao nada e aceito.
    MissingSecret,
    /// O header `X-Hub-Signature-256` nao veio (ou veio vazio).
    MissingHeader,
    /// O header veio sem o prefixo `sha256=`.
    MissingPrefix,
    /// Depois do prefixo, o resto nao e hex valido.
    MalformedHeader,
    /// Hex valido com tamanho diferente de 32 bytes. HMAC-SHA256 tem sempre
    /// 32; qualquer outro tamanho e lixo, nao uma assinatura errada.
    WrongLength,
    /// Assinatura bem formada que nao corresponde ao corpo. Este e o caso
    /// que importa: corpo adulterado, segredo errado, ou forja.
    Mismatch,
}

impl SignatureError {
    /// Texto curto para log.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingSecret => "app_secret nao configurado",
            Self::MissingHeader => "header X-Hub-Signature-256 ausente",
            Self::MissingPrefix => "X-Hub-Signature-256 sem o prefixo sha256=",
            Self::MalformedHeader => "X-Hub-Signature-256 nao e hex valido",
            Self::WrongLength => "X-Hub-Signature-256 nao tem 32 bytes",
            Self::Mismatch => "assinatura nao confere com o corpo",
        }
    }
}

impl std::fmt::Display for SignatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Decodifica hex minusculo/maiusculo para bytes.
///
/// Escrito a mao em vez de puxar o crate `hex`: sao doze linhas, e o
/// `garraia-channels` ja carrega dependencia demais por feature. Recusa
/// tamanho impar e qualquer digito fora de `[0-9a-fA-F]`.
fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let (pares, resto) = s.as_bytes().as_chunks::<2>();
    debug_assert!(resto.is_empty(), "o tamanho impar ja foi recusado acima");
    let mut out = Vec::with_capacity(pares.len());
    for [hi, lo] in pares {
        let hi = (*hi as char).to_digit(16)?;
        let lo = (*lo as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
    }
    Some(out)
}

/// Calcula o valor completo do header que a Meta mandaria para este corpo,
/// **com** o prefixo `sha256=`.
///
/// Exposta para os testes e para quem precise assinar uma requisicao de
/// simulacao. `new_from_slice` nao falha porque o HMAC aceita chave de
/// qualquer tamanho, e o unico caso degenerado (chave vazia) e barrado
/// antes, em [`verify_signature`].
pub fn compute_signature(app_secret: &str, body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(app_secret.as_bytes())
        .expect("HMAC-SHA256 aceita chave de qualquer tamanho");
    mac.update(body);
    let digest = mac.finalize().into_bytes();
    let mut out = String::with_capacity(PREFIX.len() + DIGEST_LEN * 2);
    out.push_str(PREFIX);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Verifica a assinatura de um webhook do WhatsApp.
///
/// `body` tem de ser o corpo cru da requisicao, byte a byte.
///
/// A comparacao final usa [`subtle::ConstantTimeEq`], para o tempo de
/// resposta nao revelar quantos bytes iniciais o atacante acertou. Os
/// descartes anteriores (prefixo, hex, tamanho) nao vazam nada util: o
/// formato e publico e fixo, documentado pela propria Meta.
///
/// Segredo em branco e recusa imediata ([`SignatureError::MissingSecret`]) —
/// sem isso, uma instalacao mal configurada validaria contra uma chave que
/// qualquer um pode reproduzir, e a verificacao viraria teatro.
///
/// `trim().is_empty()` e nao `is_empty()`: a mesma regra do construtor
/// [`super::WhatsAppChannel::new`], para que `app_secret = "   "` no TOML
/// seja recusado pelos dois. `ChannelConfig.settings` e um mapa nao-tipado,
/// entao esse valor passa pela config sem ninguem reclamar.
pub fn verify_signature(app_secret: &str, body: &[u8], header: &str) -> Result<(), SignatureError> {
    if app_secret.trim().is_empty() {
        return Err(SignatureError::MissingSecret);
    }
    if header.is_empty() {
        return Err(SignatureError::MissingHeader);
    }

    let hex = header
        .strip_prefix(PREFIX)
        .ok_or(SignatureError::MissingPrefix)?;

    let received = decode_hex(hex).ok_or(SignatureError::MalformedHeader)?;
    if received.len() != DIGEST_LEN {
        return Err(SignatureError::WrongLength);
    }

    let mut mac = HmacSha256::new_from_slice(app_secret.as_bytes())
        .expect("HMAC-SHA256 aceita chave de qualquer tamanho");
    mac.update(body);
    let expected = mac.finalize().into_bytes();

    if expected.ct_eq(&received).into() {
        Ok(())
    } else {
        Err(SignatureError::Mismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "app-secret-de-teste";
    const BODY: &[u8] = br#"{"entry":[{"changes":[{"value":{"messages":[]}}]}]}"#;

    fn assinatura_valida() -> String {
        compute_signature(SECRET, BODY)
    }

    #[test]
    fn assinatura_correta_passa() {
        assert!(verify_signature(SECRET, BODY, &assinatura_valida()).is_ok());
    }

    /// O formato e hex com prefixo, nao Base64. Herdar o formato do LINE
    /// aqui rejeitaria toda requisicao legitima da Meta — este teste fixa a
    /// diferenca que a #1070 mandou observar.
    #[test]
    fn o_formato_e_prefixo_mais_64_chars_de_hex() {
        let sig = assinatura_valida();
        assert!(sig.starts_with("sha256="), "falta o prefixo: {sig}");
        let hex = sig.strip_prefix("sha256=").unwrap();
        assert_eq!(hex.len(), 64, "hex de 32 bytes tem 64 chars: {hex}");
        assert!(
            hex.chars().all(|c| c.is_ascii_hexdigit()),
            "so digitos hex: {hex}"
        );
    }

    #[test]
    fn corpo_adulterado_nao_passa() {
        let sig = assinatura_valida();
        let outro = br#"{"entry":[{"changes":[{"value":{"messages":[1]}}]}]}"#;
        assert_eq!(
            verify_signature(SECRET, outro, &sig),
            Err(SignatureError::Mismatch)
        );
    }

    #[test]
    fn segredo_errado_nao_passa() {
        let sig = compute_signature("outro-segredo", BODY);
        assert_eq!(
            verify_signature(SECRET, BODY, &sig),
            Err(SignatureError::Mismatch)
        );
    }

    /// Fail-closed: sem segredo nao ha o que verificar. Sem esta recusa,
    /// uma instalacao sem `app_secret` validaria contra a chave vazia, que
    /// qualquer um reproduz.
    #[test]
    fn segredo_vazio_ou_em_branco_recusa_antes_de_tudo() {
        for secret in ["", "   ", "\t\n"] {
            assert_eq!(
                verify_signature(secret, BODY, &assinatura_valida()),
                Err(SignatureError::MissingSecret),
                "segredo {secret:?} devia ser recusado"
            );
        }
    }

    #[test]
    fn header_ausente_recusa() {
        assert_eq!(
            verify_signature(SECRET, BODY, ""),
            Err(SignatureError::MissingHeader)
        );
    }

    /// Assinatura correta, mas sem o prefixo que a Meta manda. Recusar aqui
    /// e o que impede alguem de mandar o hex cru e passar.
    #[test]
    fn hex_correto_sem_prefixo_recusa() {
        let sig = assinatura_valida();
        let sem_prefixo = sig.strip_prefix("sha256=").unwrap();
        assert_eq!(
            verify_signature(SECRET, BODY, sem_prefixo),
            Err(SignatureError::MissingPrefix)
        );
    }

    #[test]
    fn hex_malformado_recusa() {
        for header in [
            "sha256=zz",  // fora de [0-9a-f]
            "sha256=abc", // tamanho impar
            "sha256=",    // vazio depois do prefixo
            "sha256=nao-e-hex-de-jeito-nenhum",
        ] {
            let r = verify_signature(SECRET, BODY, header);
            assert!(
                matches!(
                    r,
                    Err(SignatureError::MalformedHeader) | Err(SignatureError::WrongLength)
                ),
                "header {header:?} devia ser recusado, veio {r:?}"
            );
        }
    }

    /// Hex valido de tamanho errado. Separado do malformado porque e o caso
    /// que um atacante tentaria de proposito: um digest de outro algoritmo.
    #[test]
    fn hex_valido_de_tamanho_errado_recusa() {
        assert_eq!(
            verify_signature(SECRET, BODY, "sha256=deadbeef"),
            Err(SignatureError::WrongLength)
        );
    }

    /// Hex maiusculo. A Meta manda minusculo, mas aceitar os dois nao
    /// enfraquece nada — o HMAC comparado e o mesmo — e evita um falso
    /// negativo se algum proxy normalizar o header.
    #[test]
    fn hex_maiusculo_tambem_passa() {
        let sig = assinatura_valida();
        let hex = sig.strip_prefix("sha256=").unwrap().to_uppercase();
        assert!(verify_signature(SECRET, BODY, &format!("sha256={hex}")).is_ok());
    }

    /// Corpo vazio ainda produz assinatura verificavel — nao e um caso de
    /// erro, e a Meta pode mandar POST sem `entry`.
    #[test]
    fn corpo_vazio_assina_e_verifica() {
        let sig = compute_signature(SECRET, b"");
        assert!(verify_signature(SECRET, b"", &sig).is_ok());
    }

    /// Cada motivo tem texto proprio para o log do operador — mas nenhum
    /// deles chega na resposta HTTP (ver `webhook.rs`, corpo constante).
    #[test]
    fn todo_motivo_tem_texto_de_log() {
        for e in [
            SignatureError::MissingSecret,
            SignatureError::MissingHeader,
            SignatureError::MissingPrefix,
            SignatureError::MalformedHeader,
            SignatureError::WrongLength,
            SignatureError::Mismatch,
        ] {
            assert!(!e.as_str().is_empty());
            assert_eq!(e.to_string(), e.as_str());
        }
    }
}
