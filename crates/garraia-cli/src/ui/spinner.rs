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
//! Latência e tempo decorrido (#936) também são derivados de **ticks**, nunca
//! de relógio: o spinner só fica visível depois de [`SHOW_DELAY_TICKS`]
//! (resposta rápida não pisca nada) e o rótulo `4.7s` aparece depois de
//! [`ELAPSED_AFTER_TICKS`], calculado como `ticks × FRAME_INTERVAL_MS`.
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
///
/// A lista é **intercalada de propósito** (#936): três mensagens profissionais
/// para cada uma com a personalidade do Garra (as quatro citadas na própria
/// issue). A rotação é sequencial, então a proporção 3:1 sai da ordem da
/// lista — nada de `rand`. O teste
/// `personality_messages_stay_in_the_minority_and_spread_out` fixa a razão e
/// o espaçamento.
const MESSAGES_UNICODE: &[&str] = &[
    "Processando…",
    "Gerando a resposta…",
    "Consultando o modelo…",
    "Afiando as garras…",
    "Analisando o pedido…",
    "Organizando os tokens…",
    "Reunindo o contexto…",
    "Caçando uma boa resposta…",
    "Preparando a resposta…",
    "Verificando os detalhes…",
    "Compondo o texto…",
    "Farejando bugs…",
    "Revisando a resposta…",
    "Trabalhando nisso…",
    "Quase lá…",
    "Servindo café aos transistores…",
];

/// As mesmas mensagens sem acento e sem reticências tipográficas.
///
/// Não basta trocar o desenho da garra: numa code page legada do Windows os
/// próprios `ç`, `õ` e `…` viram mojibake. O resto da CLI já segue essa regra
/// (`voce >`, `Ate mais!` em `chat.rs`), e aqui ela vale para o fallback ASCII
/// inteiro — quadro E texto. Índice a índice, o mesmo texto de
/// `MESSAGES_UNICODE`.
const MESSAGES_ASCII: &[&str] = &[
    "Processando...",
    "Gerando a resposta...",
    "Consultando o modelo...",
    "Afiando as garras...",
    "Analisando o pedido...",
    "Organizando os tokens...",
    "Reunindo o contexto...",
    "Cacando uma boa resposta...",
    "Preparando a resposta...",
    "Verificando os detalhes...",
    "Compondo o texto...",
    "Farejando bugs...",
    "Revisando a resposta...",
    "Trabalhando nisso...",
    "Quase la...",
    "Servindo cafe aos transistores...",
];

/// Quadros por segundo do desenho da garra.
pub const FRAME_INTERVAL_MS: u64 = 90;

/// Quantos quadros uma mensagem dura antes de rodar para a próxima
/// (`28 * 90ms` ≈ 2,5 s).
const FRAMES_PER_MESSAGE: usize = 28;

/// Ticks antes do primeiro quadro visível (`3 * 90ms` ≈ 270 ms, dentro da
/// janela de ~250-350 ms do #936). Resposta que chega antes disso nunca
/// produz um quadro — sem flash para limpar. Derivado de ticks, não de
/// relógio: o estado continua puro.
const SHOW_DELAY_TICKS: usize = 3;

