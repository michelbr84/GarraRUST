//! Desenho do QR no terminal.
//!
//! O bridge entrega a **string crua** que o Baileys produz e nunca desenha
//! nada: stdout do bridge e so NDJSON. Quem renderiza e o Rust, aqui.
//!
//! # A escolha de contraste, que e a unica que pode quebrar o scanner
//!
//! Um QR e uma matriz de modulos claros e escuros. Num terminal nao existe
//! "branco" e "preto": existe **glifo** (cor de primeiro plano) e **celula
//! vazia** (cor de fundo). Qual dos dois vira o modulo escuro depende do tema
//! do usuario, que este processo nao conhece.
//!
//! A convencao adotada e a do `qrcode-terminal` em `small` mode — a mesma que
//! o Hermes usa e que funciona na pratica: **desenha-se o modulo CLARO** com
//! blocos (`█`, `▀`, `▄`) e deixa-se o modulo ESCURO como celula vazia. Num
//! terminal de fundo escuro (o default dominante) isso produz modulos claros
//! sobre fundo escuro; num terminal de fundo claro produz o inverso.
//!
//! Isso e seguro porque **as duas polaridades sao lidas** por qualquer scanner
//! moderno — o decodificador detecta a polaridade pelos padroes de
//! posicionamento. O que nao e lido e um QR meio-tom, ou um QR cuja proporcao
//! esta errada. Por isso a regra real e outra: **cada modulo precisa ocupar
//! largura igual a altura**. Meio-bloco resolve isso empilhando duas linhas de
//! modulos numa linha de texto (celula de terminal e ~2:1); o fallback ASCII
//! resolve repetindo o caractere (`##`), nunca com um `#` so.
//!
//! O fallback ASCII mantem a MESMA polaridade do Unicode de proposito. Duas
//! polaridades diferentes significariam que uma das duas saidas esta errada em
//! todo terminal; uma polaridade so significa que as duas estao certas nos
//! mesmos terminais.
//!
//! # Quiet zone
//!
//! Uma margem de 1 modulo, conforme o plano. E menos que os 4 da norma; a
//! aposta e que a borda do terminal e o espaco em volta somam o resto, que e
//! como todo renderizador de QR em terminal se comporta. Quando o terminal e
//! estreito demais para o QR + margem, [`Style::for_terminal`] cai para
//! [`Style::Raw`] em vez de desenhar algo que o scanner nao le.
//!
//! # Invariante de terminal
//!
//! Nada aqui esconde o cursor, emite cor ou move o cursor — pelo mesmo motivo
//! do `spinner.rs`: nao pode existir caminho de saida (Ctrl+C, panic) capaz de
//! deixar o terminal do usuario estragado. [`render`] e uma funcao **pura** que
//! devolve `String`; quem imprime e a CLI.

use qrcode::{EcLevel, QrCode};

/// Largura minima de terminal, em colunas, para o QR de meio-bloco caber.
///
/// Um QR de versao 6 (o tamanho tipico de um link de pareamento do WhatsApp)
/// tem 41 modulos; com a margem de 1 de cada lado sao 43 colunas. O piso de 60
/// e o mesmo que o Hermes documenta e da folga para versoes maiores.
pub const MIN_TERMINAL_COLUMNS: u16 = 60;

/// Margem clara ao redor do simbolo, em modulos.
const QUIET_ZONE: usize = 1;

/// Como desenhar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// Meio-blocos Unicode: 1 celula = 2 modulos na vertical.
    UnicodeHalfBlocks,
    /// `##` por modulo. Duas colunas por modulo para a proporcao fechar.
    Ascii,
    /// Sem desenho: a string crua, para o usuario colar noutro lugar.
    Raw,
}

impl Style {
    /// Escolhe o estilo a partir do que o terminal suporta.
    ///
    /// `unicode` vem do `detect_style()` da CLI (locale), `interactive` de
    /// `IsTerminal`, `columns` da largura do terminal. Pura: o chamador e quem
    /// sonda o ambiente, o que mantem a tabela de decisao testavel.
    pub fn for_terminal(unicode: bool, interactive: bool, columns: Option<u16>) -> Self {
        if !interactive {
            // Pipe, CI, systemd: desenhar um QR seria lixo no log e ninguem
            // apontaria a camera para um arquivo.
            return Style::Raw;
        }
        let wide_enough = columns.is_none_or(|c| c >= MIN_TERMINAL_COLUMNS);
        if !wide_enough {
            // Melhor a string crua do que um QR espremido que nao le.
            return Style::Raw;
        }
        if unicode {
            Style::UnicodeHalfBlocks
        } else {
            Style::Ascii
        }
    }
}

