//! GAR-579 — Non-interactive `garra ask` command.
//!
//! Separate channel from `garra chat`: parseable, no banner, no ANSI, no
//! REPL, LLM-only (no tools registered). Designed for Claude Code, CI,
//! hooks, scripts, and a future MCP wrapper.
//!
//! Scope cuts approved 2026-05-11:
//!   - No `--stream` (JSON one-shot).
//!   - No `--enable-tools` (LLM-only, no `bash`/`file_*`/`git_diff`).
//!   - No `--system-prompt-file` (only `--system-prompt <STR>`).
//!
//! Reuses GAR-576 helpers via `chat::detect_provider` and
//! `chat::select_explicit_provider`.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use anyhow::Result;
use garraia_agents::{AgentRuntime, ChatMessage};
use garraia_config::AppConfig;
use regex::Regex;
use serde_json::json;
use tokio::io::AsyncReadExt;

use crate::chat;

/// 64 KiB stdin cap. Larger inputs are rejected with `UsageError` rather
/// than silently truncated.
const STDIN_CAP_BYTES: usize = 64 * 1024;

/// Truncation threshold for error messages emitted in the JSON envelope.
/// Limits log spam from verbose provider responses while preserving
/// enough context to debug.
const ERROR_MSG_TRUNCATE: usize = 512;

/// GAR-579 — typed errors with stable `kind` strings and sysexits-style
/// exit codes. `Display` is intentionally PII-safe: the operator can
/// rely on `message()` never including raw api-key fingerprints (those
/// are scrubbed by [`sanitize_provider_error`] at the boundary).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AskError {
    /// Bad CLI usage or empty input. Exit 2.
    UsageError(String),
    /// No provider could be resolved (config + flags + env). Exit 69.
    NoProvider(String),
    /// Provider returned an error (auth, network, rate limit, etc.).
    /// Message has already been passed through `sanitize_provider_error`.
    /// Exit 69.
    ProviderError(String),
    /// LLM call exceeded the `--timeout-secs` window. Exit 124.
    Timeout(u64),
    /// I/O failure (stdin read, etc.). Exit 74.
    IoError(String),
}

impl AskError {
    pub(crate) fn exit_code(&self) -> i32 {
        match self {
            Self::UsageError(_) => 2,
            Self::NoProvider(_) | Self::ProviderError(_) => 69,
            Self::Timeout(_) => 124,
            Self::IoError(_) => 74,
        }
    }

    pub(crate) fn kind_str(&self) -> &'static str {
        match self {
            Self::UsageError(_) => "usage",
            Self::NoProvider(_) => "no_provider",
            Self::ProviderError(_) => "provider_error",
            Self::Timeout(_) => "timeout",
            Self::IoError(_) => "io",
        }
    }

    pub(crate) fn message(&self) -> String {
        match self {
            Self::UsageError(m)
            | Self::NoProvider(m)
            | Self::ProviderError(m)
            | Self::IoError(m) => m.clone(),
            Self::Timeout(s) => format!("LLM call exceeded {s}s timeout"),
        }
    }
}

static RE_OPENROUTER_KEY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"sk-or-v1-[A-Za-z0-9_\-]+").expect("RE_OPENROUTER_KEY"));
static RE_GENERIC_SK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"sk-[A-Za-z0-9_\-]{8,}").expect("RE_GENERIC_SK"));

/// GAR-579 — scrub api-key fingerprints from a provider error message
/// before it gets serialized into the JSON envelope or emitted to stderr.
///
/// Truncates to [`ERROR_MSG_TRUNCATE`] bytes to limit log spam from
/// verbose OpenAI-style error bodies. Order matters: OpenRouter-style
/// keys are matched FIRST because the generic `sk-…` pattern would
/// otherwise consume them.
///
/// Defense in depth — the real fix for provider-error redaction belongs
/// in `garraia-security::RedactingWriter` (out of scope for this PR).
pub(crate) fn sanitize_provider_error(msg: &str) -> String {
    let truncated: String = if msg.chars().count() > ERROR_MSG_TRUNCATE {
        let mut s: String = msg.chars().take(ERROR_MSG_TRUNCATE).collect();
        s.push('…');
        s
    } else {
        msg.to_string()
    };
    let s = RE_OPENROUTER_KEY.replace_all(&truncated, "sk-or-v1-[REDACTED]");
    RE_GENERIC_SK.replace_all(&s, "sk-[REDACTED]").into_owned()
}

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

