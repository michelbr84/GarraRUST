//! Indicador de atividade do `garra chat`.
//!
//! # Por que existe
//!
//! Entre o envio da mensagem e o primeiro token da resposta, o REPL ficava
//! parado no `garra >` sem sinal nenhum de vida. No Ollama — o provedor local
//! padrão — o efeito é pior: `OllamaProvider` não implementa
//! `LlmProvider::stream_complete`, então o runtime cai no caminho batch
//! (`runtime.rs`) e entrega a resposta inteira como um único delta. O usuário
//! encara um prompt morto durante toda a latência e depois vê tudo de uma vez.
//!
//! # Forma
//!
//! O estado é **puro e determinístico**: [`SpinnerState`] só avança quando
//! alguém chama [`SpinnerState::tick`] e nunca lê relógio nem dorme. Quem
//! decide o ritmo é o `tokio::time::interval` do `select!` em `chat.rs`, o que
//! deixa a rotação de quadros e de mensagens testável sem `sleep` real.
//!
//! O [`Spinner`] escreve no mesmo sink `io::Write` por onde os deltas saem, e
//! não no stdout global. É isso que permite afirmar, em teste, que a saída
//! não-TTY não contém um único quadro de animação.
//!
//! # Invariantes
//!
//! - O cursor **nunca** é escondido. Não emitimos `\x1b[?25l` em lugar nenhum,
//!   então não existe caminho de saída — nem `Ctrl+C`, nem panic — capaz de
//!   deixar o terminal do usuário sem cursor.
//! - [`Spinner::clear`] é idempotente: chamar duas vezes não escreve nada na
//!   segunda. `chat.rs` depende disso para limpar incondicionalmente em todos
//!   os caminhos de saída.
//! - As mensagens são rótulos de **atividade**, não descrição de raciocínio.
//!   Nada aqui afirma o que o modelo "está pensando", nem expõe prompt,
//!   ferramenta ou estado interno.

use std::io::{self, IsTerminal};

/// Quadros Unicode: a garra fecha e desfere o bote, depois recolhe.
const FRAMES_UNICODE: &[&str] = &["⟨   ⟩", "⟨╱  ⟩", "⟨╱╱ ⟩", "⟨╱╱╱⟩", "⟨ ╱╱⟩", "⟨  ╱⟩"];

/// Mesma animação em ASCII puro, para terminais sem UTF-8 (notadamente o
/// Windows PowerShell 5.1 numa code page legada).
const FRAMES_ASCII: &[&str] = &["<   >", "</  >", "<// >", "<///>", "< //>", "<  />"];

/// Mensagens de atividade em PT-BR. Curtas, variadas e sem prometer nada sobre
/// o conteúdo da resposta.
const MESSAGES_UNICODE: &[&str] = &[
    "Afiando as garras…",
    "Caçando uma boa resposta…",
    "Organizando os tokens…",
    "Consultando os neurônios de silício…",
    "Farejando bugs…",
    "Desembaraçando pensamentos digitais…",
    "Procurando a ponta do código…",
    "Servindo café aos transistores…",
    "Convencendo os bits a cooperarem…",
    "Preparando o bote…",
    "Seguindo o rastro…",
    "Alinhando as vírgulas…",
    "Puxando o fio da meada…",
    "Espreitando a resposta…",
    "Domando os ponteiros…",
    "Lendo as entrelinhas…",
];

