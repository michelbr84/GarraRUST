//! Redacao do texto que `garra_send_message` publica num canal real (#1613).
//!
//! Um orquestrador externo escreve o texto; quem o le e uma pessoa num
//! Telegram. Um token colado no meio do pedido (de um log, de um `.env` que o
//! modelo leu) nao pode sair por ali.
//!
//! `garraia_security::redact_secrets` ja cobre os formatos com prefixo
//! reconhecivel: chaves de provider, tokens do GitHub, JWT, AWS, Telegram e
//! senha em connection string. Este modulo acrescenta o que ele deixa passar:
//! `Bearer <token>`, blocos PEM de chave privada e as chaves `sk-…` curtas
//! demais para o padrao de la.
//!
//! A varredura e manual, sem `regex`: um padrao aqui pediria um `LazyLock` com
//! `expect`, e o repo quer esse numero parado. Nada aqui falha ao compilar.

/// O que substitui cada trecho redigido. O mesmo de `redact_secrets`, para o
/// texto redigido nao denunciar qual padrao casou.
const MARCADOR: &str = "[REDACTED]";

/// Redige segredos de um texto que vai para fora do gateway.
///
/// Texto sem nenhum padrao volta identico, byte a byte: a redacao nao pode
/// mexer em mensagem normal.
pub fn redigir(texto: &str) -> String {
    let texto = garraia_security::redact_secrets(texto);
    let texto = mascarar_pem(&texto);
    let texto = mascarar_apos(&texto, "bearer ", 8, eh_token_bearer, false);
    mascarar_apos(&texto, "sk-", 8, eh_token_sk, true)
}

/// Caracteres de um token `Bearer`: alfabeto de base64 e URL, o que um header
/// Authorization carrega na pratica.
fn eh_token_bearer(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~' | '+' | '/' | '=')
}

/// Caracteres de uma chave `sk-…`: os do padrao generico do `garraia-ask`.
fn eh_token_sk(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_')
}

/// Troca o token que segue `marca` (sem diferenciar maiusculas) por
/// [`MARCADOR`], quando ele tem ao menos `minimo` caracteres.
///
/// `exige_fronteira` recusa casar no meio de uma palavra: `sk-` dentro de
/// `task-list` nao e uma chave.
fn mascarar_apos(
    texto: &str,
    marca: &str,
    minimo: usize,
    eh_token: fn(char) -> bool,
    exige_fronteira: bool,
) -> String {
    let minuscula = texto.to_ascii_lowercase();
    let marca_minuscula = marca.to_ascii_lowercase();
    let mut saida = String::with_capacity(texto.len());
    let mut cursor = 0;
    while let Some(achado) = minuscula[cursor..].find(&marca_minuscula) {
        let inicio = cursor + achado;
        let depois = inicio + marca.len();
        let fronteira_ok = !exige_fronteira
            || texto[..inicio]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_ascii_alphanumeric());
        if !fronteira_ok {
            saida.push_str(&texto[cursor..depois]);
            cursor = depois;
            continue;
        }
        let tamanho: usize = texto[depois..]
            .chars()
            .take_while(|c| eh_token(*c))
            .map(char::len_utf8)
            .sum();
        saida.push_str(&texto[cursor..depois]);
        if tamanho >= minimo {
            saida.push_str(MARCADOR);
        } else {
            saida.push_str(&texto[depois..depois + tamanho]);
        }
        cursor = depois + tamanho;
    }
    saida.push_str(&texto[cursor..]);
    saida
}

