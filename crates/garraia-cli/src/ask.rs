//! GAR-579 — Non-interactive `garra ask` command.
//!
//! Separate channel from `garra chat`: parseable, no banner, no ANSI, no
//! REPL, LLM-only (no tools registered). Designed for Claude Code, CI,
//! hooks, scripts, and the stdio MCP wrapper (`garra_ask`).
//!
//! Scope cuts approved 2026-05-11:
//!   - No `--stream` (JSON one-shot).
//!   - No `--enable-tools` (LLM-only, no `bash`/`file_*`/`git_diff`).
//!   - No `--system-prompt-file` (only `--system-prompt <STR>`).
//!
//! #1612 — o núcleo (resolução de provider explícito, chamada LLM, envelope
//! `garra.ask.v1`) mora em `garraia-ask`, para a ponte MCP HTTP do gateway usar
//! o mesmo caminho. Este arquivo fica com o que é da CLI: ler stdin, a
//! autodetecção de provider (`chat::detect_provider`) e a emissão no terminal.

use std::time::Instant;

use anyhow::Result;
use garraia_config::AppConfig;
use tokio::io::AsyncReadExt;

pub(crate) use garraia_ask::{AskError, AskOptions, AskOutcome, error_envelope, success_envelope};

use crate::chat;

/// 64 KiB stdin cap. Larger inputs are rejected with `UsageError` rather
/// than silently truncated.
const STDIN_CAP_BYTES: usize = 64 * 1024;

/// GAR-579 — resolve the message from CLI arg or stdin bytes. Pure,
/// sync, no I/O — `run_ask` reads stdin async beforehand and passes the
/// bytes in, which keeps this testable without mocking a `tokio` reader.
pub(crate) fn resolve_message(
    arg: Option<String>,
    stdin_bytes: &[u8],
    cap: usize,
) -> Result<String, AskError> {
    if let Some(m) = arg {
        let trimmed = m.trim();
        if trimmed.is_empty() {
            return Err(AskError::UsageError(
                "message argument is empty".to_string(),
            ));
        }
        return Ok(trimmed.to_string());
    }
    if stdin_bytes.len() > cap {
        return Err(AskError::UsageError(format!(
            "stdin input exceeds {cap}-byte cap"
        )));
    }
    let s = String::from_utf8_lossy(stdin_bytes).trim().to_string();
    if s.is_empty() {
        return Err(AskError::UsageError(
            "no message provided (positional arg absent and stdin empty)".to_string(),
        ));
    }
    Ok(s)
}

/// Emit an error to stdout (if `--json`) or stderr (plain text) and
/// return the corresponding exit code. Never panics on JSON serialization
/// — emits a fixed fallback envelope if `serde_json::to_string` fails.
fn emit_error(err: &AskError, json: bool) -> i32 {
    if json {
        let env = error_envelope(err.kind_str(), &err.message());
        let line = serde_json::to_string(&env).unwrap_or_else(|_| {
            String::from("{\"schema\":\"garra.ask.v1\",\"ok\":false,\"error\":{\"kind\":\"io\",\"message\":\"json serialization failed\"}}")
        });
        println!("{line}");
    } else {
        eprintln!("error: {}", err.message());
    }
    err.exit_code()
}

/// GAR-583 — o `garra ask` / `garra_ask` do stdio, com a autodetecção da CLI.
///
/// Com provider explícito, a resolução e a chamada são do núcleo compartilhado
/// ([`garraia_ask::ask_oneshot`]). Sem ele, a cadeia de autodetecção da CLI
/// escolhe o provider (pode sondar o Ollama local), e o resto é o mesmo.
pub(crate) async fn ask_oneshot(config: &AppConfig, opts: AskOptions) -> AskOutcome {
    if opts.provider_override.is_some() {
        return garraia_ask::ask_oneshot(config, opts).await;
    }
    let inicio = Instant::now();
    let provedor = chat::detect_provider(
        config,
        opts.url_override.as_deref(),
        opts.model_override.as_deref(),
        opts.assume_yes,
    )
    .await;
    garraia_ask::executar(provedor, inicio, opts).await
}

