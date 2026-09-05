//! Saídas do tracing do CLI — arquivo sempre completo, console limpo (#933).
//!
//! Até a v0.3.8 havia um subscriber único escrevendo em
//! `file_appender.and(RedactingWriter::stderr())`: o mesmo `EnvFilter`
//! alimentava o arquivo e o console, então todo INFO de registro de provider,
//! tools e sessão competia com o spinner e com a resposta streamada no chat
//! interativo. A separação é em dois layers com filtros independentes:
//!
//! - **arquivo** (`garraia.log`): continua recebendo tudo no nível pedido
//!   (`--log-level`, elevado por `--debug`). É o registro que se lê depurando
//!   incidente, então não perde nada.
//! - **stderr**: WARN+ por default (erro visível, ruído não); `--verbose`
//!   mostra o INFO operacional (provider, modelo, tools); `--debug` mostra o
//!   mesmo que o arquivo.
//!
//! `RUST_LOG` setado e válido vence os dois lados, como sempre venceu — quem
//! exporta `RUST_LOG=garraia_agents=trace` está depurando e espera ver o
//! resultado no console (comportamento legado de GAR-138).
//!
//! Invariante de segurança (regra absoluta 6): os DOIS caminhos continuam
//! embrulhados em redação — `RedactingMakeWriter` no arquivo,
//! `RedactingWriter::stderr()` no console. Já houve o bug de redigir só uma
//! metade (comentário em `main.rs` sobre 2026-08-29); o teste
//! `both_sinks_stay_redacted_in_main` varre o fonte para impedir regressão.

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::fmt::writer::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;

/// O que o console (stderr) mostra. O arquivo não é afetado por isto.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ConsoleMode {
    /// Default: só WARN+ chega ao terminal.
    Normal,
    /// `--verbose`: INFO operacional conciso (provider, modelo, tools).
    Verbose,
    /// `--debug`: o console espelha o arquivo.
    Debug,
}

/// `--debug` vence `--verbose` quando ambos vêm no argv.
pub(crate) fn console_mode(debug: bool, verbose: bool) -> ConsoleMode {
    if debug {
        ConsoleMode::Debug
    } else if verbose {
        ConsoleMode::Verbose
    } else {
        ConsoleMode::Normal
    }
}

/// Deriva as diretivas de filtro `(arquivo, stderr)`.
///
/// Pura: `RUST_LOG` entra como parâmetro, nunca é lido aqui. Um valor setado
/// mas vazio ou inválido é tratado como ausente — o mesmo fallback que o
/// `try_from_default_env().unwrap_or_else(...)` antigo fazia.
pub(crate) fn filter_directives(
    rust_log: Option<&str>,
    file_level: &str,
    mode: ConsoleMode,
) -> (String, String) {
    if let Some(spec) = rust_log
        && !spec.trim().is_empty()
        && EnvFilter::try_new(spec).is_ok()
    {
        return (spec.to_string(), spec.to_string());
    }
    let stderr_level = match mode {
        ConsoleMode::Debug => file_level,
        ConsoleMode::Verbose => "info",
        ConsoleMode::Normal => "warn",
    };
    (file_level.to_string(), stderr_level.to_string())
}

