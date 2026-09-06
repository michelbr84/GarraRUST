//! Filtro de sequencia de terminal que **sobrevive entre deltas** (#996).
//!
//! O texto do modelo chega em pedacos. Ate aqui ele ia direto para o terminal
//! (`write!(out, "{delta}")`), entao um modelo induzido a emitir `\x1b[2J`
//! limpava a tela de quem estava conversando, e `\x1b[?25l` deixava o terminal
//! sem cursor — a mesma invariante que o #995 restaurou pelo lado da
//! ferramenta.
//!
//! # Por que nao deu para reusar o saneador do #995
//!
//! Aquele recebe a saida da ferramenta **inteira**, de uma vez, e pode decidir
//! olhando o texto todo. Este nao: `\x1b` pode chegar num delta e `[2J` no
//! seguinte. Cada metade e inofensiva isolada, e um filtro sem estado deixaria
//! as duas passarem — o terminal, que so ve a concatenacao, executaria a
//! sequencia. **O estado e a feature**, nao um detalhe de implementacao.
//!
//! # Cor do modelo: bloqueada, e nao e conservadorismo
//!
//! A pergunta em aberto na issue era se o modelo devia poder emitir cor de
//! proposito, como algumas CLIs permitem. A resposta sai de um principio que o
//! projeto ja tem escrito: **respeitar `NO_COLOR`, non-TTY e saida
//! redirecionada**. Cor vinda do modelo passa por cima disso — ela nao sabe se
//! o usuario pediu `NO_COLOR`, se a saida esta indo para um pipe, ou se o
//! terminal e legado. Quem decide cor aqui e o `TerminalRenderer`, olhando
//! `Capabilities`; o modelo escreve texto.
//!
//! O renderer continua colorindo o que ele mesmo emite: o filtro trata do
//! **conteudo**, e a cor e aplicada em volta dele.
//!
//! # Estado puro, sem relogio e sem E/S
//!
//! Como o `SpinnerState` do #936, e pelo mesmo motivo: da para afirmar cada
//! transicao contra uma `String`, inclusive as que dependem de onde o delta
//! foi cortado — que e justamente o que se quer testar aqui.

/// Onde o filtro esta no meio de uma sequencia.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Estado {
    #[default]
    Texto,
    /// Viu `ESC`, esperando saber que tipo de sequencia e.
    ViuEsc,
    /// Dentro de `ESC [` — cor, movimento de cursor, limpeza de tela, `?25l`.
    EmCsi,
    /// Dentro de `ESC ]` — titulo de janela, hyperlink.
    EmOsc,
    /// Dentro de OSC e viu `ESC`, que pode ser o comeco do terminador `ESC \`.
    EmOscViuEsc,
    /// Viu `\r`. Precisa do proximo caractere para saber se e `\r\n` (uma
    /// quebra so) ou um `\r` solto (que vira quebra).
    ViuCr,
}

/// Teto de caracteres consumidos dentro de uma sequencia sem terminador.
///
/// O CSI termina no primeiro byte da faixa `0x40..=0x7e`, que inclui todas as
/// letras — entao ele se resolve sozinho em poucos caracteres. O **OSC** nao:
/// ele so termina em `BEL` ou `ESC \`, e um `ESC ]` solto engoliria a resposta
/// inteira em silencio. Com o teto, o pior caso e perder um trecho curto.
const MAX_DENTRO_DE_SEQUENCIA: usize = 128;

/// Filtro com estado, um por sessao de renderizacao.
#[derive(Debug, Default)]
pub struct AnsiFilter {
    estado: Estado,
    consumidos: usize,
}