/// GAR-579 — entry point invoked by `Commands::Ask` in `main.rs`.
///
/// Returns an exit code; the caller is responsible for `std::process::exit`.
/// Does not panic on provider/network errors — every failure path returns
/// a sanitized error through `emit_error`.
///
/// Invariants:
///   - **NEVER** registers a tool on the `AgentRuntime` (LLM-only).
///   - **NEVER** prints banner / ANSI / interactive prompts to stdout.
///   - Provider errors pass through `sanitize_provider_error` before
///     reaching stdout/stderr.
#[allow(clippy::too_many_arguments)]
pub async fn run_ask(
    config: AppConfig,
    message_arg: Option<String>,
    provider_override: Option<String>,
    model_override: Option<String>,
    url_override: Option<String>,
    json: bool,
    timeout_secs: u64,
    system_prompt_override: Option<String>,
    assume_yes: bool,
) -> Result<i32> {
    // 1. Resolve message — read stdin only if arg absent.
    let stdin_bytes: Vec<u8> = if message_arg.is_none() {
        let mut buf = Vec::with_capacity(8192);
        let mut limited = tokio::io::stdin().take((STDIN_CAP_BYTES + 1) as u64);
        if let Err(e) = limited.read_to_end(&mut buf).await {
            return Ok(emit_error(
                &AskError::IoError(format!("stdin read failed: {e}")),
                json,
            ));
        }
        buf
    } else {
        Vec::new()
    };
    let message = match resolve_message(message_arg, &stdin_bytes, STDIN_CAP_BYTES) {
        Ok(m) => m,
        Err(e) => return Ok(emit_error(&e, json)),
    };

    // 2. Call the shared core.
    let opts = AskOptions {
        message,
        provider_override,
        model_override,
        url_override,
        timeout_secs,
        system_prompt_override,
        assume_yes,
    };
    let outcome = ask_oneshot(&config, opts).await;

    // 3. Emit on stdout / stderr per --json flag.
    match outcome {
        AskOutcome::Success {
            answer,
            provider,
            model,
            latency_ms,
        } => {
            if json {
                let env = success_envelope(&answer, &provider, &model, latency_ms);
                let line = serde_json::to_string(&env).unwrap_or_else(|_| {
                    String::from("{\"schema\":\"garra.ask.v1\",\"ok\":false,\"error\":{\"kind\":\"io\",\"message\":\"json serialization failed\"}}")
                });
                println!("{line}");
            } else {
                println!("{}", answer.trim_end());
            }
            Ok(0)
        }
        AskOutcome::Failure(err) => Ok(emit_error(&err, json)),
    }
}

#[cfg(test)]
mod tests {
    //! GAR-579 — Pure tests. Zero rede, zero env-mutation, zero filesystem.

    use super::*;

    // ─── resolve_message ───────────────────────────────────────────────

    #[test]
    fn resolve_message_arg_wins_over_stdin() {
        let got = resolve_message(Some("from-arg".into()), b"from-stdin", 64).unwrap();
        assert_eq!(got, "from-arg");
    }

    #[test]
    fn resolve_message_uses_stdin_when_arg_absent() {
        let got = resolve_message(None, b"hello stdin", 64).unwrap();
        assert_eq!(got, "hello stdin");
    }

    #[test]
    fn resolve_message_trims_arg_whitespace() {
        let got = resolve_message(Some("   spaced   \n".into()), &[], 64).unwrap();
        assert_eq!(got, "spaced");
    }

    #[test]
    fn resolve_message_empty_arg_returns_usage_error() {
        let err = resolve_message(Some("   ".into()), &[], 64).unwrap_err();
        assert!(matches!(err, AskError::UsageError(_)));
    }

    #[test]
    fn resolve_message_no_arg_no_stdin_returns_usage_error() {
        let err = resolve_message(None, &[], 64).unwrap_err();
        assert!(matches!(err, AskError::UsageError(_)));
    }

    #[test]
    fn resolve_message_stdin_over_cap_returns_usage_error() {
        let over_cap = vec![b'a'; 65];
        let err = resolve_message(None, &over_cap, 64).unwrap_err();
        match err {
            AskError::UsageError(m) => assert!(m.contains("64")),
            other => panic!("expected UsageError, got {other:?}"),
        }
    }

    // ─── Auditoria: superfícies legíveis por máquina ───────────────────

    /// O indicador de atividade é UX de terminal interativo e não pode vazar
    /// para nenhuma superfície legível por máquina.
    ///
    /// `garra ask` (com ou sem `--json`) e o servidor MCP são consumidos por
    /// scripts, CI e outros agentes: um único quadro de animação no stdout
    /// corromperia o envelope `garra.ask.v1` ou o frame JSON-RPC. A garantia é
    /// estrutural — nenhum dos dois chama `stream_turn` —, e este teste trava
    /// a invariante em vez de deixá-la por convenção.
    #[test]
    fn machine_readable_surfaces_never_reference_the_spinner() {
        let forbidden = ["spinner", "render_frame", "SpinnerState", "SpinnerStyle"];
        for (name, source) in [
            ("ask.rs", include_str!("ask.rs")),
            ("mcp_server.rs", include_str!("mcp_server.rs")),
        ] {
            let production = source.split("#[cfg(test)]").next().unwrap_or(source);
            for needle in forbidden {
                assert!(
                    !production.contains(needle),
                    "{name} production code must not contain `{needle}` — \
                     o spinner é exclusivo do REPL interativo"
                );
            }
        }
    }
}
