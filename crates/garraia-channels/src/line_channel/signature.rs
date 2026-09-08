//! Verificacao da assinatura do webhook do LINE (HMAC-SHA256 + Base64).
//!
//! Ate #1051 isto era um stub que devolvia `true` sempre. Quem descobrisse
//! a URL do webhook podia forjar mensagem como se viesse do LINE, porque a
//! unica prova de autenticidade que o protocolo oferece e essa assinatura.
//!
//! O contrato do LINE (Messaging API, "Verify webhook signature"):
//!
//! `Base64( HMAC-SHA256( channel_secret, raw_request_body ) )`
//!
//! comparado com o header `X-Line-Signature`.
//!
//! **Base64, nao hex.** A issue #1051 descreve o header como hex; o LINE
//! usa Base64 padrao (com padding). Um digest de 32 bytes vira 44 chars
//! em Base64 e 64 em hex, entao a escolha errada rejeitaria toda
//! requisicao legitima. Os vetores de teste deste modulo fixam o formato.
//!
//! **O corpo tem de ser o byte a byte recebido.** Serializar de novo o
//! JSON ja parseado muda espacos e ordem de chaves e quebra a assinatura;
//! por isso [`verify_signature`] recebe `&[u8]`, e nao um tipo ja
//! desserializado. Quem escrever a rota do webhook (o wiring e o #1050)
//! precisa ler o body cru antes do parse.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

/// Por que uma assinatura foi recusada.
///
/// O motivo serve para log do operador. **Nao** deve ir para a resposta
/// HTTP: para quem chama de fora, todos os casos sao um 403 igual, senao
/// o proprio erro vira um oraculo que diz o que ajustar na proxima
/// tentativa.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureError {
    /// O canal foi construido sem `channel_secret`. Fail-closed: sem
    /// segredo nao ha o que verificar, entao nada e aceito.
    MissingSecret,
    /// O header `X-Line-Signature` nao veio na requisicao (ou veio vazio).
    MissingHeader,
    /// O header veio, mas nao e Base64 valido.
    MalformedHeader,
    /// Base64 valido com tamanho diferente de 32 bytes. HMAC-SHA256 tem
    /// sempre 32; qualquer outro tamanho e lixo, nao uma assinatura errada.
    WrongLength,
    /// Assinatura bem formada que nao corresponde ao corpo. Este e o caso
    /// que importa: corpo adulterado, segredo errado, ou forja.
    Mismatch,
}

impl SignatureError {
    /// Texto curto para log.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingSecret => "channel_secret nao configurado",
            Self::MissingHeader => "header X-Line-Signature ausente",
            Self::MalformedHeader => "X-Line-Signature nao e Base64 valido",
            Self::WrongLength => "X-Line-Signature nao tem 32 bytes",
            Self::Mismatch => "assinatura nao confere com o corpo",
        }
    }
}

impl std::fmt::Display for SignatureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Calcula a assinatura que o LINE mandaria para este corpo.
///
/// Exposta para os testes e para quem precise assinar uma requisicao de
/// simulacao. `new_from_slice` nao falha porque o HMAC aceita chave de
/// qualquer tamanho, e o unico caso degenerado (chave vazia) e barrado
/// antes, em [`verify_signature`].
pub fn compute_signature(channel_secret: &str, body: &[u8]) -> String {
    let mut mac = HmacSha256::new_from_slice(channel_secret.as_bytes())
        .expect("HMAC-SHA256 aceita chave de qualquer tamanho");
    mac.update(body);
    BASE64.encode(mac.finalize().into_bytes())
}