impl AnsiFilter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Passa um pedaco do texto e devolve o que pode ir para o terminal.
    ///
    /// O estado fica: um `ESC` no fim deste delta continua valendo no proximo.
    pub fn push(&mut self, delta: &str) -> String {
        let mut out = String::with_capacity(delta.len());

        for c in delta.chars() {
            match self.estado {
                Estado::Texto => self.no_texto(c, &mut out),

                Estado::ViuCr => {
                    self.estado = Estado::Texto;
                    // `\r\n` e uma quebra so; `\r` solto vira quebra e o
                    // caractere atual segue normalmente.
                    out.push('\n');
                    if c != '\n' {
                        self.no_texto(c, &mut out);
                    }
                }

                Estado::ViuEsc => {
                    self.consumidos = 0;
                    match c {
                        '[' => self.estado = Estado::EmCsi,
                        ']' => self.estado = Estado::EmOsc,
                        // Sequencia de dois caracteres (`ESC c` reseta o
                        // terminal). Some inteira.
                        _ => self.estado = Estado::Texto,
                    }
                }

                Estado::EmCsi => {
                    self.consumidos += 1;
                    if ('\u{40}'..='\u{7e}').contains(&c)
                        || self.consumidos > MAX_DENTRO_DE_SEQUENCIA
                    {
                        self.estado = Estado::Texto;
                    }
                }

                Estado::EmOsc => {
                    self.consumidos += 1;
                    if c == '\u{7}' || self.consumidos > MAX_DENTRO_DE_SEQUENCIA {
                        self.estado = Estado::Texto;
                    } else if c == '\u{1b}' {
                        self.estado = Estado::EmOscViuEsc;
                    }
                }

                Estado::EmOscViuEsc => {
                    self.consumidos += 1;
                    // `ESC \` fecha o OSC. Qualquer outra coisa volta para
                    // dentro dele — menos o teto, que sempre solta.
                    self.estado = if c == '\\' || self.consumidos > MAX_DENTRO_DE_SEQUENCIA {
                        Estado::Texto
                    } else {
                        Estado::EmOsc
                    };
                }
            }
        }

        out
    }

    fn no_texto(&mut self, c: char, out: &mut String) {
        match c {
            '\u{1b}' => {
                self.estado = Estado::ViuEsc;
                self.consumidos = 0;
            }
            '\r' => self.estado = Estado::ViuCr,
            '\n' | '\t' => out.push(c),
            // Backspace, tabulacao vertical, form feed, NUL, DEL e o bloco C1
            // (onde mora o CSI de 8 bits). Marcador visivel em vez de sumico:
            // apagar em silencio faria a tentativa parecer texto normal.
            c if c.is_control() || ('\u{80}'..='\u{9f}').contains(&c) => out.push('\u{fffd}'),
            c => out.push(c),
        }
    }

    /// Fecha o fluxo: devolve o que ficou pendente e volta ao estado limpo.
    ///
    /// So o `\r` no fim do ultimo delta produz saida aqui — uma sequencia
    /// inacabada nao vira nada, de proposito. Chamar isso no fim do turno e o
    /// que impede que um `ESC` pendurado no fim de uma resposta engula o
    /// primeiro caractere da proxima.
    pub fn finish(&mut self) -> String {
        let pendente = if self.estado == Estado::ViuCr {
            "\n".to_string()
        } else {
            String::new()
        };
        self.estado = Estado::Texto;
        self.consumidos = 0;
        pendente
    }

    /// Saneia um texto que chega inteiro, sem carregar estado.
    ///
    /// Para aviso, erro e dica — que nao sao streaming, mas tambem carregam
    /// texto que veio de fora (mensagem de erro de provedor, por exemplo).
    pub fn sanitize_once(texto: &str) -> String {
        let mut f = Self::new();
        let mut s = f.push(texto);
        s.push_str(&f.finish());
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Passa o texto em pedacos de `n` caracteres, como o streaming faria.
    fn em_pedacos(texto: &str, n: usize) -> String {
        let chars: Vec<char> = texto.chars().collect();
        let mut f = AnsiFilter::new();
        let mut out = String::new();
        for bloco in chars.chunks(n) {
            let pedaco: String = bloco.iter().collect();
            out.push_str(&f.push(&pedaco));
        }
        out.push_str(&f.finish());
        out
    }

    /// O teste que da razao de existir ao modulo: a sequencia chega partida, e
    /// cada metade e inofensiva sozinha.
    #[test]
    fn sequencia_partida_entre_deltas_nao_passa() {
        let mut f = AnsiFilter::new();
        let a = f.push("tudo bem\u{1b}");
        let b = f.push("[2Jaqui");
        let junto = format!("{a}{b}");
        assert!(!junto.contains('\u{1b}'), "ESC sobreviveu: {junto:?}");
        assert_eq!(junto, "tudo bemaqui");
    }

    /// E vale para qualquer corte, nao so o que eu escolhi para o teste acima.
    #[test]
    fn nenhum_corte_deixa_a_sequencia_passar() {
        let hostil = "antes\u{1b}[2J\u{1b}[?25l\u{1b}]0;INVADIDO\u{7}depois";
        for n in 1..=hostil.chars().count() {
            let saida = em_pedacos(hostil, n);
            assert!(
                !saida.contains('\u{1b}'),
                "ESC passou com pedaco de {n}: {saida:?}"
            );
            assert_eq!(saida, "antesdepois", "corte de {n} mudou o texto");
        }
    }

    /// Cor do modelo tambem sai: quem decide cor e o renderer, olhando
    /// `NO_COLOR` e o resto das capacidades do terminal.
    #[test]
    fn cor_do_modelo_nao_passa() {
        assert_eq!(
            AnsiFilter::sanitize_once("\u{1b}[32mverde\u{1b}[0m"),
            "verde"
        );
    }

    /// Um `ESC ]` sem terminador engoliria a resposta inteira sem o teto.
    #[test]
    fn osc_sem_terminador_nao_engole_o_resto() {
        let entrada = format!("\u{1b}]0;{}FIM", "x".repeat(MAX_DENTRO_DE_SEQUENCIA * 2));
        let saida = AnsiFilter::sanitize_once(&entrada);
        assert!(saida.ends_with("FIM"), "engoliu o resto: {saida:?}");
    }

    /// Texto normal atravessa intacto, acento incluso.
    #[test]
    fn texto_normal_atravessa_intacto() {
        assert_eq!(
            em_pedacos("compilação concluída\ncom açúcar\te tab", 3),
            "compilação concluída\ncom açúcar\te tab"
        );
    }

    #[test]
    fn backspace_e_movimento_vertical_viram_marcador() {
        assert_eq!(AnsiFilter::sanitize_once("a\u{8}b"), "a\u{fffd}b");
        assert_eq!(AnsiFilter::sanitize_once("a\u{b}b"), "a\u{fffd}b");
        assert_eq!(AnsiFilter::sanitize_once("a\u{c}b"), "a\u{fffd}b");
    }

    #[test]
    fn retorno_de_carro_vira_quebra_sem_duplicar() {
        assert_eq!(AnsiFilter::sanitize_once("um\r\ndois"), "um\ndois");
        assert_eq!(AnsiFilter::sanitize_once("um\rdois"), "um\ndois");
        // E o `\r` no fim do fluxo nao se perde.
        assert_eq!(AnsiFilter::sanitize_once("um\r"), "um\n");
        // Inclusive quando o corte cai exatamente entre o `\r` e o `\n`.
        let mut f = AnsiFilter::new();
        let a = f.push("um\r");
        let b = f.push("\ndois");
        assert_eq!(format!("{a}{b}"), "um\ndois");
    }

    /// `finish` limpa o estado: um `ESC` pendurado no fim de um turno nao pode
    /// engolir o primeiro caractere do turno seguinte.
    #[test]
    fn finish_limpa_o_estado_pendente() {
        let mut f = AnsiFilter::new();
        f.push("texto\u{1b}");
        f.finish();
        assert_eq!(f.push("Ainda aqui"), "Ainda aqui");
    }
}