/// Troca cada bloco `-----BEGIN … PRIVATE KEY-----` ate o `-----END …-----`
/// correspondente por [`MARCADOR`]. Um BEGIN sem END redige ate o fim do texto:
/// na duvida, o que sobra e segredo.
fn mascarar_pem(texto: &str) -> String {
    const INICIO: &str = "-----BEGIN ";
    const FIM: &str = "-----END ";
    let mut saida = String::with_capacity(texto.len());
    let mut cursor = 0;
    while let Some(achado) = texto[cursor..].find(INICIO) {
        let inicio = cursor + achado;
        let corpo = inicio + INICIO.len();
        let cabecalho_fim = texto[corpo..]
            .find("-----")
            .map_or(texto.len(), |k| corpo + k);
        if !texto[corpo..cabecalho_fim].contains("PRIVATE KEY") {
            saida.push_str(&texto[cursor..corpo]);
            cursor = corpo;
            continue;
        }
        saida.push_str(&texto[cursor..inicio]);
        saida.push_str(MARCADOR);
        cursor = match texto[corpo..].find(FIM) {
            Some(k) => {
                let apos_fim = corpo + k + FIM.len();
                texto[apos_fim..]
                    .find("-----")
                    .map_or(texto.len(), |j| apos_fim + j + 5)
            }
            None => texto.len(),
        };
    }
    saida.push_str(&texto[cursor..]);
    saida
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Cada padrao, num pedido de envio, sai redigido e sem o segredo.
    #[test]
    fn cada_padrao_de_segredo_sai_redigido() {
        let casos = [
            // chave OpenRouter (coberta por `redact_secrets`)
            "use sk-or-v1-abcdefghijklmnopqrstuvwxyz0123456789 agora",
            // chave generica curta, so o padrao do redacao.rs
            "chave sk-abc12345 aqui",
            // GitHub PAT classico e fine-grained
            "token ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            "pat github_pat_11ABCDEFG0123456789_abcdefghijklmnop",
            // AWS access key id
            "aws AKIAIOSFODNN7EXAMPLE fim",
            // JWT
            "jwt eyJhbGciOiJIUzI1.eyJzdWIiOiIxMjM0NTY3.SflKxwRJSMeKKF2QT4fwpMeJf36POk",
            // Bearer em texto solto
            "curl -H 'Authorization: Bearer abcdefgh12345678' x",
            // bloco PEM de chave privada
            "-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEF\n-----END PRIVATE KEY-----",
        ];
        let segredos = [
            "sk-or-v1-abcdefghijklmnopqrstuvwxyz0123456789",
            "sk-abc12345",
            "ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            "github_pat_11ABCDEFG0123456789_abcdefghijklmnop",
            "AKIAIOSFODNN7EXAMPLE",
            "eyJhbGciOiJIUzI1",
            "abcdefgh12345678",
            "MIIEvQIBADANBgkqhkiG9w0BAQEF",
        ];
        for (texto, segredo) in casos.iter().zip(segredos) {
            let saida = redigir(texto);
            assert!(
                saida.contains(MARCADOR),
                "nao redigiu `{texto}` -> `{saida}`"
            );
            assert!(!saida.contains(segredo), "vazou `{segredo}` em `{saida}`");
        }
    }

    /// `Bearer` vira `Bearer [REDACTED]`: o esquema fica, o token some.
    #[test]
    fn bearer_mantem_o_esquema_e_some_com_o_token() {
        assert_eq!(
            redigir("Authorization: Bearer abcdefgh12345678"),
            "Authorization: Bearer [REDACTED]"
        );
    }

    /// `sk-` no meio de uma palavra nao e chave.
    #[test]
    fn sk_dentro_de_palavra_nao_e_redigido() {
        assert_eq!(
            redigir("veja task-list-abcdef agora"),
            "veja task-list-abcdef agora"
        );
    }

    /// Um BEGIN de chave sem END nao deixa o resto do texto passar.
    #[test]
    fn pem_sem_end_redige_ate_o_fim() {
        let saida = redigir("antes -----BEGIN PRIVATE KEY----- MIIEsegredo depois");
        assert_eq!(saida, "antes [REDACTED]");
    }

    /// Um BEGIN de certificado (nao chave) fica como esta.
    #[test]
    fn certificado_publico_nao_e_redigido() {
        let texto = "-----BEGIN CERTIFICATE-----\nMIIBcert\n-----END CERTIFICATE-----";
        assert_eq!(redigir(texto), texto);
    }

    /// O texto limpo volta identico, com acento e emoji.
    #[test]
    fn texto_limpo_sai_identico() {
        for texto in [
            "reunião às 15h, pauta: revisar o roteiro",
            "envie o relatório para a equipe 🦀",
            "task-list e sk-learn sao palavras, nao chaves: sk-learn",
            "",
        ] {
            assert_eq!(redigir(texto), texto, "{texto}");
        }
    }
}