/// Monta o subscriber de dois layers. Os writers chegam já redigidos — este
/// módulo não conhece segredos, só filtros e formato.
///
/// Boxed porque o braço JSON e o braço texto têm tipos concretos diferentes;
/// `Box<dyn Subscriber>` é o menor denominador que `with_default` (testes) e
/// `init` (produção) aceitam igualmente.
pub(crate) fn build_subscriber<F, E>(
    file_writer: F,
    stderr_writer: E,
    file_directives: &str,
    stderr_directives: &str,
    json: bool,
) -> Box<dyn tracing::Subscriber + Send + Sync>
where
    F: for<'a> MakeWriter<'a> + Send + Sync + 'static,
    E: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    // As diretivas vêm de filter_directives (válidas por construção) ou de um
    // nível fixo; o fallback é cinto de segurança, não caminho esperado.
    let file_filter =
        EnvFilter::try_new(file_directives).unwrap_or_else(|_| EnvFilter::new("info"));
    let stderr_filter =
        EnvFilter::try_new(stderr_directives).unwrap_or_else(|_| EnvFilter::new("warn"));

    use tracing_subscriber::Layer;
    let registry = tracing_subscriber::registry();
    if json {
        Box::new(
            registry
                .with(
                    fmt::layer()
                        .json()
                        .with_writer(file_writer)
                        .with_ansi(false)
                        .with_filter(file_filter),
                )
                .with(
                    fmt::layer()
                        .json()
                        .with_writer(stderr_writer)
                        .with_ansi(false)
                        .with_filter(stderr_filter),
                ),
        )
    } else {
        Box::new(
            registry
                .with(
                    fmt::layer()
                        .with_writer(file_writer)
                        .with_ansi(false)
                        .with_filter(file_filter),
                )
                .with(
                    fmt::layer()
                        .with_writer(stderr_writer)
                        .with_ansi(false)
                        .with_filter(stderr_filter),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;
    use std::sync::{Arc, Mutex};

    /// MakeWriter afirmável: cada layer escreve num `Vec<u8>` compartilhado.
    #[derive(Clone, Default)]
    struct Sink(Arc<Mutex<Vec<u8>>>);

    impl Sink {
        fn contents(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().expect("sink poisoned")).into_owned()
        }
    }

    impl io::Write for Sink {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().expect("sink poisoned").extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Sink {
        type Writer = Sink;
        fn make_writer(&'a self) -> Sink {
            self.clone()
        }
    }

    /// Emite um probe em cada nível sob o subscriber de dois layers e devolve
    /// o que cada sink capturou.
    fn capture(rust_log: Option<&str>, file_level: &str, mode: ConsoleMode) -> (String, String) {
        let file = Sink::default();
        let stderr = Sink::default();
        let (file_dirs, stderr_dirs) = filter_directives(rust_log, file_level, mode);
        let sub = build_subscriber(
            file.clone(),
            stderr.clone(),
            &file_dirs,
            &stderr_dirs,
            false,
        );
        tracing::subscriber::with_default(sub, || {
            tracing::debug!("debug-probe");
            tracing::info!("info-probe");
            tracing::warn!("warn-probe");
        });
        (file.contents(), stderr.contents())
    }

    #[test]
    fn normal_mode_keeps_info_off_the_console_but_in_the_file() {
        let (file, stderr) = capture(None, "info", ConsoleMode::Normal);
        assert!(file.contains("info-probe"), "file must keep INFO: {file:?}");
        assert!(file.contains("warn-probe"));
        assert!(!file.contains("debug-probe"), "file level is info");
        assert!(
            !stderr.contains("info-probe"),
            "console must stay clean of INFO: {stderr:?}"
        );
        assert!(!stderr.contains("debug-probe"));
        assert!(
            stderr.contains("warn-probe"),
            "warnings must stay visible: {stderr:?}"
        );
    }

    #[test]
    fn verbose_mode_shows_info_on_the_console() {
        let (file, stderr) = capture(None, "info", ConsoleMode::Verbose);
        assert!(file.contains("info-probe"));
        assert!(stderr.contains("info-probe"));
        assert!(stderr.contains("warn-probe"));
        assert!(!stderr.contains("debug-probe"), "verbose is not debug");
    }

    #[test]
    fn debug_mode_mirrors_the_file_on_the_console() {
        let (file, stderr) = capture(None, "debug", ConsoleMode::Debug);
        for probe in ["debug-probe", "info-probe", "warn-probe"] {
            assert!(file.contains(probe), "file missing {probe}: {file:?}");
            assert!(stderr.contains(probe), "stderr missing {probe}: {stderr:?}");
        }
    }

    #[test]
    fn rust_log_still_wins_both_sinks() {
        // Escape hatch legado (GAR-138): RUST_LOG explícito é pedido de
        // depuração e vale para console e arquivo, como antes da separação.
        let (file, stderr) = capture(Some("trace"), "info", ConsoleMode::Normal);
        assert!(file.contains("debug-probe"));
        assert!(stderr.contains("debug-probe"));
    }

    #[test]
    fn empty_or_invalid_rust_log_falls_back_to_the_flags() {
        for bad in [Some(""), Some("   "), Some("not==valid==filter")] {
            let (file_dirs, stderr_dirs) = filter_directives(bad, "info", ConsoleMode::Normal);
            assert_eq!(file_dirs, "info", "RUST_LOG={bad:?}");
            assert_eq!(stderr_dirs, "warn", "RUST_LOG={bad:?}");
        }
    }

    #[test]
    fn debug_flag_beats_verbose() {
        assert_eq!(console_mode(true, true), ConsoleMode::Debug);
        assert_eq!(console_mode(false, true), ConsoleMode::Verbose);
        assert_eq!(console_mode(false, false), ConsoleMode::Normal);
    }

    #[test]
    fn log_level_flag_keeps_governing_the_file_only() {
        // `--log-level debug` sem `--debug`: o arquivo desce a debug, o
        // console continua limpo — quem quer console barulhento pede --debug.
        let (file, stderr) = capture(None, "debug", ConsoleMode::Normal);
        assert!(file.contains("debug-probe"));
        assert!(!stderr.contains("debug-probe"));
        assert!(!stderr.contains("info-probe"));
    }

    /// Regra absoluta 6: redação nos DOIS caminhos. O wiring vive em
    /// `main.rs`, então o guard varre o fonte — o mesmo padrão do
    /// `spinner.rs` para invariantes invisíveis a testes de comportamento.
    #[test]
    fn both_sinks_stay_redacted_in_main() {
        let main_src = include_str!("main.rs");
        assert!(
            main_src.contains("RedactingMakeWriter::new("),
            "o appender de arquivo deve continuar embrulhado em redação"
        );
        assert!(
            main_src.contains("RedactingWriter::stderr()"),
            "o stderr deve continuar embrulhado em redação"
        );
        assert!(
            !main_src.contains(".and(RedactingWriter::stderr())"),
            "a composição antiga de sink único não deve voltar — os dois \
             layers têm filtros independentes (#933)"
        );
    }
}