/// As mesmas mensagens sem acento e sem reticências tipográficas.
///
/// Não basta trocar o desenho da garra: numa code page legada do Windows os
/// próprios `ç`, `õ` e `…` viram mojibake. O resto da CLI já segue essa regra
/// (`voce >`, `Ate mais!` em `chat.rs`), e aqui ela vale para o fallback ASCII
/// inteiro — quadro E texto. Índice a índice, o mesmo texto de
/// `MESSAGES_UNICODE`.
const MESSAGES_ASCII: &[&str] = &[
    "Afiando as garras...",
    "Cacando uma boa resposta...",
    "Organizando os tokens...",
    "Consultando os neuronios de silicio...",
    "Farejando bugs...",
    "Desembaracando pensamentos digitais...",
    "Procurando a ponta do codigo...",
    "Servindo cafe aos transistores...",
    "Convencendo os bits a cooperarem...",
    "Preparando o bote...",
    "Seguindo o rastro...",
    "Alinhando as virgulas...",
    "Puxando o fio da meada...",
    "Espreitando a resposta...",
    "Domando os ponteiros...",
    "Lendo as entrelinhas...",
];

/// Quadros por segundo do desenho da garra.
pub const FRAME_INTERVAL_MS: u64 = 90;

/// Quantos quadros uma mensagem dura antes de rodar para a próxima
/// (`28 * 90ms` ≈ 2,5 s).
const FRAMES_PER_MESSAGE: usize = 28;

/// Largura assumida quando o terminal não sabe informar a dele.
const FALLBACK_WIDTH: usize = 80;

/// Nunca truncamos abaixo disso — abaixo de ~12 colunas não sobra nada legível.
const MIN_WIDTH: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpinnerStyle {
    Unicode,
    Ascii,
}

impl SpinnerStyle {
    fn frames(self) -> &'static [&'static str] {
        match self {
            SpinnerStyle::Unicode => FRAMES_UNICODE,
            SpinnerStyle::Ascii => FRAMES_ASCII,
        }
    }

    fn messages(self) -> &'static [&'static str] {
        match self {
            SpinnerStyle::Unicode => MESSAGES_UNICODE,
            SpinnerStyle::Ascii => MESSAGES_ASCII,
        }
    }
}

/// Estado puro da animação: sem relógio, sem I/O, sem alocação por quadro.
#[derive(Debug, Clone)]
pub struct SpinnerState {
    style: SpinnerStyle,
    frame: usize,
    ticks: usize,
    message: usize,
}

impl SpinnerState {
    /// `seed` roda a mensagem inicial (o contador de turnos do REPL), para dois
    /// turnos seguidos não abrirem sempre com a mesma frase. Determinístico de
    /// propósito: nada de `rand`.
    pub fn new(style: SpinnerStyle, seed: usize) -> Self {
        Self {
            style,
            frame: 0,
            ticks: 0,
            message: seed % MESSAGES_UNICODE.len(),
        }
    }

    /// Avança um quadro. Único jeito de o estado mudar.
    pub fn tick(&mut self) {
        self.frame = (self.frame + 1) % self.style.frames().len();
        self.ticks += 1;
        if self.ticks.is_multiple_of(FRAMES_PER_MESSAGE) {
            self.message = (self.message + 1) % self.style.messages().len();
        }
    }

    pub fn frame(&self) -> &'static str {
        self.style.frames()[self.frame]
    }

    pub fn message(&self) -> &'static str {
        self.style.messages()[self.message]
    }

    /// Linha completa, já truncada para `width` colunas.
    pub fn line(&self, width: usize) -> String {
        let raw = format!("{} {}", self.frame(), self.message());
        // O marcador de corte segue o estilo: "…" só é seguro onde o resto
        // do texto também é.
        let ellipsis = match self.style {
            SpinnerStyle::Unicode => "…",
            SpinnerStyle::Ascii => "...",
        };
        truncate_to_width(&raw, width.max(MIN_WIDTH), ellipsis)
    }
}

/// Trunca respeitando fronteiras de caractere (as mensagens têm acentuação, e
/// cortar no meio de um `char` produziria bytes inválidos).
fn truncate_to_width(text: &str, width: usize, ellipsis: &str) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    let keep = width.saturating_sub(ellipsis.chars().count());
    let mut out: String = text.chars().take(keep).collect();
    out.push_str(ellipsis);
    out
}