/// Ticks até o tempo decorrido entrar na linha (`28 * 90ms` ≈ 2,5 s — junto
/// da primeira rotação de mensagem, de propósito: espera curta não precisa de
/// cronômetro, espera longa ganha os dois sinais de vida de uma vez).
const ELAPSED_AFTER_TICKS: usize = FRAMES_PER_MESSAGE;

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

    /// O quadro atual pode ir para a tela? Falso durante a janela de aparição
    /// (#936): resposta rápida termina antes e o usuário nunca vê um flash.
    pub fn visible(&self) -> bool {
        self.ticks >= SHOW_DELAY_TICKS
    }

    /// Tempo decorrido derivado dos ticks (`ticks × 90ms`), com uma casa
    /// decimal — `"4.7s"`. `None` antes de [`ELAPSED_AFTER_TICKS`].
    ///
    /// É o tempo **animado**, não um relógio: sob `MissedTickBehavior::Delay`
    /// ele pode ficar aquém do relógio de parede se o runtime engasgar. A
    /// troca é deliberada — manter `SpinnerState` puro e testável vale mais
    /// que precisão de cronômetro num rótulo de espera.
    pub fn elapsed_label(&self) -> Option<String> {
        if self.ticks < ELAPSED_AFTER_TICKS {
            return None;
        }
        let tenths = (self.ticks as u64 * FRAME_INTERVAL_MS) / 100;
        Some(format!("{}.{}s", tenths / 10, tenths % 10))
    }

    /// Linha completa, já truncada para `width` colunas.
    pub fn line(&self, width: usize) -> String {
        let raw = match self.elapsed_label() {
            Some(elapsed) => format!("{} {} {}", self.frame(), self.message(), elapsed),
            None => format!("{} {}", self.frame(), self.message()),
        };
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

    /// Desenha o quadro atual (se a janela de aparição já passou) e avança o
    /// estado.
    ///
    /// Escreve `\r` + a linha + `\x1b[K` (apaga até o fim da linha), de modo que
    /// uma mensagem curta nunca deixe cauda de uma mensagem longa anterior.
    ///
    /// Durante os primeiros [`SHOW_DELAY_TICKS`] o estado avança mas nada é
    /// escrito (#936): resposta quase instantânea não pisca spinner nenhum, e
    /// `painted` continua falso — então o [`Spinner::clear`] desses turnos
    /// também não escreve nada.
    pub fn render_frame(&mut self, out: &mut (impl io::Write + ?Sized)) {
        if self.state.visible() {
            let line = self.state.line(self.width);
            let _ = write!(out, "\r\x1b[2m{line}\x1b[0m\x1b[K");
            let _ = out.flush();
            self.painted = true;
        }
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

/// A saída é um terminal disposto a receber interface rica?
///
/// Falso em qualquer superfície onde cor e animação seriam ruído ou lixo:
/// stdout redirecionado/pipe, `NO_COLOR`, `TERM=dumb`.
///
/// Desde o #942 quem consome isto é [`Capabilities::detect`](super::Capabilities::detect),
/// o dono único da decisão. Mora aqui porque aqui estava a versão que já
/// olhava as três coisas.
pub(crate) fn stdout_is_rich_terminal() -> bool {
    if !io::stdout().is_terminal() {
        return false;
    }
    if env_is_set("NO_COLOR") {
        return false;
    }
    !std::env::var("TERM").is_ok_and(|t| t == "dumb")
}

/// O escape hatch que desliga **só** a animação, mantendo o resto da
/// interface rica de pé.
pub(crate) fn animation_opted_out() -> bool {
    env_is_set("GARRAIA_NO_SPINNER")
}

/// `NO_COLOR` segue a convenção: presente e não-vazia conta como ligada.
fn env_is_set(key: &str) -> bool {
    std::env::var(key).is_ok_and(|v| !v.is_empty())
}

/// UTF-8 precisa ser afirmado, não presumido: na dúvida, ASCII.
pub(crate) fn locale_supports_unicode() -> bool {
    matches!(detect_style(), SpinnerStyle::Unicode)
}

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

    /// Spinner já além da janela de aparição, pronto para pintar no próximo
    /// `render_frame`. O caminho dentro da janela tem testes próprios.
    fn spinner_past_show_delay(style: SpinnerStyle) -> Spinner {
        let mut spinner = Spinner::new(style, 80, 0);
        let mut sink: Vec<u8> = Vec::new();
        for _ in 0..SHOW_DELAY_TICKS {
            spinner.render_frame(&mut sink);
        }
        assert!(sink.is_empty(), "a janela de aparição não pode pintar");
        spinner
    }

    #[test]
    fn render_frame_writes_a_frame_then_clear_removes_it() {
        let mut spinner = spinner_past_show_delay(SpinnerStyle::Ascii);
        let mut out: Vec<u8> = Vec::new();

        spinner.render_frame(&mut out);
        let painted = String::from_utf8(out.clone()).unwrap();
        assert!(painted.starts_with('\r'), "quadro começa com CR");
        assert!(painted.contains("///"), "quadro desenhado: {painted:?}");

        out.clear();
        spinner.clear(&mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "\r\x1b[2K");
    }

    #[test]
    fn clear_is_idempotent() {
        let mut spinner = spinner_past_show_delay(SpinnerStyle::Ascii);
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

    /// #936: nada aparece dentro da janela de aparição, e um turno que termina
    /// ali dentro também não gera limpeza — não há o que limpar.
    #[test]
    fn nothing_is_painted_during_the_show_delay() {
        let mut spinner = Spinner::new(SpinnerStyle::Ascii, 80, 0);
        let mut out: Vec<u8> = Vec::new();
        for _ in 0..SHOW_DELAY_TICKS {
            spinner.render_frame(&mut out);
        }
        assert!(out.is_empty(), "pintou dentro da janela: {out:?}");

        // Resposta rápida: o clear de um spinner nunca pintado é um no-op.
        spinner.clear(&mut out);
        assert!(out.is_empty(), "limpou o que nunca foi pintado");

        // O tick seguinte à janela pinta.
        spinner.render_frame(&mut out);
        assert!(!out.is_empty(), "primeiro quadro após a janela");
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

    /// #936: profissional na maioria, personalidade na minoria — e espalhada.
    /// As quatro frases de personalidade são exatamente as que a issue cita.
    #[test]
    fn personality_messages_stay_in_the_minority_and_spread_out() {
        const GARRA_FLAVOR: [&str; 4] = [
            "Afiando as garras…",
            "Caçando uma boa resposta…",
            "Farejando bugs…",
            "Servindo café aos transistores…",
        ];
        let positions: Vec<usize> = MESSAGES_UNICODE
            .iter()
            .enumerate()
            .filter(|(_, m)| GARRA_FLAVOR.contains(m))
            .map(|(i, _)| i)
            .collect();
        assert_eq!(positions.len(), GARRA_FLAVOR.len(), "as 4 têm de estar lá");
        assert!(
            positions.len() * 4 <= MESSAGES_UNICODE.len(),
            "personalidade acima de 1/4 da lista"
        );
        // Rotação é sequencial, então "espalhada" = nunca duas adjacentes,
        // inclusive na volta da lista.
        for pair in positions.windows(2) {
            assert!(
                pair[1] - pair[0] >= 2,
                "personalidade adjacente: {positions:?}"
            );
        }
        let wrap_gap = MESSAGES_UNICODE.len() - positions.last().unwrap() + positions[0];
        assert!(
            wrap_gap >= 2,
            "personalidade adjacente na volta: {positions:?}"
        );
    }

    /// #936: o cronômetro só aparece em espera longa, e cresce com os ticks.
    #[test]
    fn elapsed_label_appears_only_after_the_threshold() {
        let mut state = SpinnerState::new(SpinnerStyle::Unicode, 0);
        for _ in 0..(ELAPSED_AFTER_TICKS - 1) {
            assert_eq!(state.elapsed_label(), None, "cedo demais para cronômetro");
            state.tick();
        }
        state.tick();
        // 28 ticks × 90ms = 2520ms → "2.5s"; derivado de ticks, sem relógio.
        assert_eq!(state.elapsed_label().as_deref(), Some("2.5s"));
        assert!(state.line(80).ends_with("2.5s"), "{}", state.line(80));

        for _ in 0..25 {
            state.tick();
        }
        // 53 ticks × 90ms = 4770ms → "4.7s".
        assert_eq!(state.elapsed_label().as_deref(), Some("4.7s"));
    }

    /// A linha com cronômetro continua respeitando a largura do terminal.
    #[test]
    fn elapsed_line_is_still_truncated_to_width() {
        let mut state = SpinnerState::new(SpinnerStyle::Ascii, 0);
        for _ in 0..(ELAPSED_AFTER_TICKS * 3) {
            state.tick();
        }
        assert!(state.elapsed_label().is_some());
        for width in [MIN_WIDTH, 20, 30] {
            let line = state.line(width);
            assert!(line.chars().count() <= width, "estourou {width}: {line:?}");
            assert!(line.is_ascii(), "cronômetro quebrou o fallback: {line:?}");
        }
    }
}