/// Falhas de renderizacao.
#[derive(Debug, thiserror::Error)]
pub enum QrError {
    #[error("nao foi possivel codificar o QR: {0}")]
    Encode(String),
}

/// Desenha `data` no estilo pedido. **Pura**: mesma entrada, mesma saida.
///
/// O nivel de correcao e `L`, o mesmo que o Baileys/`qrcode-terminal` usam:
/// a string de pareamento e longa e um nivel maior estouraria a versao,
/// deixando o simbolo largo demais para caber no terminal.
pub fn render(data: &str, style: Style) -> Result<String, QrError> {
    if style == Style::Raw {
        return Ok(data.to_string());
    }

    let code = QrCode::with_error_correction_level(data.as_bytes(), EcLevel::L)
        .map_err(|e| QrError::Encode(e.to_string()))?;
    let width = code.width();
    let modules = code.to_colors();

    // `true` = modulo ESCURO. A funcao aceita coordenadas fora do simbolo e
    // devolve `false` (claro) — e assim que a quiet zone sai de graca.
    let dark = |x: isize, y: isize| -> bool {
        if x < 0 || y < 0 || x as usize >= width || y as usize >= width {
            return false;
        }
        modules[y as usize * width + x as usize] == qrcode::Color::Dark
    };

    let span = width as isize;
    let quiet = QUIET_ZONE as isize;
    let lo = -quiet;
    let hi = span + quiet;

    let mut out = String::new();
    match style {
        Style::Raw => unreachable!("tratado acima"),
        Style::Ascii => {
            for y in lo..hi {
                for x in lo..hi {
                    // Polaridade: desenha-se o CLARO. Ver a nota do modulo.
                    out.push_str(if dark(x, y) { "  " } else { "##" });
                }
                out.push('\n');
            }
        }
        Style::UnicodeHalfBlocks => {
            let mut y = lo;
            while y < hi {
                for x in lo..hi {
                    let top_light = !dark(x, y);
                    // A ultima linha impar nao tem par; a metade de baixo cai
                    // fora do simbolo e portanto e clara, o que ainda conta
                    // como quiet zone.
                    let bottom_light = !dark(x, y + 1);
                    out.push(match (top_light, bottom_light) {
                        (true, true) => '█',
                        (true, false) => '▀',
                        (false, true) => '▄',
                        (false, false) => ' ',
                    });
                }
                out.push('\n');
                y += 2;
            }
        }
    }
    Ok(out)
}

/// Dica impressa junto com a string crua quando nao da para desenhar.
pub const RAW_HINT_PT: &str = "Terminal sem Unicode ou estreito demais para desenhar o QR. \
Cole a string acima num gerador de QR, ou rode de novo num terminal com \
pelo menos 60 colunas e UTF-8.";

/// Idem, em ingles.
pub const RAW_HINT_EN: &str = "This terminal cannot draw the QR (no Unicode, or fewer than 60 columns). \
Paste the string above into a QR generator, or re-run in a UTF-8 terminal at \
least 60 columns wide.";

#[cfg(test)]
mod tests {
    use super::*;

    /// String fixa: o snapshot so muda se a renderizacao mudar.
    const FIXED: &str = "2@GarraIA/TEST/FIXTURE,abcdef0123456789,==";

    #[test]
    fn style_decision_table() {
        let cases: &[(bool, bool, Option<u16>, Style)] = &[
            (true, true, Some(120), Style::UnicodeHalfBlocks),
            (true, true, Some(60), Style::UnicodeHalfBlocks),
            (true, true, Some(59), Style::Raw),
            (true, true, None, Style::UnicodeHalfBlocks),
            (false, true, Some(120), Style::Ascii),
            (false, true, Some(40), Style::Raw),
            (true, false, Some(200), Style::Raw),
            (false, false, None, Style::Raw),
        ];
        for &(unicode, interactive, columns, expected) in cases {
            assert_eq!(
                Style::for_terminal(unicode, interactive, columns),
                expected,
                "unicode={unicode} interactive={interactive} columns={columns:?}"
            );
        }
    }

    #[test]
    fn raw_style_returns_the_payload_untouched() {
        assert_eq!(render(FIXED, Style::Raw).expect("render"), FIXED);
    }

    #[test]
    fn rendering_is_deterministic() {
        let a = render(FIXED, Style::UnicodeHalfBlocks).expect("a");
        let b = render(FIXED, Style::UnicodeHalfBlocks).expect("b");
        assert_eq!(a, b, "render precisa ser puro");
    }

