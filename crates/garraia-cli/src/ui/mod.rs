//! Camada de apresentacao do CLI — `UiEvent` + `TerminalRenderer` (#942).
//!
//! Decidida na [ADR 0017](../../../docs/adr/0017-ui-event-terminal-renderer.md).
//! A pergunta que ela responde: **como o runtime conta o que esta fazendo, sem
//! que contar vire desenhar?**
//!
//! ```text
//! AgentRuntime
//!    │
//!    ├── tracing ──────────> arquivo redigido + stderr filtrado (#933)
//!    │
//!    └── UiEvent ──> TerminalRenderer ──> impl io::Write (stdout, ou Vec<u8>)
//! ```
//!
//! # Invariantes que a ADR fixa, e que este modulo carrega
//!
//! 1. **O renderer e chamado de dentro do `select!` do `stream_turn`, nunca de
//!    uma task propria.** E a linha que protege a drenagem do canal limitado:
//!    o runtime empurra deltas com `send().await` num `mpsc` de capacidade
//!    finita, e um receptor que so drena "quando sobra tempo" trava o produtor
//!    — foi o travamento original do `garra chat`. Por isso nada aqui e
//!    `async`, nada aqui spawna, e o renderer nao possui canal nenhum.
//! 2. **Todo caminho de desenho aceita um `impl io::Write`**, entao todo teste
//!    afirma contra um `Vec<u8>` — inclusive os caminhos ASCII, sem cor e
//!    nao-TTY, que sao os que ninguem exercita a mao.
//! 3. **O cursor nunca e escondido.** `\x1b[?25l` nao e emitido em lugar
//!    nenhum, entao nenhum caminho de saida — sucesso, erro, timeout, Ctrl+C,
//!    panico — deixa o terminal sem cursor.
//! 4. **Nao-TTY nao emite sequencia de escape alguma.**
//!
//! # O que o renderer NAO possui
//!
//! - **O relogio.** Ele nao dorme, nao mede tempo de parede e nao spawna: o
//!   ritmo vem de fora, como [`UiEvent::ActivityTick`], emitido pelo
//!   `tokio::time::interval` do `select!`. Mesma regra do `SpinnerState`.
//! - **O `tracing`.** Um [`UiEvent::Warning`] nao vira log, e um `warn!` nao
//!   vira linha de interface. Quando os dois precisam acontecer, quem produz
//!   faz os dois explicitamente — a duplicacao e intencional e visivel, porque
//!   unificar foi o que fez o console virar despejo de INFO (#933).

pub mod ansi_filter;
pub mod conversation;
pub mod spinner;
pub mod tool_log;

use std::io;

pub use conversation::{Header, Style, git_branch, project_markers};
pub use spinner::{FRAME_INTERVAL_MS, Spinner, SpinnerStyle};