/// Verifica a assinatura de um webhook do LINE.
///
/// `body` tem de ser o corpo cru da requisicao, byte a byte.
///
/// A comparacao final usa [`subtle::ConstantTimeEq`], para o tempo de
/// resposta nao revelar quantos bytes iniciais o atacante acertou. O
/// descarte por tamanho antes disso nao vaza nada util: 32 bytes e o
/// tamanho publico e fixo de todo HMAC-SHA256.
///
/// Segredo em branco e recusa imediata ([`SignatureError::MissingSecret`]) —
/// sem isso, uma instalacao mal configurada validaria contra uma chave que
/// qualquer um pode reproduzir, e a verificacao viraria teatro.
///
/// `trim().is_empty()` e nao `is_empty()`: a mesma regra do construtor
/// [`super::LineChannel::new`]. Com as duas medindo "vazio" de jeitos
/// diferentes, `channel_secret = "   "` no TOML era recusado pelo canal mas
/// aceito por esta funcao — e ela e API publica reexportada (`line_signature`
/// em `lib.rs`), entao um handler futuro que a chame direto validaria contra
/// a chave `"   "`. `ChannelConfig.settings` e um mapa nao-tipado, entao esse
/// valor passa pela config sem ninguem reclamar.
pub fn verify_signature(
    channel_secret: &str,
    body: &[u8],
    header: &str,
) -> Result<(), SignatureError> {
    if channel_secret.trim().is_empty() {
        return Err(SignatureError::MissingSecret);
    }
    if header.is_empty() {
        return Err(SignatureError::MissingHeader);
    }

    let received = BASE64
        .decode(header)
        .map_err(|_| SignatureError::MalformedHeader)?;
    if received.len() != 32 {
        return Err(SignatureError::WrongLength);
    }

    let mut mac = HmacSha256::new_from_slice(channel_secret.as_bytes())
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

    const SECRET: &str = "segredo-do-canal-de-teste";
    const BODY: &[u8] = br#"{"events":[{"type":"message","replyToken":"abc"}]}"#;

    #[test]
    fn assinatura_gerada_pelo_proprio_modulo_confere() {
        let sig = compute_signature(SECRET, BODY);
        assert_eq!(verify_signature(SECRET, BODY, &sig), Ok(()));
    }

    /// Vetor fixo: prova o formato (Base64, 44 chars com padding) sem
    /// depender de `compute_signature`, que e o codigo sob teste. Se
    /// alguem trocar Base64 por hex, este teste cai.
    #[test]
    fn o_header_e_base64_de_32_bytes_nao_hex() {
        let sig = compute_signature(SECRET, BODY);
        assert_eq!(sig.len(), 44, "Base64 de 32 bytes tem 44 chars: {sig}");
        assert!(sig.ends_with('='), "Base64 de 32 bytes termina em padding");
        let raw = BASE64.decode(&sig).expect("saida deve ser Base64 valido");
        assert_eq!(raw.len(), 32, "HMAC-SHA256 tem 32 bytes");
        assert!(
            sig.chars().any(|c| !c.is_ascii_hexdigit()),
            "se fosse hex, este teste nao distinguiria os dois formatos"
        );
    }

    /// O caso que motivou a issue: corpo alterado depois de assinado.
    #[test]
    fn corpo_adulterado_apos_a_assinatura_e_recusado() {
        let sig = compute_signature(SECRET, BODY);
        let adulterado = br#"{"events":[{"type":"message","replyToken":"XYZ"}]}"#;
        assert_eq!(
            verify_signature(SECRET, adulterado, &sig),
            Err(SignatureError::Mismatch)
        );
    }

    #[test]
    fn segredo_errado_e_recusado() {
        let sig = compute_signature("outro-segredo", BODY);
        assert_eq!(
            verify_signature(SECRET, BODY, &sig),
            Err(SignatureError::Mismatch)
        );
    }

    #[test]
    fn header_ausente_e_recusado() {
        assert_eq!(
            verify_signature(SECRET, BODY, ""),
            Err(SignatureError::MissingHeader)
        );
    }

    #[test]
    fn header_que_nao_e_base64_e_recusado() {
        assert_eq!(
            verify_signature(SECRET, BODY, "!!! nao e base64 !!!"),
            Err(SignatureError::MalformedHeader)
        );
    }

    /// Base64 valido, mas curto demais para ser um HMAC-SHA256.
    #[test]
    fn header_base64_com_tamanho_errado_e_recusado() {
        let curto = BASE64.encode([0u8; 16]);
        assert_eq!(
            verify_signature(SECRET, BODY, &curto),
            Err(SignatureError::WrongLength)
        );
    }

    /// Fail-closed: sem segredo nada passa, nem a assinatura calculada
    /// com a propria chave vazia.
    #[test]
    fn segredo_vazio_recusa_ate_a_assinatura_da_chave_vazia() {
        let sig = compute_signature("", BODY);
        assert_eq!(
            verify_signature("", BODY, &sig),
            Err(SignatureError::MissingSecret)
        );
    }

    /// O construtor recusa `channel_secret = "   "` (`mod.rs`), mas esta
    /// funcao e API publica reexportada e pode ser chamada direto pelo
    /// handler de webhook do #1050. As duas tem de medir "vazio" igual,
    /// senao a mais fraca e justamente a exportada.
    #[test]
    fn segredo_so_com_espacos_e_recusado_como_o_construtor_recusa() {
        for branco in ["   ", "\t", "\n", " \t\n "] {
            let sig = compute_signature(branco, BODY);
            assert_eq!(
                verify_signature(branco, BODY, &sig),
                Err(SignatureError::MissingSecret),
                "segredo {branco:?} nao pode validar nem a propria assinatura"
            );
        }
    }

    /// Corpo vazio ainda tem assinatura valida — o LINE assina o corpo
    /// que mandar, e um corpo vazio nao e motivo para pular a checagem.
    #[test]
    fn corpo_vazio_ainda_e_verificado() {
        let sig = compute_signature(SECRET, b"");
        assert_eq!(verify_signature(SECRET, b"", &sig), Ok(()));
        assert_eq!(
            verify_signature(SECRET, BODY, &sig),
            Err(SignatureError::Mismatch)
        );
    }
}
