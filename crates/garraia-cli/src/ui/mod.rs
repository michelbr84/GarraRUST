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

pub mod conversation;
pub mod spinner;

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
}

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
    prefix_written: bool,
}

impl TerminalRenderer {
    pub fn new(caps: Capabilities, spinner: Option<Spinner>) -> Self {
        Self {
            assistant_prefix: caps.style().assistant_prefix(),
            caps,
            spinner,
            prefix_written: false,
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
        }
    }

    /// Um turno novo comeca: o rotulo volta a ser devido e a animacao,
    /// se houver, recomeca do zero.
    pub fn begin_turn(&mut self, spinner: Option<Spinner>) {
        self.spinner = spinner;
        self.prefix_written = false;
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
        }
    }

    /// Um quadro da animacao — e so antes do primeiro token: depois disso a
    /// linha pertence a resposta, e um quadro colidiria com ela.
    fn tick(&mut self, out: &mut (impl io::Write + ?Sized)) {
        if self.prefix_written {
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
        let _ = write!(out, "{delta}");
        let _ = out.flush();
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
        if !self.prefix_written {
            let _ = write!(out, "{}", self.assistant_prefix);
            self.prefix_written = true;
        }
        let _ = out.flush();
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
        if self.caps.interactive {
            let _ = writeln!(out, "{separador}{color}{text}{}", conversation::RESET);
        } else {
            let _ = writeln!(out, "{separador}{text}");
        }
        let _ = out.flush();
    }
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

    #[test]
    fn plain_e_realmente_plain() {
        assert_eq!(Capabilities::PLAIN.style(), Style::PLAIN);
        assert!(Capabilities::PLAIN.spinner(0).is_none());
    }
}