/// O que aconteceu, na linguagem da **interface** — nunca na do runtime.
///
/// Deliberadamente pobre: cada variante e algo que muda o que esta na tela.
/// "Chamou a ferramenta X" (#937), "o output era grande demais" (#938) e
/// "o erro tem uma acao sugerida" (#941) entram aqui como variantes novas,
/// nao como `println!` espalhado pelo runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiEvent<'a> {
    /// Um tick do relogio de fora. O renderer nao tem relogio proprio.
    ActivityTick,
    /// Um pedaco do texto da resposta, como saiu do modelo.
    TextDelta(&'a str),
    /// O turno acabou — com sucesso, erro, timeout ou cancelamento. O renderer
    /// nao precisa saber qual: o que ele deve fazer e o mesmo nos quatro.
    TurnFinished,
    /// Aviso dirigido ao usuario. Nao e log.
    Warning(&'a str),
    /// Erro dirigido ao usuario. Nao e log.
    Error(&'a str),
    /// Sugestao discreta logo abaixo de um aviso ou erro — a "Dica:" que
    /// aponta o proximo passo. Sem linha em branco antes, porque ela pertence
    /// a mensagem que acabou de sair.
    Hint(&'a str),

    /// Uma ferramenta comecou (#937). `detail` ja vem redigido e truncado do
    /// `garraia-agents` — o renderer nao ve input cru de ferramenta.
    ToolStarted { name: &'a str, detail: &'a str },

    /// Uma ferramenta terminou (#937).
    ToolFinished {
        name: &'a str,
        duration: std::time::Duration,
        success: bool,
        /// Uma linha sobre o resultado, ja redigida e truncada.
        summary: &'a str,
        /// O numero que o usuario digita no `/tool N` para ver a saida
        /// inteira (#938). `None` quando nada guardou a saida — sem registro
        /// nao ha o que oferecer, e imprimir um numero morto seria pior do
        /// que nao imprimir nenhum.
        indice: Option<usize>,
    },
}

/// Glifos do ciclo de vida de ferramenta (#937), por estado.
///
/// O par Unicode/ASCII anda junto com o resto da interface: num terminal sem
/// UTF-8 o `●` viraria mojibake exatamente como o `❯` viraria.
struct ToolGlyphs {
    ok: &'static str,
    fail: &'static str,
}

const TOOL_GLYPHS_UNICODE: ToolGlyphs = ToolGlyphs {
    ok: "\u{25cf}",
    fail: "\u{d7}",
};
const TOOL_GLYPHS_ASCII: ToolGlyphs = ToolGlyphs { ok: "*", fail: "x" };

/// O ramo da arvore que liga o resultado a linha da ferramenta.
const BRANCH_UNICODE: &str = "  \u{2514}\u{2500} ";
const BRANCH_ASCII: &str = "  |- ";

/// O que este terminal aguenta, decidido **uma vez so**.
///
/// Antes do #942 a decisao morava em dois lugares que foram escritos para
/// concordar e ainda assim divergiam num caso: `Style::detect` olhava TTY,
/// `NO_COLOR` e `TERM=dumb`, mas **nao** o locale, enquanto o spinner olhava o
/// locale para escolher entre quadro Unicode e ASCII. Num terminal
/// interativo com `LANG=C` o resultado era meia interface: `❯` em UTF-8 ao
/// lado de uma animacao ASCII. Agora ha um dono.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// A saida e um terminal que aceita cor e nao pediu para nao receber.
    pub interactive: bool,
    /// O terminal aguenta UTF-8 (derivado do locale).
    pub unicode: bool,
    /// A animacao de atividade foi pedida? `GARRAIA_NO_SPINNER` desliga so ela,
    /// mantendo o resto da interface rica.
    pub animation: bool,
    pub width: usize,
}

impl Capabilities {
    /// Tudo desligado: ASCII puro, sem uma unica sequencia de escape. E o que
    /// sai quando a saida esta redirecionada para arquivo ou pipe.
    pub const PLAIN: Self = Self {
        interactive: false,
        unicode: false,
        animation: false,
        width: conversation::DEFAULT_WIDTH,
    };

    pub fn detect() -> Self {
        let width = conversation::terminal_width();
        if !spinner::stdout_is_rich_terminal() {
            return Self {
                width,
                ..Self::PLAIN
            };
        }
        Self {
            interactive: true,
            unicode: spinner::locale_supports_unicode(),
            animation: !spinner::animation_opted_out(),
            width,
        }
    }

    /// O estilo da conversa (#934/#935) derivado destas capacidades.
    pub fn style(&self) -> Style {
        match (self.unicode, self.interactive) {
            (true, true) => Style::RICH,
            (false, false) => Style::PLAIN,
            // Terminal interativo sem UTF-8 (o `LANG=C` do #942): cor sim,
            // desenho nao.
            (unicode, color) => Style { unicode, color },
        }
    }

    fn spinner_style(&self) -> SpinnerStyle {
        if self.unicode {
            SpinnerStyle::Unicode
        } else {
            SpinnerStyle::Ascii
        }
    }

    /// A animacao para este turno, ou `None` quando o ambiente nao a quer.
    ///
    /// `seed` roda a mensagem inicial (o contador de turnos do REPL), para dois
    /// turnos seguidos nao abrirem com a mesma frase.
    pub fn spinner(&self, seed: usize) -> Option<Spinner> {
        self.animation
            .then(|| Spinner::new(self.spinner_style(), self.width, seed))
    }
}

/// Desenha um turno no terminal.
///
/// Guarda a ordem obrigatoria que antes vivia numa macro dentro do
/// `stream_turn`: apagar a linha de atividade, escrever o rotulo `Garra` uma
/// **unica** vez, so entao o texto do modelo. Sem essa ordem o rotulo sairia
/// no meio da resposta ou colidiria com um quadro da animacao.
#[derive(Debug)]
pub struct TerminalRenderer {
    caps: Capabilities,
    spinner: Option<Spinner>,
    assistant_prefix: String,
    /// O rotulo `Garra` ja saiu neste turno? Uma vez so, sempre.
    prefix_written: bool,
    /// A linha de atividade pode ser desenhada agora?
    ///
    /// Separado do `prefix_written` de proposito (#937): o primeiro token
    /// entrega a linha para a resposta, mas quando uma **ferramenta termina**
    /// o modelo volta a pensar e a animacao deve voltar — sem que o rotulo
    /// seja reimpresso. Era o "retomar o spinner depois da tool" que ficou de
    /// fora do #936 por nao haver evento para ouvir.
    animating: bool,
    /// Filtro de sequencia de terminal para o texto que vem de fora (#996).
    ///
    /// Vive no renderer, e nao numa funcao solta, porque **precisa de estado**:
    /// o texto do modelo e streaming e uma sequencia pode chegar partida entre
    /// dois deltas, cada metade inofensiva sozinha. Ver `ansi_filter`.
    filtro: ansi_filter::AnsiFilter,
}

impl TerminalRenderer {
    pub fn new(caps: Capabilities, spinner: Option<Spinner>) -> Self {
        Self {
            filtro: ansi_filter::AnsiFilter::new(),
            assistant_prefix: caps.style().assistant_prefix(),
            caps,
            spinner,
            prefix_written: false,
            animating: true,
        }
    }

    /// Renderer com rotulo explicito — **so para teste**.
    ///
    /// Os testes do `stream_turn` afirmam a saida byte a byte; derivar o
    /// rotulo do estilo faria cada assercao carregar um `\nGarra\n` que nao e
    /// o que ela esta afirmando. Com o rotulo explicito, cada teste diz o que
    /// testa — e o teste do rotulo continua sendo o daqui.
    #[cfg(test)]
    pub fn with_prefix(
        caps: Capabilities,
        spinner: Option<Spinner>,
        prefix: impl Into<String>,
    ) -> Self {
        Self {
            caps,
            spinner,
            assistant_prefix: prefix.into(),
            prefix_written: false,
            animating: true,
            filtro: ansi_filter::AnsiFilter::new(),
        }
    }

    /// Um turno novo comeca: o rotulo volta a ser devido e a animacao,
    /// se houver, recomeca do zero.
    pub fn begin_turn(&mut self, spinner: Option<Spinner>) {
        self.spinner = spinner;
        self.prefix_written = false;
        self.animating = true;
    }

    /// Ha animacao rodando neste turno? O `select!` usa isto para nao armar o
    /// braco do ticker quando nao ha nada para animar.
    pub fn has_animation(&self) -> bool {
        self.spinner.is_some()
    }

    /// Processa um evento. **Sincrono de proposito** (invariante 1 da ADR).
    pub fn handle(&mut self, event: UiEvent<'_>, out: &mut (impl io::Write + ?Sized)) {
        match event {
            UiEvent::ActivityTick => self.tick(out),
            UiEvent::TextDelta(delta) => self.write_delta(delta, out),
            UiEvent::TurnFinished => self.finish(out),
            UiEvent::Warning(text) | UiEvent::Error(text) => {
                self.write_notice(text, conversation::YELLOW, true, out)
            }
            UiEvent::Hint(text) => self.write_notice(text, conversation::DIM, false, out),
            UiEvent::ToolStarted { name, detail } => self.write_tool_started(name, detail, out),
            UiEvent::ToolFinished {
                name,
                duration,
                success,
                summary,
                indice,
            } => self.write_tool_finished(name, duration, success, summary, indice, out),
        }
    }

    /// Um quadro da animacao — e so antes do primeiro token: depois disso a
    /// linha pertence a resposta, e um quadro colidiria com ela.
    fn tick(&mut self, out: &mut (impl io::Write + ?Sized)) {
        if !self.animating {
            return;
        }
        if let Some(s) = self.spinner.as_mut() {
            s.render_frame(out);
        }
    }

    fn write_delta(&mut self, delta: &str, out: &mut (impl io::Write + ?Sized)) {
        if let Some(s) = self.spinner.as_mut() {
            s.clear(out);
        }
        if !self.prefix_written {
            let _ = write!(out, "{}", self.assistant_prefix);
            self.prefix_written = true;
        }
        // O texto do modelo nao pode escrever comando no terminal (#996). O
        // filtro tem estado porque a sequencia pode chegar partida entre dois
        // deltas — ver `ansi_filter`.
        let seguro = self.filtro.push(delta);
        let _ = write!(out, "{seguro}");
        let _ = out.flush();
        // A linha agora pertence a resposta.
        self.animating = false;
    }

    /// Fim do turno: limpeza **incondicional**, valendo para sucesso, erro do
    /// provedor, timeout e Ctrl+C. `Spinner::clear` e idempotente, entao
    /// repetir e barato — e chamar sempre e o que garante que nenhum caminho
    /// de saida deixe um quadro pendurado na tela.
    ///
    /// O rotulo sai mesmo quando nao veio delta nenhum: e ele que separa o
    /// prompt do usuario do que vier a seguir (o aviso de timeout, o erro),
    /// e o comportamento vinha assim do `stream_turn`.
    fn finish(&mut self, out: &mut (impl io::Write + ?Sized)) {
        if let Some(s) = self.spinner.as_mut() {
            s.clear(out);
        }
        // Fecha o filtro **antes** do rotulo: um `\r` no fim do ultimo delta
        // ainda deve virar quebra, e um `ESC` pendurado nao pode atravessar
        // para o turno seguinte e engolir o primeiro caractere dele (#996).
        let pendente = self.filtro.finish();
        if !pendente.is_empty() {
            let _ = write!(out, "{pendente}");
        }
        if !self.prefix_written {
            let _ = write!(out, "{}", self.assistant_prefix);
            self.prefix_written = true;
        }
        let _ = out.flush();
    }

    /// `● Bash cargo test` — a ferramenta comecou.
    ///
    /// A animacao e limpa antes, sempre: e o criterio de aceite "spinner is
    /// cleared before a tool event is rendered" da #937, e sem isso o quadro
    /// da garra ficaria colado na linha da ferramenta.
    ///
    /// O glifo do inicio e o mesmo do sucesso de proposito. A alternativa —
    /// um `◐` que depois vira `●` na mesma linha — exigiria reescrever uma
    /// linha ja rolada, e este terminal e de fluxo, nao de tela cheia
    /// (ADR 0017, opcao B rejeitada). O resultado aparece na linha seguinte.
    fn write_tool_started(
        &mut self,
        name: &str,
        detail: &str,
        out: &mut (impl io::Write + ?Sized),
    ) {
        if let Some(s) = self.spinner.as_mut() {
            s.clear(out);
        }
        // Enquanto a ferramenta roda, quem ocupa a tela e ela.
        self.animating = false;
        let glifo = self.tool_glyphs().ok;
        let rotulo = tool_label(name);
        let linha = if detail.is_empty() {
            format!("{glifo} {rotulo}")
        } else {
            format!("{glifo} {rotulo} {detail}")
        };
        if self.caps.interactive {
            let _ = writeln!(out, "{}{linha}{}", conversation::DIM, conversation::RESET);
        } else {
            let _ = writeln!(out, "{linha}");
        }
        let _ = out.flush();
    }

    /// `  └─ 148 passed · 6.3s` — o resultado, pendurado na linha anterior.
    ///
    /// Em falha, o glifo muda e a linha inteira sai na cor de aviso: e o par
    /// "estados visualmente distintos" + "falha mostra o trecho mais
    /// relevante" da #937.
    fn write_tool_finished(
        &mut self,
        name: &str,
        duration: std::time::Duration,
        success: bool,
        summary: &str,
        indice: Option<usize>,
        out: &mut (impl io::Write + ?Sized),
    ) {
        if let Some(s) = self.spinner.as_mut() {
            s.clear(out);
        }
        // Ferramenta terminou: o modelo volta a pensar, entao a animacao volta.
        self.animating = true;
        let glifos = self.tool_glyphs();
        let ramo = if self.caps.unicode {
            BRANCH_UNICODE
        } else {
            BRANCH_ASCII
        };
        let separador = if self.caps.unicode { " · " } else { " | " };
        let tempo = format_duration(duration);

        // Sucesso e so o resultado pendurado; falha repete o nome, porque a
        // linha de inicio pode estar longe depois de uma saida longa.
        let (corpo, cor) = if success {
            let corpo = if summary.is_empty() {
                tempo.clone()
            } else {
                format!("{summary}{separador}{tempo}")
            };
            (format!("{ramo}{corpo}"), conversation::DIM)
        } else {
            let corpo = if summary.is_empty() {
                format!("falhou{separador}{tempo}")
            } else {
                format!("{summary}{separador}{tempo}")
            };
            (
                format!("{} {} {ramo}{corpo}", glifos.fail, tool_label(name)),
                conversation::YELLOW,
            )
        };

        // O ponteiro para a saida inteira (#938). Curto de proposito: ele
        // aparece em **toda** linha de ferramenta, entao qualquer coisa mais
        // longa que `#7` viraria ruido repetido a cada chamada. O `/help`
        // explica o que fazer com o numero; aqui basta ele existir.
        let corpo = match indice {
            Some(i) => format!("{corpo}{separador}#{i}"),
            None => corpo,
        };

        if self.caps.interactive {
            let _ = writeln!(out, "{cor}{corpo}{}", conversation::RESET);
        } else {
            let _ = writeln!(out, "{corpo}");
        }
        let _ = out.flush();
    }

    fn tool_glyphs(&self) -> ToolGlyphs {
        if self.caps.unicode {
            TOOL_GLYPHS_UNICODE
        } else {
            TOOL_GLYPHS_ASCII
        }
    }

    /// Aviso, erro ou dica. `blank_line_before` separa a mensagem do que veio
    /// antes; a dica nao a quer, porque pertence a mensagem logo acima.
    fn write_notice(
        &mut self,
        text: &str,
        color: &str,
        blank_line_before: bool,
        out: &mut (impl io::Write + ?Sized),
    ) {
        if let Some(s) = self.spinner.as_mut() {
            s.clear(out);
        }
        let separador = if blank_line_before { "\n" } else { "" };
        // Aviso e erro tambem carregam texto de fora — a mensagem de erro de
        // um provedor, por exemplo, e corpo de resposta HTTP. Nao e streaming,
        // entao vai sem estado (#996).
        let text = ansi_filter::AnsiFilter::sanitize_once(text);
        if self.caps.interactive {
            let _ = writeln!(out, "{separador}{color}{text}{}", conversation::RESET);
        } else {
            let _ = writeln!(out, "{separador}{text}");
        }
        let _ = out.flush();
    }
}

/// Nome da ferramenta como o usuario le: `bash` vira `Bash`, `file_read`
/// vira `Read`.
///
/// Mapa explicito em vez de titlecase automatico: `file_read` viraria
/// "File Read", e o que o operador reconhece e "Read". Ferramenta sem entrada
/// aqui aparece com o nome cru — honesto, e o pior caso e feio, nao errado.
fn tool_label(name: &str) -> String {
    match name {
        "bash" => "Bash".to_string(),
        "file_read" => "Read".to_string(),
        "file_write" => "Write".to_string(),
        "repo_search" => "Search".to_string(),
        "web_search" => "Web".to_string(),
        "web_fetch" => "Fetch".to_string(),
        "git_diff" => "Diff".to_string(),
        "list_dir" => "List".to_string(),
        "run_tests" => "Tests".to_string(),
        outro => outro.to_string(),
    }
}

/// Duracao em uma casa decimal para segundos, inteiro para milissegundos:
/// `6.3s`, `840ms`. Acima de um minuto, `1m03s`.
pub(crate) fn format_duration(d: std::time::Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        return format!("{ms}ms");
    }
    let total_s = d.as_secs();
    if total_s < 60 {
        let decimos = (ms / 100) % 10;
        return format!("{total_s}.{decimos}s");
    }
    format!("{}m{:02}s", total_s / 60, total_s % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rich() -> Capabilities {
        Capabilities {
            interactive: true,
            unicode: true,
            animation: true,
            width: 80,
        }
    }

    fn render(caps: Capabilities, events: &[UiEvent<'_>]) -> String {
        let spinner = caps.spinner(0);
        let mut renderer = TerminalRenderer::new(caps, spinner);
        let mut out: Vec<u8> = Vec::new();
        for event in events {
            renderer.handle(event.clone(), &mut out);
        }
        String::from_utf8(out).expect("saida UTF-8")
    }

    /// Invariante 4 da ADR, afirmado varrendo a saida: num pipe nao sai uma
    /// unica sequencia de escape, em nenhum caminho.
    #[test]
    fn nao_tty_nao_emite_escape_algum() {
        let saida = render(
            Capabilities::PLAIN,
            &[
                UiEvent::ActivityTick,
                UiEvent::ActivityTick,
                UiEvent::ActivityTick,
                UiEvent::ActivityTick,
                UiEvent::TextDelta("oi"),
                UiEvent::ToolStarted {
                    name: "bash",
                    detail: "cargo test",
                },
                UiEvent::ToolFinished {
                    name: "bash",
                    duration: std::time::Duration::from_millis(6300),
                    success: true,
                    summary: "148 passed",
                    indice: None,
                },
                UiEvent::Warning("cuidado"),
                UiEvent::Error("quebrou"),
                UiEvent::TurnFinished,
            ],
        );
        assert!(
            !saida.contains('\x1b'),
            "escape em saida nao-TTY: {saida:?}"
        );
        assert!(saida.contains("oi"));
        assert!(saida.contains("Garra"));
    }

    /// Invariante 3 da ADR: o cursor nunca e escondido, nem no caminho rico.
    #[test]
    fn nunca_esconde_o_cursor() {
        let mut eventos = vec![UiEvent::ActivityTick; 60];
        eventos.push(UiEvent::TextDelta("resposta"));
        eventos.push(UiEvent::TurnFinished);
        let saida = render(rich(), &eventos);
        assert!(!saida.contains("\x1b[?25l"), "escondeu o cursor");
        assert!(!saida.contains("\x1b[?25h"), "mexeu no cursor");
    }

    /// O rotulo `Garra` sai **uma vez**, por mais deltas que venham.
    #[test]
    fn o_rotulo_da_resposta_sai_uma_unica_vez() {
        let saida = render(
            Capabilities::PLAIN,
            &[
                UiEvent::TextDelta("um "),
                UiEvent::TextDelta("dois "),
                UiEvent::TextDelta("tres"),
                UiEvent::TurnFinished,
            ],
        );
        assert_eq!(saida.matches("Garra").count(), 1, "{saida:?}");
        assert!(saida.ends_with("um dois tres"), "{saida:?}");
    }

    /// Turno sem delta nenhum (timeout, Ctrl+C) ainda escreve o rotulo: e ele
    /// que separa o prompt do usuario do aviso que vem depois.
    #[test]
    fn turno_sem_delta_ainda_escreve_o_rotulo() {
        let saida = render(Capabilities::PLAIN, &[UiEvent::TurnFinished]);
        assert_eq!(saida, "\nGarra\n");
    }

    /// Depois do primeiro token a linha pertence a resposta: um tick atrasado
    /// nao pode desenhar por cima dela.
    #[test]
    fn tick_depois_do_primeiro_token_nao_desenha() {
        let caps = rich();
        let mut renderer = TerminalRenderer::new(caps, caps.spinner(0));
        let mut out: Vec<u8> = Vec::new();

        // Passa a janela de aparicao para o spinner estar pintando de verdade.
        for _ in 0..10 {
            renderer.handle(UiEvent::ActivityTick, &mut out);
        }
        assert!(!out.is_empty(), "a animacao devia estar pintando");

        renderer.handle(UiEvent::TextDelta("ola"), &mut out);
        out.clear();
        for _ in 0..10 {
            renderer.handle(UiEvent::ActivityTick, &mut out);
        }
        assert!(out.is_empty(), "animou por cima da resposta: {out:?}");
    }

    /// `TurnFinished` limpa a animacao mesmo quando nao veio delta.
    #[test]
    fn fim_de_turno_limpa_a_animacao() {
        let caps = rich();
        let mut renderer = TerminalRenderer::new(caps, caps.spinner(0));
        let mut out: Vec<u8> = Vec::new();
        for _ in 0..10 {
            renderer.handle(UiEvent::ActivityTick, &mut out);
        }
        out.clear();
        renderer.handle(UiEvent::TurnFinished, &mut out);
        let saida = String::from_utf8(out).expect("UTF-8");
        assert!(saida.starts_with("\r\x1b[2K"), "nao limpou: {saida:?}");
    }

    /// Um turno novo volta a dever o rotulo — senao a segunda resposta da
    /// sessao sairia sem identificacao.
    #[test]
    fn begin_turn_devolve_o_rotulo() {
        let caps = Capabilities::PLAIN;
        let mut renderer = TerminalRenderer::new(caps, None);
        let mut out: Vec<u8> = Vec::new();
        renderer.handle(UiEvent::TextDelta("primeira"), &mut out);
        renderer.handle(UiEvent::TurnFinished, &mut out);

        renderer.begin_turn(None);
        out.clear();
        renderer.handle(UiEvent::TextDelta("segunda"), &mut out);
        let saida = String::from_utf8(out).expect("UTF-8");
        assert!(
            saida.contains("Garra"),
            "segundo turno sem rotulo: {saida:?}"
        );
    }

    /// `GARRAIA_NO_SPINNER` desliga so a animacao — o resto da interface rica
    /// continua de pe.
    #[test]
    fn animacao_desligada_nao_impede_interface_rica() {
        let caps = Capabilities {
            animation: false,
            ..rich()
        };
        assert!(caps.spinner(0).is_none());
        assert_eq!(caps.style(), Style::RICH);

        let saida = render(caps, &[UiEvent::ActivityTick, UiEvent::TurnFinished]);
        // Sem animacao, o tick nao escreve nada; o rotulo continua colorido.
        assert!(saida.contains("Garra"), "{saida:?}");
        assert!(saida.contains('\x1b'), "interface rica perdeu a cor");
    }

    /// A unificacao do #942: num terminal sem UTF-8, a conversa **e** a
    /// animacao caem para ASCII juntas. Antes cada uma decidia por conta e o
    /// usuario via `❯` ao lado de uma animacao ASCII.
    #[test]
    fn locale_sem_utf8_derruba_conversa_e_animacao_juntas() {
        let caps = Capabilities {
            interactive: true,
            unicode: false,
            animation: true,
            width: 80,
        };
        assert_eq!(caps.spinner_style(), SpinnerStyle::Ascii);
        assert_eq!(
            caps.style().user_prompt().trim_end(),
            "\x1b[32m\x1b[1m>\x1b[0m"
        );
    }

    // ── #937: ciclo de vida de ferramenta ────────────────────────────────

    /// O formato que a issue desenha: linha de inicio, resultado pendurado.
    #[test]
    fn ferramenta_bem_sucedida_desenha_inicio_e_resultado() {
        let saida = render(
            Capabilities::PLAIN,
            &[
                UiEvent::ToolStarted {
                    name: "bash",
                    detail: "cargo test",
                },
                UiEvent::ToolFinished {
                    name: "bash",
                    duration: std::time::Duration::from_millis(6300),
                    success: true,
                    summary: "148 passed",
                    indice: None,
                },
            ],
        );
        assert_eq!(saida, "* Bash cargo test\n  |- 148 passed | 6.3s\n");
    }

    #[test]
    fn ferramenta_bem_sucedida_em_unicode() {
        let caps = Capabilities {
            interactive: false,
            unicode: true,
            animation: false,
            width: 80,
        };
        let saida = render(
            caps,
            &[
                UiEvent::ToolStarted {
                    name: "file_read",
                    detail: "src/runtime.rs",
                },
                UiEvent::ToolFinished {
                    name: "file_read",
                    duration: std::time::Duration::from_millis(40),
                    success: true,
                    summary: "",
                    indice: None,
                },
            ],
        );
        assert_eq!(
            saida,
            "\u{25cf} Read src/runtime.rs\n  \u{2514}\u{2500} 40ms\n"
        );
    }

    /// Falha muda o glifo E repete o nome: depois de uma saida longa, a linha
    /// de inicio pode estar fora da tela.
    #[test]
    fn ferramenta_que_falha_muda_o_glifo_e_repete_o_nome() {
        let saida = render(
            Capabilities::PLAIN,
            &[UiEvent::ToolFinished {
                name: "bash",
                duration: std::time::Duration::from_millis(4200),
                success: false,
                summary: "error: exit 101",
                indice: None,
            }],
        );
        assert_eq!(saida, "x Bash   |- error: exit 101 | 4.2s\n");
    }

    #[test]
    fn falha_sem_resumo_ainda_diz_que_falhou() {
        let saida = render(
            Capabilities::PLAIN,
            &[UiEvent::ToolFinished {
                name: "bash",
                duration: std::time::Duration::from_secs(2),
                success: false,
                summary: "",
                indice: None,
            }],
        );
        assert!(saida.contains("falhou"), "{saida:?}");
    }

    /// Criterio de aceite da #937: a animacao e limpa antes do evento.
    #[test]
    fn animacao_e_limpa_antes_do_evento_de_ferramenta() {
        let caps = rich();
        let mut renderer = TerminalRenderer::new(caps, caps.spinner(0));
        let mut out: Vec<u8> = Vec::new();
        for _ in 0..10 {
            renderer.handle(UiEvent::ActivityTick, &mut out);
        }
        out.clear();
        renderer.handle(
            UiEvent::ToolStarted {
                name: "bash",
                detail: "ls",
            },
            &mut out,
        );
        let saida = String::from_utf8(out).expect("UTF-8");
        assert!(
            saida.starts_with("\r\x1b[2K"),
            "nao limpou a animacao antes: {saida:?}"
        );
    }

    /// A animacao para enquanto a ferramenta roda — a tela e dela.
    #[test]
    fn animacao_para_enquanto_a_ferramenta_roda() {
        let caps = rich();
        let mut renderer = TerminalRenderer::new(caps, caps.spinner(0));
        let mut out: Vec<u8> = Vec::new();
        for _ in 0..10 {
            renderer.handle(UiEvent::ActivityTick, &mut out);
        }
        renderer.handle(
            UiEvent::ToolStarted {
                name: "bash",
                detail: "ls",
            },
            &mut out,
        );
        out.clear();
        for _ in 0..10 {
            renderer.handle(UiEvent::ActivityTick, &mut out);
        }
        assert!(out.is_empty(), "animou por cima da ferramenta: {out:?}");
    }

    /// E volta quando ela termina — o "retomar o spinner depois da tool" que
    /// ficou de fora do #936 por nao haver evento para ouvir.
    #[test]
    fn animacao_volta_depois_que_a_ferramenta_termina() {
        let caps = rich();
        let mut renderer = TerminalRenderer::new(caps, caps.spinner(0));
        let mut out: Vec<u8> = Vec::new();

        // Texto primeiro: a linha passa a ser da resposta e a animacao para.
        renderer.handle(UiEvent::TextDelta("vou rodar os testes"), &mut out);
        out.clear();
        for _ in 0..10 {
            renderer.handle(UiEvent::ActivityTick, &mut out);
        }
        assert!(out.is_empty(), "animou por cima da resposta");

        renderer.handle(
            UiEvent::ToolFinished {
                name: "bash",
                duration: std::time::Duration::from_secs(1),
                success: true,
                summary: "ok",
                indice: None,
            },
            &mut out,
        );
        out.clear();
        for _ in 0..10 {
            renderer.handle(UiEvent::ActivityTick, &mut out);
        }
        assert!(
            !out.is_empty(),
            "a animacao nao voltou depois da ferramenta"
        );
    }

    /// E o rotulo `Garra` continua saindo uma vez so — retomar a animacao
    /// nao pode reimprimi-lo.
    #[test]
    fn retomar_a_animacao_nao_reimprime_o_rotulo() {
        let saida = render(
            Capabilities::PLAIN,
            &[
                UiEvent::TextDelta("antes "),
                UiEvent::ToolFinished {
                    name: "bash",
                    duration: std::time::Duration::from_secs(1),
                    success: true,
                    summary: "ok",
                    indice: None,
                },
                UiEvent::TextDelta("depois"),
                UiEvent::TurnFinished,
            ],
        );
        assert_eq!(saida.matches("Garra").count(), 1, "{saida:?}");
    }

    /// O numero tem de aparecer na linha: sem ele o `/tool <n>` do `/help`
    /// seria uma instrucao sem como ser seguida (#938).
    #[test]
    fn a_linha_da_ferramenta_mostra_o_numero_para_inspecionar() {
        let saida = render(
            Capabilities::PLAIN,
            &[UiEvent::ToolFinished {
                name: "bash",
                duration: std::time::Duration::from_millis(6300),
                success: true,
                summary: "148 passed",
                indice: Some(7),
            }],
        );
        assert!(saida.contains("#7"), "sem o numero: {saida:?}");
        assert!(saida.contains("148 passed"), "perdeu o resumo: {saida:?}");
    }

    /// Sem registro nao ha numero — imprimir um numero morto seria pior do
    /// que nao imprimir nenhum.
    #[test]
    fn sem_indice_a_linha_nao_inventa_numero() {
        let saida = render(
            Capabilities::PLAIN,
            &[UiEvent::ToolFinished {
                name: "bash",
                duration: std::time::Duration::from_millis(100),
                success: true,
                summary: "ok",
                indice: None,
            }],
        );
        assert!(!saida.contains('#'), "inventou numero: {saida:?}");
    }

    /// O modelo nao escreve comando no terminal, e a sequencia partida entre
    /// deltas e o caso que so o renderer com estado cobre (#996).
    #[test]
    fn o_texto_do_modelo_nao_injeta_sequencia_partida() {
        let saida = render(
            Capabilities::PLAIN,
            &[
                UiEvent::TextDelta("tudo bem\u{1b}"),
                UiEvent::TextDelta("[2Jainda aqui"),
                UiEvent::TurnFinished,
            ],
        );
        assert!(!saida.contains('\u{1b}'), "ESC sobreviveu: {saida:?}");
        assert!(
            saida.contains("tudo bemainda aqui"),
            "perdeu texto: {saida:?}"
        );
    }

    /// Aviso e erro tambem carregam texto de fora — corpo de erro de provedor,
    /// por exemplo.
    #[test]
    fn aviso_e_erro_tambem_sao_saneados() {
        let saida = render(
            Capabilities::PLAIN,
            &[
                UiEvent::Warning("cuidado\u{1b}[2J"),
                UiEvent::Error("falhou\u{1b}[?25l"),
                UiEvent::Hint("dica\u{1b}]0;x\u{7}"),
            ],
        );
        assert!(!saida.contains('\u{1b}'), "ESC sobreviveu: {saida:?}");
    }

    /// Um `ESC` pendurado no fim de um turno nao pode engolir o primeiro
    /// caractere do turno seguinte.
    #[test]
    fn esc_pendurado_nao_atravessa_o_fim_do_turno() {
        let mut r = TerminalRenderer::new(Capabilities::PLAIN, None);
        let mut out: Vec<u8> = Vec::new();
        r.handle(UiEvent::TextDelta("fim do turno\u{1b}"), &mut out);
        r.handle(UiEvent::TurnFinished, &mut out);
        out.clear();
        r.handle(UiEvent::TextDelta("Novo turno"), &mut out);
        assert_eq!(String::from_utf8(out).expect("utf8"), "Novo turno");
    }

    #[test]
    fn rotulo_de_ferramenta_e_o_que_o_operador_reconhece() {
        assert_eq!(tool_label("bash"), "Bash");
        assert_eq!(tool_label("file_read"), "Read");
        assert_eq!(tool_label("file_write"), "Write");
        assert_eq!(tool_label("repo_search"), "Search");
        // Ferramenta sem entrada aparece com o nome cru, nao com um erro.
        assert_eq!(tool_label("ferramenta_nova"), "ferramenta_nova");
    }

    #[test]
    fn duracao_muda_de_unidade_conforme_a_escala() {
        use std::time::Duration;
        assert_eq!(format_duration(Duration::from_millis(40)), "40ms");
        assert_eq!(format_duration(Duration::from_millis(999)), "999ms");
        assert_eq!(format_duration(Duration::from_millis(1000)), "1.0s");
        assert_eq!(format_duration(Duration::from_millis(6300)), "6.3s");
        assert_eq!(format_duration(Duration::from_secs(59)), "59.0s");
        assert_eq!(format_duration(Duration::from_secs(63)), "1m03s");
    }

    #[test]
    fn plain_e_realmente_plain() {
        assert_eq!(Capabilities::PLAIN.style(), Style::PLAIN);
        assert!(Capabilities::PLAIN.spinner(0).is_none());
    }
}