/// GAR-579 — JSON envelope schema `garra.ask.v1`, success branch.
pub(crate) fn success_envelope(
    answer: &str,
    provider: &str,
    model: &str,
    latency_ms: u128,
) -> serde_json::Value {
    json!({
        "schema": "garra.ask.v1",
        "ok": true,
        "answer": answer,
        "provider": provider,
        "model": model,
        "latency_ms": latency_ms,
    })
}

/// GAR-579 — JSON envelope schema `garra.ask.v1`, error branch.
pub(crate) fn error_envelope(kind: &str, message: &str) -> serde_json::Value {
    json!({
        "schema": "garra.ask.v1",
        "ok": false,
        "error": {
            "kind": kind,
            "message": message,
        }
    })
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

/// GAR-583 — Pure input options for [`ask_oneshot`].
///
/// `message` is required (the resolved prompt). Other fields mirror the
/// CLI flags. Used by both `run_ask` (the CLI wrapper) and the MCP
/// server's `garra_ask` tool handler. No I/O is performed by code that
/// consumes this struct directly.
#[derive(Debug, Clone)]
pub(crate) struct AskOptions {
    pub message: String,
    pub provider_override: Option<String>,
    pub model_override: Option<String>,
    pub url_override: Option<String>,
    pub timeout_secs: u64,
    pub system_prompt_override: Option<String>,
}

/// GAR-583 — Pure outcome of [`ask_oneshot`].
///
/// Distinguishes the success path (carries answer + provider + model +
/// latency) from the failure path (carries the typed [`AskError`]).
/// Callers map this to whatever output format they need: `run_ask`
/// produces JSON on stdout; the MCP server packs it into a
/// `CallToolResult` text-content envelope.
#[derive(Debug, Clone)]
pub(crate) enum AskOutcome {
    Success {
        answer: String,
        provider: String,
        model: String,
        latency_ms: u128,
    },
    Failure(AskError),
}

impl AskOutcome {
    pub(crate) fn is_ok(&self) -> bool {
        matches!(self, Self::Success { .. })
    }

    /// Build the `garra.ask.v1` JSON envelope for this outcome.
    /// Shape matches `success_envelope` / `error_envelope`.
    pub(crate) fn to_envelope(&self) -> serde_json::Value {
        match self {
            Self::Success {
                answer,
                provider,
                model,
                latency_ms,
            } => success_envelope(answer, provider, model, *latency_ms),
            Self::Failure(err) => error_envelope(err.kind_str(), &err.message()),
        }
    }

    /// Process exit code for this outcome. Success → 0; Failure →
    /// typed [`AskError`] code.
    ///
    /// Not currently consumed by `run_ask` (which does its own variant
    /// match for emission), but kept as part of the public crate API for
    /// future callers and to give MCP integrators a stable mapping —
    /// the unit tests `ask_outcome_exit_code_mapping_table_driven`
    /// exercise it directly.
    #[allow(dead_code)]
    pub(crate) fn exit_code(&self) -> i32 {
        match self {
            Self::Success { .. } => 0,
            Self::Failure(err) => err.exit_code(),
        }
    }
}

/// GAR-583 — Pure async core of `garra ask`: builds a provider, calls
/// the LLM with the configured timeout, and returns the structured
/// outcome.
///
/// **Zero I/O outside of HTTP to the provider**. No `println!`,
/// `eprintln!`, stdin read, or filesystem access. Callers are
/// responsible for emitting the [`AskOutcome`] in whatever form they
/// need (CLI JSON line, MCP `CallToolResult`, etc.).
///
/// Invariants:
///   - **NEVER** registers a tool on the `AgentRuntime` (LLM-only,
///     same audit invariant from GAR-579).
///   - Provider errors pass through [`sanitize_provider_error`] before
///     being returned in the [`AskError`].
///   - Timeout is enforced via `tokio::time::timeout`; the streaming
///     channel is drained on a background task so the producer never
///     blocks on a slow consumer.
pub(crate) async fn ask_oneshot(config: &AppConfig, opts: AskOptions) -> AskOutcome {
    let start = Instant::now();

    // 1. Resolve provider.
    let (provider_name, model_name, provider) = if let Some(ref p) = opts.provider_override {
        match chat::select_explicit_provider(config, p.as_str(), opts.model_override.as_deref()) {
            Ok(triple) => triple,
            Err(e) => {
                return AskOutcome::Failure(AskError::NoProvider(sanitize_provider_error(
                    &format!("{e:#}"),
                )));
            }
        }
    } else {
        // detect_provider also handles url_override + autodetect chain
        // (see chat::detect_provider for the precedence rules — GAR-576).
        chat::detect_provider(config, opts.url_override.as_deref()).await
    };

    // 2. Build a minimal AgentRuntime — LLM only. NO tool registration.
    let mut runtime = AgentRuntime::new();
    runtime.register_provider(provider);

    let system_prompt = opts.system_prompt_override.unwrap_or_else(|| {
        "Voce e o GarraIA. Responda de forma concisa, direta e no idioma do usuario.".to_string()
    });
    runtime.set_system_prompt(system_prompt);
    runtime.set_max_tokens(4096);

    // 3. Call LLM with timeout. We use the streaming API (no non-streaming
    //    variant exists today) and drain deltas in a background task so
    //    the channel never blocks the producer.
    let session_id = format!("ask-{}", uuid::Uuid::new_v4());
    let history: Vec<ChatMessage> = Vec::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(256);
    let drain_handle = tokio::spawn(async move { while rx.recv().await.is_some() {} });

    let call = runtime.process_message_streaming(
        &session_id,
        &opts.message,
        &history,
        tx,
        Some(&model_name),
    );
    let result = tokio::time::timeout(Duration::from_secs(opts.timeout_secs), call).await;

    // Drop the drain task (channel closure ends it naturally too).
    drain_handle.abort();

    let latency_ms = start.elapsed().as_millis();

    match result {
        Ok(Ok(full)) => AskOutcome::Success {
            answer: full,
            provider: provider_name,
            model: model_name,
            latency_ms,
        },
        Ok(Err(e)) => AskOutcome::Failure(AskError::ProviderError(sanitize_provider_error(
            &format!("{e:#}"),
        ))),
        Err(_elapsed) => AskOutcome::Failure(AskError::Timeout(opts.timeout_secs)),
    }
}

/// GAR-579 — entry point invoked by `Commands::Ask` in `main.rs`.
///
/// Returns an exit code; the caller is responsible for `std::process::exit`.
/// Does not panic on provider/network errors — every failure path returns
/// a sanitized error through `emit_error`.
///
/// GAR-583 — refactored as a thin wrapper over [`ask_oneshot`]. Stdin
/// reading + JSON/text emission stay here; the pure LLM-call core moved
/// into `ask_oneshot` so the MCP server can reuse it without I/O.
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

    // 2. Call the pure core (GAR-583).
    let opts = AskOptions {
        message,
        provider_override,
        model_override,
        url_override,
        timeout_secs,
        system_prompt_override,
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

    // ─── AskError exit codes + kind labels ─────────────────────────────

    #[test]
    fn ask_error_exit_codes_match_doc() {
        let cases: &[(AskError, i32)] = &[
            (AskError::UsageError("x".into()), 2),
            (AskError::NoProvider("x".into()), 69),
            (AskError::ProviderError("x".into()), 69),
            (AskError::Timeout(60), 124),
            (AskError::IoError("x".into()), 74),
        ];
        for (err, expected) in cases {
            assert_eq!(err.exit_code(), *expected, "exit_code mismatch for {err:?}");
        }
    }

    #[test]
    fn ask_error_kind_str_stable() {
        assert_eq!(AskError::UsageError("".into()).kind_str(), "usage");
        assert_eq!(AskError::NoProvider("".into()).kind_str(), "no_provider");
        assert_eq!(
            AskError::ProviderError("".into()).kind_str(),
            "provider_error"
        );
        assert_eq!(AskError::Timeout(30).kind_str(), "timeout");
        assert_eq!(AskError::IoError("".into()).kind_str(), "io");
    }

    // ─── JSON envelopes ────────────────────────────────────────────────

    #[test]
    fn json_envelope_success_shape() {
        let env = success_envelope("hello", "openrouter", "openrouter/free", 123);
        assert_eq!(env["schema"], "garra.ask.v1");
        assert_eq!(env["ok"], true);
        assert_eq!(env["answer"], "hello");
        assert_eq!(env["provider"], "openrouter");
        assert_eq!(env["model"], "openrouter/free");
        assert_eq!(env["latency_ms"], 123);
        // No error field on success.
        assert!(env.get("error").is_none());
    }

    #[test]
    fn json_envelope_error_shape() {
        let env = error_envelope("timeout", "exceeded 30s");
        assert_eq!(env["schema"], "garra.ask.v1");
        assert_eq!(env["ok"], false);
        assert_eq!(env["error"]["kind"], "timeout");
        assert_eq!(env["error"]["message"], "exceeded 30s");
        // No answer field on error.
        assert!(env.get("answer").is_none());
    }

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

    // ─── sanitize_provider_error ───────────────────────────────────────

    #[test]
    fn sanitize_redacts_openrouter_key_fingerprint() {
        let leaked = "401 Unauthorized — key sk-or-v1-abcdefGHIJ12345 invalid";
        let out = sanitize_provider_error(leaked);
        assert!(!out.contains("sk-or-v1-abcdefGHIJ12345"));
        assert!(out.contains("sk-or-v1-[REDACTED]"));
    }

    #[test]
    fn sanitize_redacts_openai_style_key_fingerprint() {
        let leaked = "Incorrect API key provided: sk-projAbCd123xyz_456_more";
        let out = sanitize_provider_error(leaked);
        assert!(!out.contains("AbCd123xyz_456_more"));
        assert!(out.contains("sk-[REDACTED]"));
    }

    #[test]
    fn sanitize_truncates_long_messages() {
        let huge = "x".repeat(2_000);
        let out = sanitize_provider_error(&huge);
        // Must be capped to ERROR_MSG_TRUNCATE + ellipsis suffix.
        assert!(out.chars().count() <= ERROR_MSG_TRUNCATE + 1);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn sanitize_passes_clean_messages_through() {
        let benign = "Connection refused at localhost:11434";
        assert_eq!(sanitize_provider_error(benign), benign);
    }

    // ─── AskOutcome / AskOptions (GAR-583 refactor) ────────────────────

    fn make_success() -> AskOutcome {
        AskOutcome::Success {
            answer: "hello".to_string(),
            provider: "openrouter".to_string(),
            model: "openrouter/free".to_string(),
            latency_ms: 1234,
        }
    }

    fn make_failure(err: AskError) -> AskOutcome {
        AskOutcome::Failure(err)
    }

    #[test]
    fn ask_outcome_is_ok_distinguishes_variants() {
        assert!(make_success().is_ok());
        assert!(!make_failure(AskError::Timeout(30)).is_ok());
    }

    #[test]
    fn ask_outcome_to_envelope_success_shape() {
        let env = make_success().to_envelope();
        assert_eq!(env["schema"], "garra.ask.v1");
        assert_eq!(env["ok"], true);
        assert_eq!(env["answer"], "hello");
        assert_eq!(env["provider"], "openrouter");
        assert_eq!(env["model"], "openrouter/free");
        assert_eq!(env["latency_ms"], 1234);
        assert!(env.get("error").is_none());
    }

    #[test]
    fn ask_outcome_to_envelope_error_shape() {
        let env = make_failure(AskError::ProviderError("bad".to_string())).to_envelope();
        assert_eq!(env["schema"], "garra.ask.v1");
        assert_eq!(env["ok"], false);
        assert_eq!(env["error"]["kind"], "provider_error");
        assert_eq!(env["error"]["message"], "bad");
        assert!(env.get("answer").is_none());
    }

    #[test]
    fn ask_outcome_exit_code_mapping_table_driven() {
        // GAR-583 — `AskOutcome::exit_code` mirrors `AskError::exit_code`
        // on failure and returns 0 on success.
        let cases: &[(AskOutcome, i32)] = &[
            (make_success(), 0),
            (make_failure(AskError::UsageError("x".into())), 2),
            (make_failure(AskError::NoProvider("x".into())), 69),
            (make_failure(AskError::ProviderError("x".into())), 69),
            (make_failure(AskError::Timeout(60)), 124),
            (make_failure(AskError::IoError("x".into())), 74),
        ];
        for (outcome, expected) in cases {
            assert_eq!(
                outcome.exit_code(),
                *expected,
                "exit_code mismatch for {outcome:?}"
            );
        }
    }

    // ─── Audit: this module never registers a tool ─────────────────────

    /// Compile-time + read-time guarantee. Scans only the **production**
    /// portion of the file (everything before `#[cfg(test)]`) for tool-
    /// registration patterns. If a follow-up PR ever tries to slip a
    /// tool registration into `ask.rs`, this test fails loudly.
    #[test]
    fn ask_module_never_registers_a_tool() {
        let source = include_str!("ask.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        let forbidden = [
            "register_tool",
            "BashTool",
            "FileReadTool",
            "FileWriteTool",
            "GitDiffTool",
        ];
        for needle in forbidden {
            assert!(
                !production.contains(needle),
                "ask.rs production code must not contain `{needle}` (GAR-579 invariant)"
            );
        }
    }
}