/// Renderizador com estado de "sujeira": sabe se há uma linha na tela para
/// apagar, o que torna [`Spinner::clear`] idempotente.
#[derive(Debug)]
pub struct Spinner {
    state: SpinnerState,
    width: usize,
    /// Há uma linha de spinner desenhada que ainda precisa ser apagada?
    painted: bool,
}

impl Spinner {
    pub fn new(style: SpinnerStyle, width: usize, seed: usize) -> Self {
        Self {
            state: SpinnerState::new(style, seed),
            width: width.max(MIN_WIDTH),
            painted: false,
        }
    }

    /// Desenha o quadro atual e avança o estado.
    ///
    /// Escreve `\r` + a linha + `\x1b[K` (apaga até o fim da linha), de modo que
    /// uma mensagem curta nunca deixe cauda de uma mensagem longa anterior.
    pub fn render_frame(&mut self, out: &mut (impl io::Write + ?Sized)) {
        let line = self.state.line(self.width);
        let _ = write!(out, "\r\x1b[2m{line}\x1b[0m\x1b[K");
        let _ = out.flush();
        self.painted = true;
        self.state.tick();
    }

    /// Apaga a linha do spinner. Idempotente: sem nada desenhado, não escreve.
    pub fn clear(&mut self, out: &mut (impl io::Write + ?Sized)) {
        if !self.painted {
            return;
        }
        let _ = write!(out, "\r\x1b[2K");
        let _ = out.flush();
        self.painted = false;
    }

    #[cfg(test)]
    fn state(&self) -> &SpinnerState {
        &self.state
    }
}

/// Decide se o indicador deve rodar neste ambiente.
///
/// Devolve `None` — spinner desligado — em qualquer superfície onde a animação
/// seria ruído ou lixo: stdout redirecionado/pipe, `NO_COLOR`, `TERM=dumb`, ou
/// o escape hatch explícito `GARRAIA_NO_SPINNER`.
pub fn detect(seed: usize) -> Option<Spinner> {
    if !io::stdout().is_terminal() {
        return None;
    }
    if env_is_set("NO_COLOR") || env_is_set("GARRAIA_NO_SPINNER") {
        return None;
    }
    if std::env::var("TERM").is_ok_and(|t| t == "dumb") {
        return None;
    }
    Some(Spinner::new(detect_style(), detect_width(), seed))
}

/// `NO_COLOR` segue a convenção: presente e não-vazia conta como ligada.
fn env_is_set(key: &str) -> bool {
    std::env::var(key).is_ok_and(|v| !v.is_empty())
}

/// UTF-8 precisa ser afirmado, não presumido: na dúvida, ASCII.
fn detect_style() -> SpinnerStyle {
    for key in ["LC_ALL", "LC_CTYPE", "LANG"] {
        if let Ok(value) = std::env::var(key) {
            if value.is_empty() {
                continue;
            }
            let upper = value.to_uppercase();
            return if upper.contains("UTF-8") || upper.contains("UTF8") {
                SpinnerStyle::Unicode
            } else {
                SpinnerStyle::Ascii
            };
        }
    }
    // Windows não define as variáveis POSIX de locale, mas o console moderno
    // (Windows Terminal, PowerShell 7) renderiza UTF-8 sem problema.
    if cfg!(windows) {
        SpinnerStyle::Unicode
    } else {
        SpinnerStyle::Ascii
    }
}