    /// Snapshot do Unicode: dimensoes e as quatro primeiras linhas.
    ///
    /// O QR inteiro nao entra no fonte (seria ilegivel e quebraria a cada
    /// mudanca de payload). O que importa e fixado: a forma (linhas = metade
    /// dos modulos + margem), o alfabeto (so os quatro glifos) e o fato de a
    /// borda ser clara.
    #[test]
    fn unicode_snapshot_shape() {
        let out = render(FIXED, Style::UnicodeHalfBlocks).expect("render");
        let lines: Vec<&str> = out.lines().collect();

        let code = QrCode::with_error_correction_level(FIXED.as_bytes(), EcLevel::L).expect("qr");
        let span = code.width() + 2 * QUIET_ZONE;
        assert_eq!(lines.len(), span.div_ceil(2), "uma linha por 2 modulos");
        for line in &lines {
            assert_eq!(line.chars().count(), span, "largura = modulos + margem");
            assert!(
                line.chars().all(|c| "█▀▄ ".contains(c)),
                "alfabeto inesperado em {line:?}"
            );
        }

        // A primeira linha e margem (clara) em cima; a metade de baixo dela ja
        // toca o primeiro modulo do simbolo, entao ela nao pode ser so ' '.
        assert!(
            lines[0].chars().any(|c| c == '█' || c == '▀'),
            "a quiet zone precisa ser desenhada como CLARA (bloco), nao vazia"
        );
        // A primeira e a ultima coluna sao margem: sempre claras nas duas
        // metades, logo '█'.
        for line in &lines {
            let mut chars = line.chars();
            assert_eq!(chars.next(), Some('█'), "coluna esquerda e quiet zone");
            assert_eq!(chars.last(), Some('█'), "coluna direita e quiet zone");
        }
    }

    #[test]
    fn ascii_snapshot_shape() {
        let out = render(FIXED, Style::Ascii).expect("render");
        let lines: Vec<&str> = out.lines().collect();

        let code = QrCode::with_error_correction_level(FIXED.as_bytes(), EcLevel::L).expect("qr");
        let span = code.width() + 2 * QUIET_ZONE;
        assert_eq!(lines.len(), span, "uma linha por modulo");
        for line in &lines {
            assert_eq!(line.len(), span * 2, "dois caracteres por modulo");
            assert!(line.starts_with("##"), "margem esquerda e clara");
            assert!(line.ends_with("##"), "margem direita e clara");
        }
        assert_eq!(
            lines[0],
            "##".repeat(span),
            "a primeira linha e margem inteira"
        );
    }

    /// A mesma matriz nos dois estilos: mesma polaridade, mesma geometria.
    #[test]
    fn ascii_and_unicode_agree_on_polarity() {
        let ascii = render(FIXED, Style::Ascii).expect("ascii");
        let unicode = render(FIXED, Style::UnicodeHalfBlocks).expect("unicode");
        let ascii_rows: Vec<&str> = ascii.lines().collect();
        let uni_rows: Vec<&str> = unicode.lines().collect();

        for (i, uni) in uni_rows.iter().enumerate() {
            let top = ascii_rows[i * 2];
            for (col, ch) in uni.chars().enumerate() {
                let ascii_light = &top[col * 2..col * 2 + 2] == "##";
                let uni_top_light = ch == '█' || ch == '▀';
                assert_eq!(
                    ascii_light, uni_top_light,
                    "polaridade divergiu na linha {i} coluna {col}"
                );
            }
        }
    }

    #[test]
    fn output_carries_no_ansi_escape_at_all() {
        for style in [Style::UnicodeHalfBlocks, Style::Ascii, Style::Raw] {
            let out = render(FIXED, style).expect("render");
            assert!(
                !out.contains('\x1b'),
                "{style:?} emitiu escape ANSI — nada aqui pode mexer no terminal"
            );
        }
    }

    #[test]
    fn an_empty_payload_still_encodes() {
        // O bridge nao deveria mandar isto, mas um `unwrap` aqui seria panic
        // em producao; o teste fixa que o caminho existe e nao explode.
        assert!(render("", Style::UnicodeHalfBlocks).is_ok());
    }

    #[test]
    fn an_oversized_payload_is_an_error_not_a_panic() {
        let huge = "x".repeat(8_000);
        assert!(matches!(
            render(&huge, Style::UnicodeHalfBlocks),
            Err(QrError::Encode(_))
        ));
    }
}