fn detect_width() -> usize {
    console::Term::stdout()
        .size_checked()
        .map(|(_, cols)| cols as usize)
        .unwrap_or(FALLBACK_WIDTH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_cycles_frames() {
        let mut state = SpinnerState::new(SpinnerStyle::Unicode, 0);
        let first = state.frame();
        let total = FRAMES_UNICODE.len();
        for _ in 0..total {
            state.tick();
        }
        assert_eq!(state.frame(), first, "um ciclo completo volta ao quadro 0");
    }

    #[test]
    fn every_frame_is_visited_before_repeating() {
        let mut state = SpinnerState::new(SpinnerStyle::Unicode, 0);
        let mut seen = vec![state.frame()];
        for _ in 1..FRAMES_UNICODE.len() {
            state.tick();
            seen.push(state.frame());
        }
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), FRAMES_UNICODE.len(), "quadros não se repetem");
    }

    #[test]
    fn message_rotates_on_the_expected_boundary() {
        let mut state = SpinnerState::new(SpinnerStyle::Unicode, 0);
        let first = state.message();
        for _ in 0..(FRAMES_PER_MESSAGE - 1) {
            state.tick();
        }
        assert_eq!(state.message(), first, "não roda antes da fronteira");
        state.tick();
        assert_ne!(state.message(), first, "roda exatamente na fronteira");
    }

    #[test]
    fn seed_rotates_the_opening_message() {
        let a = SpinnerState::new(SpinnerStyle::Unicode, 0);
        let b = SpinnerState::new(SpinnerStyle::Unicode, 1);
        assert_ne!(a.message(), b.message());
    }

    #[test]
    fn ascii_style_is_pure_ascii() {
        // O fallback só serve para algo se ele mesmo não tiver byte não-ASCII.
        for frame in FRAMES_ASCII {
            assert!(frame.is_ascii(), "quadro ASCII contém não-ASCII: {frame}");
        }
        for message in MESSAGES_ASCII {
            assert!(
                message.is_ascii(),
                "mensagem ASCII contém não-ASCII: {message}"
            );
        }
    }

    /// A LINHA INTEIRA precisa ser ASCII no modo fallback, não só o desenho.
    ///
    /// Regressão: a primeira versão trocava só os quadros e continuava
    /// mandando "Caçando…" com cedilha e reticências tipográficas — numa code
    /// page legada do Windows isso vira mojibake, que é justamente o problema
    /// que o fallback existe para evitar.
    #[test]
    fn ascii_style_renders_a_fully_ascii_line() {
        let mut state = SpinnerState::new(SpinnerStyle::Ascii, 0);
        for _ in 0..(FRAMES_PER_MESSAGE * MESSAGES_ASCII.len() + 7) {
            for width in [MIN_WIDTH, 24, 80] {
                let line = state.line(width);
                assert!(line.is_ascii(), "linha ASCII contém não-ASCII: {line:?}");
            }
            state.tick();
        }
    }

    #[test]
    fn ascii_and_unicode_message_lists_stay_in_sync() {
        // Índice a índice: a semente e a rotação são compartilhadas.
        assert_eq!(MESSAGES_UNICODE.len(), MESSAGES_ASCII.len());
    }

    #[test]
    fn unicode_and_ascii_styles_have_matching_frame_counts() {
        assert_eq!(FRAMES_UNICODE.len(), FRAMES_ASCII.len());
    }

    #[test]
    fn line_is_truncated_to_the_terminal_width() {
        let state = SpinnerState::new(SpinnerStyle::Ascii, 0);
        for width in [MIN_WIDTH, 20, 30, 40] {
            let line = state.line(width);
            assert!(
                line.chars().count() <= width,
                "largura {width} estourada por {line:?} ({} chars)",
                line.chars().count()
            );
        }
    }

    #[test]
    fn narrow_width_is_clamped_not_panicking() {
        let state = SpinnerState::new(SpinnerStyle::Unicode, 0);
        // Larguras absurdas não podem entrar em pânico nem gerar string vazia.
        for width in [0, 1, 2, 5] {
            let line = state.line(width);
            assert!(!line.is_empty());
            assert!(line.chars().count() <= MIN_WIDTH);
        }
    }

    #[test]
    fn truncation_never_splits_a_multibyte_char() {
        // "Caçando", "neurônios": cortar por bytes produziria UTF-8 inválido.
        let text = "⟨╱╱╱⟩ Consultando os neurônios de silício…";
        for width in 1..text.chars().count() {
            let cut = truncate_to_width(text, width, "…");
            assert!(cut.chars().count() <= width);
            assert_eq!(cut, String::from_utf8(cut.clone().into_bytes()).unwrap());
        }
    }

    #[test]
    fn render_frame_writes_a_frame_then_clear_removes_it() {
        let mut spinner = Spinner::new(SpinnerStyle::Ascii, 80, 0);
        let mut out: Vec<u8> = Vec::new();

        spinner.render_frame(&mut out);
        let painted = String::from_utf8(out.clone()).unwrap();
        assert!(painted.starts_with('\r'), "quadro começa com CR");
        assert!(painted.contains("<   >"), "quadro desenhado: {painted:?}");

        out.clear();
        spinner.clear(&mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "\r\x1b[2K");
    }

    #[test]
    fn clear_is_idempotent() {
        let mut spinner = Spinner::new(SpinnerStyle::Ascii, 80, 0);
        let mut out: Vec<u8> = Vec::new();
        spinner.render_frame(&mut out);

        out.clear();
        spinner.clear(&mut out);
        let first = out.len();
        assert!(first > 0);

        out.clear();
        spinner.clear(&mut out);
        assert!(out.is_empty(), "segundo clear não escreve nada");
    }

    #[test]
    fn clear_without_render_writes_nothing() {
        let mut spinner = Spinner::new(SpinnerStyle::Ascii, 80, 0);
        let mut out: Vec<u8> = Vec::new();
        spinner.clear(&mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn spinner_never_hides_the_cursor() {
        // Invariante de segurança: nenhum caminho pode deixar o terminal do
        // usuário sem cursor, então a sequência simplesmente não é emitida.
        let mut spinner = Spinner::new(SpinnerStyle::Unicode, 80, 0);
        let mut out: Vec<u8> = Vec::new();
        for _ in 0..(FRAMES_PER_MESSAGE * 2) {
            spinner.render_frame(&mut out);
        }
        spinner.clear(&mut out);
        let rendered = String::from_utf8(out).unwrap();
        assert!(!rendered.contains("\x1b[?25l"), "escondeu o cursor");
        assert!(!rendered.contains("\x1b[?25h"), "mexeu no cursor");
    }

    #[test]
    fn render_advances_the_state() {
        let mut spinner = Spinner::new(SpinnerStyle::Ascii, 80, 0);
        let mut out: Vec<u8> = Vec::new();
        let before = spinner.state().frame();
        spinner.render_frame(&mut out);
        assert_ne!(spinner.state().frame(), before);
    }

    #[test]
    fn messages_are_activity_labels_not_reasoning_claims() {
        // Guarda de produto: as frases são rótulos de atividade. Nada de
        // afirmar o que o modelo "pensa", nem vazar prompt/ferramenta.
        let forbidden = [
            "pensando que",
            "raciocin",
            "prompt",
            "system",
            "tool_call",
            "chain of thought",
        ];
        for message in MESSAGES_UNICODE.iter().chain(MESSAGES_ASCII) {
            let lower = message.to_lowercase();
            for needle in forbidden {
                assert!(
                    !lower.contains(needle),
                    "mensagem {message:?} contém {needle:?}"
                );
            }
        }
    }

    #[test]
    fn messages_are_short_enough_for_a_narrow_terminal() {
        for message in MESSAGES_UNICODE.iter().chain(MESSAGES_ASCII) {
            assert!(
                message.chars().count() <= 40,
                "mensagem longa demais: {message:?}"
            );
            assert!(!message.is_empty());
        }
    }

    #[test]
    fn messages_are_unique() {
        let mut seen: Vec<&str> = MESSAGES_UNICODE.to_vec();
        seen.sort_unstable();
        let total = seen.len();
        seen.dedup();
        assert_eq!(seen.len(), total, "mensagem duplicada na lista");
    }
}
